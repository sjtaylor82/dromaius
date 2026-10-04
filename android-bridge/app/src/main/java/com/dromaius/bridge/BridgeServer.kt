package com.dromaius.bridge

import android.net.LocalServerSocket
import android.net.LocalSocket
import android.util.Log
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.BufferedOutputStream
import java.io.ByteArrayOutputStream
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
                readCommands(socket)
            } catch (e: IOException) {
                Log.i(TAG, "Client disconnected: ${e.message}")
            }
            synchronized(lock) {
                if (client === socket) closeClientLocked()
            }
        }
    }

    /** Reads bounded lines so a faulty or hostile adb client cannot exhaust memory. */
    private fun readCommands(socket: LocalSocket) {
        val input = BufferedInputStream(socket.inputStream, 64 * 1024)
        val line = ByteArrayOutputStream()
        var oversized = false
        while (!isInterrupted) {
            val byte = input.read()
            if (byte == -1) return
            if (byte != '\n'.code) {
                if (!oversized && line.size() < MAX_COMMAND_BYTES) line.write(byte)
                else oversized = true
                continue
            }
            if (oversized) {
                Log.w(TAG, "Rejected oversized command")
            } else if (line.size() > 0) {
                try {
                    onCommand(JSONObject(line.toString(Charsets.UTF_8.name())))
                } catch (e: Exception) {
                    // Do not log command contents: they can contain typed passwords.
                    Log.w(TAG, "Rejected invalid command", e)
                }
            }
            line.reset()
            oversized = false
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
        private const val MAX_COMMAND_BYTES = 1024 * 1024
    }
}
