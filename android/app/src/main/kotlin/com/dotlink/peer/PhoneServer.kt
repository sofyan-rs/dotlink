package com.dotlink.peer

import android.util.Log
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.BufferedInputStream
import java.io.InputStream
import java.io.OutputStream
import java.net.ServerSocket
import java.net.Socket
import java.net.URLDecoder
import javax.net.ssl.SSLServerSocket

/** A request to this phone's server. [body] holds exactly the request's Content-Length bytes. */
class PhoneRequest(
    val method: String,
    val path: String,
    val query: Map<String, String>,
    val headers: Map<String, String>,
    val body: InputStream,
    val contentLength: Long,
) {
    val bearer: String? get() = headers["authorization"]?.removePrefix("Bearer ")?.takeIf { it != headers["authorization"] }

    fun json(): JSONObject = JSONObject(body.readBytes().decodeToString())
}

class PhoneResponse(
    val status: Int,
    val body: String,
) {
    companion object {
        fun json(o: JSONObject) = PhoneResponse(200, o.toString())

        fun error(
            status: Int,
            text: String,
        ) = PhoneResponse(status, text)
    }
}

/**
 * The HTTPS server other phones send to: one request per connection, bodies by Content-Length.
 * That is all a phone needs (pairing, a file, a clipboard text), so no HTTP library.
 */
class PhoneServer(
    private val scope: CoroutineScope,
    private val handle: suspend (PhoneRequest) -> PhoneResponse,
) {
    private var socket: ServerSocket? = null
    private var job: Job? = null

    /** The port listened on, once started. */
    @Volatile var port: Int = 0
        private set

    /** Listens on the first free port from [PORT]. Returns false when none is. */
    fun start(): Boolean {
        if (job?.isActive == true) return true
        val factory = PhoneTls.serverContext().serverSocketFactory
        val server =
            (PORT until PORT + 10).firstNotNullOfOrNull { p ->
                runCatching { factory.createServerSocket(p, 16) as SSLServerSocket }.getOrNull()
            } ?: return false
        socket = server
        port = server.localPort
        job =
            scope.launch {
                while (isActive) {
                    val client = runCatching { server.accept() }.getOrNull() ?: break
                    launch { serve(client) }
                }
            }
        Log.i(TAG, "phone server on $port")
        return true
    }

    fun stop() {
        runCatching { socket?.close() }
        job?.cancel()
        socket = null
        job = null
        port = 0
    }

    private suspend fun serve(client: Socket) {
        client.use { s ->
            runCatching {
                s.soTimeout = 30_000
                s.tcpNoDelay = true
                val input = BufferedInputStream(s.getInputStream(), 64 * 1024)
                val head = readHead(input) ?: return
                val lines = head.split("\r\n")
                val (method, target) = lines.first().split(' ').let { (it.getOrNull(0) ?: "") to (it.getOrNull(1) ?: "/") }
                val headers =
                    lines
                        .drop(1)
                        .mapNotNull { l -> l.indexOf(':').takeIf { it > 0 }?.let { l.substring(0, it).trim().lowercase() to l.substring(it + 1).trim() } }
                        .toMap()
                val length = headers["content-length"]?.toLongOrNull() ?: 0
                val path = target.substringBefore('?')
                val query =
                    target
                        .substringAfter('?', "")
                        .split('&')
                        .filter { '=' in it }
                        .associate { URLDecoder.decode(it.substringBefore('='), "UTF-8") to URLDecoder.decode(it.substringAfter('='), "UTF-8") }
                val response = handle(PhoneRequest(method, path, query, headers, Limited(input, length), length))
                write(s.getOutputStream(), response)
            }.onFailure { Log.w(TAG, "phone server: ${it.message}") }
        }
    }

    /** The request line and headers, up to the blank line; null when the client hung up. */
    private fun readHead(input: InputStream): String? {
        val out = StringBuilder()
        var last4 = 0
        while (true) {
            val b = input.read()
            if (b < 0) return null
            out.append(b.toChar())
            last4 = (last4 shl 8) or b
            if (last4 == 0x0d0a0d0a) return out.substring(0, out.length - 4)
            if (out.length > 16 * 1024) return null
        }
    }

    private fun write(
        out: OutputStream,
        r: PhoneResponse,
    ) {
        val body = r.body.encodeToByteArray()
        val type = if (r.status == 200) "application/json" else "text/plain; charset=utf-8"
        val head = "HTTP/1.1 ${r.status} ${reason(r.status)}\r\nContent-Type: $type\r\nContent-Length: ${body.size}\r\nConnection: close\r\n\r\n"
        out.write(head.encodeToByteArray())
        out.write(body)
        out.flush()
    }

    private fun reason(status: Int) =
        when (status) {
            200 -> "OK"
            400 -> "Bad Request"
            401 -> "Unauthorized"
            403 -> "Forbidden"
            404 -> "Not Found"
            else -> "Error"
        }

    /** At most [left] bytes of [input]: the body, and no further. */
    private class Limited(
        private val input: InputStream,
        private var left: Long,
    ) : InputStream() {
        override fun read(): Int {
            if (left <= 0) return -1
            val b = input.read()
            if (b >= 0) left--
            return b
        }

        override fun read(
            b: ByteArray,
            off: Int,
            len: Int,
        ): Int {
            if (left <= 0) return -1
            val n = input.read(b, off, minOf(len.toLong(), left).toInt())
            if (n > 0) left -= n
            return n
        }
    }

    companion object {
        private const val TAG = "Dotlink"

        /** The PC listens on 47390; phones from 47391 on. */
        const val PORT = 47391
    }
}
