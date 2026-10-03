package com.dromaius.bridge

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.util.Log
import org.json.JSONObject
import java.io.BufferedOutputStream
import java.io.IOException
import java.io.OutputStream

/**
 * Newline-delimited JSON over an abstract-namespace local socket.
 *
 * The desktop reaches it with `adb forward tcp:<port> localabstract:<name>`.
 * Only adbd (uid shell or root) may connect, so other apps on the device
 * cannot drive the accessibility service through this socket.
 */
class BridgeServer(
    private val socketName: String,
    private val onConnected: () -> Unit,
    private val onCommand: (JSONObject) -> Unit,
) : Thread("bridge-server") {

    private val lock = Any()
    private var client: LocalSocket? = null
    private var out: OutputStream? = null

    @Volatile
    var isConnected = false
        private set

    override fun run() {
        val server = try {
            LocalServerSocket(socketName)
        } catch (e: IOException) {
            Log.e(TAG, "Cannot bind $socketName", e)
            return
        }
        while (!isInterrupted) {
            val socket = try {
                server.accept()
            } catch (e: IOException) {
                Log.e(TAG, "accept failed", e)
                continue
            }
            val uid = try { socket.peerCredentials.uid } catch (e: IOException) { -1 }
            if (uid != SHELL_UID && uid != ROOT_UID) {
                Log.w(TAG, "Rejected connection from uid $uid")
                socket.close()
                continue
            }
            synchronized(lock) {
                closeClientLocked()
                client = socket
                out = BufferedOutputStream(socket.outputStream, 64 * 1024)
                isConnected = true
            }
            onConnected()
            try {
                socket.inputStream.bufferedReader(Charsets.UTF_8).forEachLine { line ->
                    if (line.isBlank()) return@forEachLine
                    try {
                        onCommand(JSONObject(line))
                    } catch (e: Exception) {
                        Log.w(TAG, "Bad command: $line", e)
                    }
                }
            } catch (e: IOException) {
                Log.i(TAG, "Client disconnected: ${e.message}")
            }
            synchronized(lock) {
                if (client === socket) closeClientLocked()
            }
        }
    }

    fun send(message: JSONObject) {
        val bytes = (message.toString() + "\n").toByteArray(Charsets.UTF_8)
        synchronized(lock) {
            val stream = out ?: return
            try {
                stream.write(bytes)
                stream.flush()
            } catch (e: IOException) {
                closeClientLocked()
            }
        }
    }

    private fun closeClientLocked() {
        isConnected = false
        try { client?.close() } catch (_: IOException) {}
        client = null
        out = null
    }

    companion object {
        private const val TAG = "DromaiusBridge"
        private const val SHELL_UID = 2000
        private const val ROOT_UID = 0
    }
}
