package com.dromaius.bridge

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.app.Notification
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.graphics.Path
import android.graphics.Point
import android.graphics.Rect
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.view.Display
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import android.view.accessibility.AccessibilityNodeInfo.AccessibilityAction
import android.view.accessibility.AccessibilityWindowInfo
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.atomic.AtomicBoolean

class BridgeService : AccessibilityService() {

    private lateinit var worker: Handler
    private val server: BridgeServer? get() = sharedServer
    private val snapshotScheduled = AtomicBoolean(false)

    // Worker-thread state: nodes from the latest snapshot, keyed by the ids we sent.
    private var nodeCache = HashMap<Long, AccessibilityNodeInfo>()
    private var liveRegionText = HashMap<Long, String>()

    override fun onServiceConnected() {
        val thread = HandlerThread("bridge-worker").apply { start() }
        worker = Handler(thread.looper)
        current = this
        // Typing happens on the PC keyboard; keep the on-screen keyboard out of the way.
        softKeyboardController.setShowMode(SHOW_MODE_HIDDEN)
        registerReceiver(
            packagesChanged,
            IntentFilter().apply {
                addAction(Intent.ACTION_PACKAGE_ADDED)
                addAction(Intent.ACTION_PACKAGE_REMOVED)
                addAction(Intent.ACTION_PACKAGE_CHANGED)
                addDataScheme("package")
            },
            RECEIVER_NOT_EXPORTED,
        )
        // Android may unbind and rebind the service (e.g. while UiAutomation
        // runs). The socket server outlives instances; just re-attach to it.
        if (ensureServer().isConnected) onClientConnected()
    }

    /** Keeps the desktop's app list current as apps are installed or removed. */
    private val packagesChanged = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (server?.isConnected == true) worker.post { sendApps() }
        }
    }

    override fun onDestroy() {
        try { unregisterReceiver(packagesChanged) } catch (_: IllegalArgumentException) {}
        if (current === this) current = null
        if (::worker.isInitialized) worker.looper.quitSafely()
        super.onDestroy()
    }

    private fun onClientConnected() {
        worker.post { sendHello(); sendApps(); sendSnapshot() }
    }

    private fun onClientCommand(cmd: JSONObject) {
        worker.post { handleCommand(cmd) }
    }

    override fun onInterrupt() {}

    override fun onAccessibilityEvent(event: AccessibilityEvent) {
        val srv = server ?: return
        if (!srv.isConnected) return
        when (event.eventType) {
            AccessibilityEvent.TYPE_ANNOUNCEMENT -> {
                announce(event.text.joinToString(" "))
            }
            AccessibilityEvent.TYPE_NOTIFICATION_STATE_CHANGED -> {
                // Toasts arrive as notification-state events without a Notification.
                when (val data = event.parcelableData) {
                    is Notification -> notificationPosted(event.packageName?.toString() ?: "", data)
                    else -> announce(event.text.joinToString(" "))
                }
            }
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> {
                val msg = JSONObject()
                    .put("type", "windowChanged")
                    .put("package", event.packageName?.toString() ?: "")
                    .put("title", event.text.joinToString(" "))
                srv.send(msg)
                scheduleSnapshot()
            }
            else -> scheduleSnapshot()
        }
    }

    private var lastNotification = ""

    /** Passes a new notification to the desktop, which announces it. */
    private fun notificationPosted(pkg: String, n: Notification) {
        // Ongoing ones (music, downloads, running services) update constantly.
        if (n.flags and (Notification.FLAG_ONGOING_EVENT or Notification.FLAG_GROUP_SUMMARY) != 0) return
        val extras = n.extras
        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString() ?: ""
        val text = (extras.getCharSequence(Notification.EXTRA_BIG_TEXT)
            ?: extras.getCharSequence(Notification.EXTRA_TEXT))?.toString() ?: ""
        if (title.isBlank() && text.isBlank()) return
        val app = try {
            packageManager.getApplicationLabel(packageManager.getApplicationInfo(pkg, 0)).toString()
        } catch (_: PackageManager.NameNotFoundException) {
            pkg
        }
        // Apps often re-post the same notification; announce it once.
        val key = "$pkg|$title|$text"
        if (key == lastNotification) return
        lastNotification = key
        server?.send(
            JSONObject().put("type", "notification").put("app", app).put("title", title).put("text", text)
        )
    }

    private fun announce(text: String) {
        if (text.isBlank()) return
        server?.send(JSONObject().put("type", "announce").put("text", text))
    }

    private fun scheduleSnapshot(delayMs: Long = SNAPSHOT_DEBOUNCE_MS) {
        if (snapshotScheduled.compareAndSet(false, true)) {
            worker.postDelayed({
                snapshotScheduled.set(false)
                sendSnapshot()
            }, delayMs)
        }
    }

    private fun displaySize(): Point {
        val dm = getSystemService(DisplayManager::class.java)
        val size = Point()
        @Suppress("DEPRECATION")
        dm.getDisplay(Display.DEFAULT_DISPLAY)?.getRealSize(size)
        return size
    }

    private fun sendHello() {
        val size = displaySize()
        server?.send(
            JSONObject()
                .put("type", "hello")
                .put("version", PROTOCOL_VERSION)
                .put("sdk", Build.VERSION.SDK_INT)
                .put("device", "${Build.MANUFACTURER} ${Build.MODEL}")
                .put("width", size.x)
                .put("height", size.y)
                .put("home", homePackage())
        )
    }

    private fun homePackage(): String {
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
        return packageManager.resolveActivity(intent, PackageManager.MATCH_DEFAULT_ONLY)
            ?.activityInfo?.packageName ?: ""
    }

    /** Launchable apps, as the home screen would show them. */
    private fun sendApps() {
        val pm = packageManager
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        val apps = pm.queryIntentActivities(intent, 0)
            .filter { it.activityInfo.packageName != packageName }
            .distinctBy { it.activityInfo.packageName }
            .map {
                Triple(
                    it.activityInfo.packageName,
                    it.loadLabel(pm).toString(),
                    it.activityInfo.applicationInfo.nativeLibraryDir
                        ?.substringAfterLast('/') ?: ""
                )
            }
            .sortedBy { it.second.lowercase() }
        val arr = JSONArray()
        for ((pkg, label, nativeAbi) in apps) {
            arr.put(
                JSONObject()
                    .put("package", pkg)
                    .put("label", label)
                    .put("nativeAbi", nativeAbi)
            )
        }
        server?.send(JSONObject().put("type", "apps").put("apps", arr))
    }

    private fun launch(pkg: String): String? {
        val intent = packageManager.getLaunchIntentForPackage(pkg) ?: return "$pkg cannot be opened"
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
        startActivity(intent)
        return null
    }

    // ---------------------------------------------------------------- snapshot

    private class Budget(var remaining: Int)

    private fun sendSnapshot() {
        val srv = server ?: return
        if (!srv.isConnected) return
        val cache = HashMap<Long, AccessibilityNodeInfo>()
        val live = HashMap<Long, String>()
        val budget = Budget(MAX_NODES)
        val windowsJson = JSONArray()
        for (w in windows) {
            val root = w.root ?: continue
            val bounds = Rect().also { w.getBoundsInScreen(it) }
            val wj = JSONObject()
                .put("id", w.id)
                .put("type", windowType(w.type))
                .put("title", w.title?.toString() ?: "")
                .put("package", root.packageName?.toString() ?: "")
                .put("layer", w.layer)
                .put("active", w.isActive)
                .put("focused", w.isFocused)
                .put("bounds", rectJson(bounds))
            serialize(root, 0, cache, live, budget)?.let { wj.put("root", it) }
            windowsJson.put(wj)
        }
        val size = displaySize()
        srv.send(
            JSONObject()
                .put("type", "tree")
                .put("width", size.x)
                .put("height", size.y)
                .put("windows", windowsJson)
        )
        // Announce live-region changes, like TalkBack does.
        for ((id, text) in live) {
            val before = liveRegionText[id]
            if (before != null && before != text && text.isNotBlank()) announce(text)
        }
        liveRegionText = live
        nodeCache = cache
    }

    private fun serialize(
        n: AccessibilityNodeInfo,
        depth: Int,
        cache: HashMap<Long, AccessibilityNodeInfo>,
        live: HashMap<Long, String>,
        budget: Budget,
    ): JSONObject? {
        if (depth > MAX_DEPTH || budget.remaining <= 0) return null
        budget.remaining--
        val id = nodeId(n)
        cache[id] = n
        val o = JSONObject().put("id", id)
        n.className?.let { o.put("cls", it.toString()) }
        // Password values must never cross the bridge or enter debug snapshots.
        // The desktop keeps what the user is actively typing in its local field.
        if (!n.isPassword) n.text?.let { if (it.isNotEmpty()) o.put("text", it.toString()) }
        n.contentDescription?.let { if (it.isNotEmpty()) o.put("desc", it.toString()) }
        n.hintText?.let { if (it.isNotEmpty()) o.put("hint", it.toString()) }
        n.stateDescription?.let { if (it.isNotEmpty()) o.put("state", it.toString()) }
        n.paneTitle?.let { if (it.isNotEmpty()) o.put("pane", it.toString()) }
        n.tooltipText?.let { if (it.isNotEmpty()) o.put("tooltip", it.toString()) }
        n.error?.let { if (it.isNotEmpty()) o.put("error", it.toString()) }
        n.viewIdResourceName?.let { o.put("viewId", it) }
        n.extras?.getCharSequence(ROLE_DESCRIPTION_KEY)?.let { o.put("roleDesc", it.toString()) }
        o.put("bounds", rectJson(Rect().also { n.getBoundsInScreen(it) }))

        // Kept rather than dropped: Android sometimes reports controls that are
        // on screen as invisible (e.g. mid-animation); the desktop decides.
        if (!n.isVisibleToUser) o.put("invisible", true)
        if (n.isClickable) o.put("clickable", true)
        if (n.isLongClickable) o.put("longClickable", true)
        if (n.isFocusable) o.put("focusable", true)
        if (n.isFocused) o.put("focused", true)
        if (n.isAccessibilityFocused) o.put("a11yFocused", true)
        if (n.isCheckable) o.put("checkable", true)
        @Suppress("DEPRECATION")
        if (n.isChecked) o.put("checked", true)
        if (n.isEditable) o.put("editable", true)
        if (n.isPassword) o.put("password", true)
        if (n.isScrollable) o.put("scrollable", true)
        if (n.isSelected) o.put("selected", true)
        if (!n.isEnabled) o.put("disabled", true)
        if (n.isHeading) o.put("heading", true)
        if (n.isMultiLine) o.put("multiLine", true)
        if (n.isShowingHintText) o.put("showingHint", true)
        if (n.isEditable) {
            o.put("selStart", n.textSelectionStart)
            o.put("selEnd", n.textSelectionEnd)
            o.put("inputType", n.inputType)
        }

        val actions = JSONArray()
        val custom = JSONArray()
        for (a in n.actionList) {
            if (a.id > STANDARD_ACTION_MAX && a.label != null) {
                custom.put(JSONObject().put("id", a.id).put("label", a.label.toString()))
            } else {
                actions.put(a.id)
            }
        }
        o.put("actions", actions)
        if (custom.length() > 0) o.put("customActions", custom)

        n.collectionInfo?.let {
            o.put("collection", JSONObject().put("rows", it.rowCount).put("cols", it.columnCount))
        }
        n.collectionItemInfo?.let {
            o.put(
                "item",
                JSONObject().put("row", it.rowIndex).put("col", it.columnIndex).put("heading", it.isHeading)
            )
        }
        n.rangeInfo?.let {
            o.put(
                "range",
                JSONObject().put("kind", it.type).put("min", it.min.toDouble())
                    .put("max", it.max.toDouble()).put("current", it.current.toDouble())
            )
        }
        if (n.liveRegion != 0) {
            o.put("live", n.liveRegion)
            live[id] = listOfNotNull(n.text, n.contentDescription, n.stateDescription).joinToString(" ")
        }

        val children = JSONArray()
        for (i in 0 until n.childCount) {
            val child = n.getChild(i) ?: continue
            serialize(child, depth + 1, cache, live, budget)?.let { children.put(it) }
        }
        if (children.length() > 0) o.put("children", children)
        return o
    }

    private fun nodeId(n: AccessibilityNodeInfo): Long {
        // AccessibilityNodeInfo.hashCode() is derived from the source view id and
        // window id, so it is stable for the lifetime of the view.
        return ((n.windowId.toLong() and 0x7fffffffL) shl 32) or (n.hashCode().toLong() and 0xffffffffL)
    }

    private fun rectJson(r: Rect) = JSONArray().put(r.left).put(r.top).put(r.right).put(r.bottom)

    private fun windowType(type: Int) = when (type) {
        AccessibilityWindowInfo.TYPE_APPLICATION -> "application"
        AccessibilityWindowInfo.TYPE_INPUT_METHOD -> "inputMethod"
        AccessibilityWindowInfo.TYPE_SYSTEM -> "system"
        AccessibilityWindowInfo.TYPE_ACCESSIBILITY_OVERLAY -> "accessibilityOverlay"
        AccessibilityWindowInfo.TYPE_SPLIT_SCREEN_DIVIDER -> "splitScreenDivider"
        else -> "other"
    }

    // ---------------------------------------------------------------- commands

    private fun handleCommand(cmd: JSONObject) {
        val req = cmd.optLong("req", 0)
        val result = try {
            when (cmd.optString("type")) {
                "refresh" -> { sendSnapshot(); null }
                "release" -> touchUp()
                "apps" -> { sendApps(); null }
                "launch" -> launch(cmd.getString("package"))
                "action" -> performNodeAction(cmd)
                "global" -> performGlobal(cmd.optString("action"))
                "tap" -> { tap(cmd.getDouble("x").toFloat(), cmd.getDouble("y").toFloat(), 50); null }
                else -> "unknown command"
            }
        } catch (e: Exception) {
            e.message ?: e.toString()
        }
        if (req != 0L) {
            val reply = JSONObject().put("type", "result").put("req", req).put("ok", result == null)
            if (result != null) reply.put("error", result)
            server?.send(reply)
        }
        scheduleSnapshot()
    }

    /** Returns null on success, or an error message. */
    private fun performNodeAction(cmd: JSONObject): String? {
        val node = nodeCache[cmd.getLong("id")] ?: return "node not found"
        if (!node.refresh()) return "node is gone"
        return when (val action = cmd.getString("action")) {
            "click" -> clickOrTap(node, long = false)
            "longClick" -> clickOrTap(node, long = true)
            "touchDown" -> touchDown(node)
            "focus" -> ok(node.performAction(AccessibilityNodeInfo.ACTION_FOCUS))
            "a11yFocus" -> ok(node.performAction(AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS))
            // Best effort: returns false when the node is already fully visible.
            "showOnScreen" -> { node.performAction(AccessibilityAction.ACTION_SHOW_ON_SCREEN.id); null }
            "scrollForward" -> scroll(node, AccessibilityNodeInfo.ACTION_SCROLL_FORWARD)
            "scrollBackward" -> scroll(node, AccessibilityNodeInfo.ACTION_SCROLL_BACKWARD)
            "imeEnter" -> ok(node.performAction(AccessibilityAction.ACTION_IME_ENTER.id))
            "expand" -> ok(node.performAction(AccessibilityNodeInfo.ACTION_EXPAND))
            "collapse" -> ok(node.performAction(AccessibilityNodeInfo.ACTION_COLLAPSE))
            "dismiss" -> ok(node.performAction(AccessibilityNodeInfo.ACTION_DISMISS))
            "setText" -> {
                val args = Bundle()
                args.putCharSequence(
                    AccessibilityNodeInfo.ACTION_ARGUMENT_SET_TEXT_CHARSEQUENCE,
                    cmd.optString("text", "")
                )
                if (!node.performAction(AccessibilityNodeInfo.ACTION_SET_TEXT, args)) {
                    "set text failed"
                } else if (cmd.has("selStart")) {
                    setSelection(node, cmd.getInt("selStart"), cmd.getInt("selEnd"))
                } else null
            }
            "setSelection" -> setSelection(node, cmd.getInt("selStart"), cmd.getInt("selEnd"))
            "setProgress" -> {
                val args = Bundle()
                args.putFloat(AccessibilityNodeInfo.ACTION_ARGUMENT_PROGRESS_VALUE, cmd.getDouble("value").toFloat())
                ok(node.performAction(AccessibilityAction.ACTION_SET_PROGRESS.id, args))
            }
            "custom" -> ok(node.performAction(cmd.getInt("actionId")))
            else -> "unknown action $action"
        }
    }

    private fun ok(success: Boolean): String? = if (success) null else "action failed"

    private fun setSelection(node: AccessibilityNodeInfo, start: Int, end: Int): String? {
        val args = Bundle()
        args.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_START_INT, start)
        args.putInt(AccessibilityNodeInfo.ACTION_ARGUMENT_SELECTION_END_INT, end)
        return ok(node.performAction(AccessibilityNodeInfo.ACTION_SET_SELECTION, args))
    }

    private fun clickOrTap(node: AccessibilityNodeInfo, long: Boolean): String? {
        val action = if (long) AccessibilityNodeInfo.ACTION_LONG_CLICK else AccessibilityNodeInfo.ACTION_CLICK
        var target: AccessibilityNodeInfo? = node
        while (target != null) {
            if (target.actionList.any { it.id == action }) {
                if (target.performAction(action)) return null
                break
            }
            target = target.parent
        }
        // No accessible click action: fall back to a real touch at the node's center.
        val r = Rect().also { node.getBoundsInScreen(it) }
        if (r.isEmpty) return "nothing to click"
        tap(r.exactCenterX(), r.exactCenterY(), if (long) 800 else 50)
        return null
    }

    private fun scroll(node: AccessibilityNodeInfo, action: Int): String? {
        var target: AccessibilityNodeInfo? = node
        while (target != null) {
            if (target.actionList.any { it.id == action } && target.performAction(action)) return null
            target = target.parent
        }
        return "cannot scroll"
    }

    // A touch held down for push-to-talk, until touchUp.
    private var held: GestureDescription.StrokeDescription? = null
    private var heldX = 0f
    private var heldY = 0f

    /** Puts a finger down on the node and keeps it there (push-to-talk). */
    private fun touchDown(node: AccessibilityNodeInfo): String? {
        val r = Rect().also { node.getBoundsInScreen(it) }
        if (r.isEmpty) return "nothing to press"
        touchUp()
        heldX = r.exactCenterX()
        heldY = r.exactCenterY()
        val path = Path().apply { moveTo(heldX, heldY) }
        // willContinue = true: the finger stays down after this stroke.
        val stroke = GestureDescription.StrokeDescription(path, 0, 100, true)
        held = stroke
        dispatchGesture(GestureDescription.Builder().addStroke(stroke).build(), null, null)
        return null
    }

    /** Lifts the finger put down by touchDown. */
    private fun touchUp(): String? {
        val stroke = held ?: return null
        held = null
        val path = Path().apply { moveTo(heldX, heldY) }
        val end = stroke.continueStroke(path, 0, 50, false)
        dispatchGesture(GestureDescription.Builder().addStroke(end).build(), null, null)
        return null
    }

    private fun tap(x: Float, y: Float, durationMs: Long) {
        val path = Path().apply { moveTo(x, y) }
        val gesture = GestureDescription.Builder()
            .addStroke(GestureDescription.StrokeDescription(path, 0, durationMs))
            .build()
        dispatchGesture(gesture, null, null)
    }

    private fun performGlobal(action: String): String? {
        val id = when (action) {
            "back" -> GLOBAL_ACTION_BACK
            "home" -> GLOBAL_ACTION_HOME
            "recents" -> GLOBAL_ACTION_RECENTS
            "notifications" -> GLOBAL_ACTION_NOTIFICATIONS
            "quickSettings" -> GLOBAL_ACTION_QUICK_SETTINGS
            "powerDialog" -> GLOBAL_ACTION_POWER_DIALOG
            "allApps" -> GLOBAL_ACTION_ACCESSIBILITY_ALL_APPS
            else -> return "unknown global action $action"
        }
        return ok(performGlobalAction(id))
    }

    companion object {
        @Volatile
        private var current: BridgeService? = null
        private var sharedServer: BridgeServer? = null

        @Synchronized
        private fun ensureServer(): BridgeServer =
            sharedServer ?: BridgeServer(
                SOCKET_NAME,
                onConnected = { current?.onClientConnected() },
                onCommand = { cmd -> current?.onClientCommand(cmd) },
            ).also {
                sharedServer = it
                it.start()
            }

        const val SOCKET_NAME = "dromaius_bridge"
        const val PROTOCOL_VERSION = 1
        private const val SNAPSHOT_DEBOUNCE_MS = 60L
        private const val MAX_NODES = 4000
        private const val MAX_DEPTH = 80
        // Standard action ids are bit flags up to 0x00200000; framework AccessibilityAction
        // ids (e.g. ACTION_SHOW_ON_SCREEN = 0x01020036) are resource ids.
        private const val STANDARD_ACTION_MAX = 0x01ffffff
        private const val ROLE_DESCRIPTION_KEY = "AccessibilityNodeInfo.roleDescription"
    }
}
