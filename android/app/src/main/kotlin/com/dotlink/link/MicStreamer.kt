package com.dotlink.link

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString.Companion.toByteString
import org.json.JSONObject

/** Streams the phone microphone (48 kHz mono PCM) to the PC, where it is "Dotlink Mic". */
class MicStreamer(
    private val link: Link,
    private val onStopped: (error: String?) -> Unit,
) {
    private var socket: WebSocket? = null
    private var thread: Thread? = null

    @Volatile private var stopped = false

    @SuppressLint("MissingPermission") // The service only starts us after RECORD_AUDIO is granted.
    fun start() {
        socket =
            link.openSocket(
                "/ws/mic",
                object : WebSocketListener() {
                    override fun onOpen(
                        webSocket: WebSocket,
                        response: Response,
                    ) {
                        webSocket.send(JSONObject().put("rate", RATE).put("ch", 1).toString())
                    }

                    override fun onMessage(
                        webSocket: WebSocket,
                        text: String,
                    ) {
                        val msg = JSONObject(text)
                        if (msg.optBoolean("ok")) record(webSocket)
                        msg.optString("error").takeIf { it.isNotEmpty() }?.let { stop(it) }
                    }

                    override fun onFailure(
                        webSocket: WebSocket,
                        t: Throwable,
                        response: Response?,
                    ) = stop(t.message ?: "connection lost")

                    override fun onClosed(
                        webSocket: WebSocket,
                        code: Int,
                        reason: String,
                    ) = stop(null)
                },
            )
        if (socket == null) stop("PC not connected")
    }

    @SuppressLint("MissingPermission")
    private fun record(ws: WebSocket) {
        thread =
            Thread {
                val chunk = RATE / 100 * 2 // 10 ms of 16-bit mono: each chunk waits that long to fill
                val minBuf = AudioRecord.getMinBufferSize(RATE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
                val rec =
                    try {
                        AudioRecord(
                            // Voice processing: echo cancelling and noise suppression, like a call.
                            MediaRecorder.AudioSource.VOICE_COMMUNICATION,
                            RATE,
                            AudioFormat.CHANNEL_IN_MONO,
                            AudioFormat.ENCODING_PCM_16BIT,
                            maxOf(minBuf, chunk * 8),
                        )
                    } catch (e: Exception) {
                        stop(e.message ?: "microphone unavailable")
                        return@Thread
                    }
                val buf = ByteArray(chunk)
                try {
                    rec.startRecording()
                    while (!stopped) {
                        val n = rec.read(buf, 0, buf.size)
                        // A backlog (Wi-Fi stall) is dropped rather than played late forever.
                        if (n > 0 && ws.queueSize() < chunk * 4) ws.send(buf.toByteString(0, n))
                    }
                } finally {
                    rec.stop()
                    rec.release()
                }
            }.apply {
                name = "dotlink-mic"
                start()
            }
    }

    fun stop(error: String? = null) {
        if (stopped) return
        stopped = true
        socket?.close(1000, null)
        socket = null
        onStopped(error)
    }

    companion object {
        const val RATE = 48_000
    }
}
