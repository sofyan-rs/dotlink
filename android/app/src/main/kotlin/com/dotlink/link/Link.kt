package com.dotlink.link

import android.annotation.SuppressLint
import android.app.Application
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Intent
import android.database.ContentObserver
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.core.app.NotificationCompat
import com.dotlink.DotlinkApp
import com.dotlink.R
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.BufferedSink
import org.json.JSONObject
import java.net.URLEncoder
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

sealed interface LinkState {
    data object Unpaired : LinkState

    data object Offline : LinkState

    data object Connecting : LinkState

    data class Online(
        val pcName: String,
    ) : LinkState
}

data class Transfer(
    val id: Long,
    val name: String,
    val incoming: Boolean,
    val size: Long,
    val done: Long = 0,
    val status: Status = Status.Running,
    val error: String? = null,
) {
    enum class Status { Running, Done, Failed }
}

/**
 * The connection to the paired PC: a control WebSocket (clipboard, incoming files), plus
 * helpers for the other channels (input, terminal, camera, mic) and file transfers.
 */
class Link(
    private val app: Application,
) {
    val prefs = Prefs(app)
    val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    val state = MutableStateFlow<LinkState>(if (prefs.paired) LinkState.Offline else LinkState.Unpaired)
    val transfers = MutableStateFlow<List<Transfer>>(emptyList())
    val cameraOn = MutableStateFlow(false)

    /** True while the Camera tab is on screen; the streamer then publishes [cameraFrame]. */
    val cameraPreviewActive = MutableStateFlow(false)

    /** The last frame sent to the PC (rotated and mirrored), for the Camera tab's preview. */
    val cameraFrame = MutableStateFlow<android.graphics.Bitmap?>(null)
    val cameraRotation = MutableStateFlow(prefs.rotation)
    val cameraMirror = MutableStateFlow(prefs.mirror)
    val cameraTorch = MutableStateFlow(false)

    /** Zoom ratio (1 = no zoom); clamped to what the camera supports. */
    val cameraZoom = MutableStateFlow(1f)
    val micOn = MutableStateFlow(false)

    /** [Prefs.themeMode], observable so every open screen re-themes at once. */
    val themeMode = MutableStateFlow(prefs.themeMode)

    /** The PC wants to cast this screen but Wireless debugging is off; LinkService asks the user. */
    val adbWifiRequests = MutableSharedFlow<Unit>(extraBufferCapacity = 2)

    /** The PC asked to turn the camera ("camera") or mic ("mic") on or off; LinkService acts on it. */
    val streamRequests = MutableSharedFlow<Pair<String, Boolean>>(extraBufferCapacity = 8)

    /** One-off messages for the UI (errors, "Clipboard sent"). */
    val notices = MutableSharedFlow<String>(extraBufferCapacity = 16)

    @Volatile private var client: OkHttpClient? = null

    @Volatile private var control: WebSocket? = null
    private var controlJob: Job? = null
    private val kicks = Channel<Unit>(Channel.CONFLATED)
    private val transferIds = AtomicLong(1)

    /** Last text that went through the link either way, so we don't bounce it back. */
    @Volatile private var lastClip: String? = null

    val host: String? get() = prefs.lastHost

    // ---------- HTTP ----------

    /** OkHttp client that trusts exactly the certificate pinned at pairing. */
    fun http(): OkHttpClient = client ?: pinnedClient(prefs.fingerprint ?: error("not paired")).also { client = it }

    private fun url(
        path: String,
        host: String = prefs.lastHost ?: prefs.hosts.first(),
    ): String {
        val h = if (host.contains(':')) "[$host]" else host
        return "https://$h:${prefs.port}$path"
    }

    private fun request(
        path: String,
        host: String? = null,
    ) = Request
        .Builder()
        .url(if (host != null) url(path, host) else url(path))
        .header("Authorization", "Bearer ${prefs.token}")

    // ---------- pairing ----------

    /** Pairs using a `dotlink://pair?...` link from the PC's QR code. Returns the PC name. */
    suspend fun pair(link: String): Result<String> =
        withContext(Dispatchers.IO) {
            runCatching {
                val uri = Uri.parse(link.trim())
                require(uri.scheme == "dotlink" && uri.host == "pair") { "Not a Dotlink pairing code" }
                val hosts = uri.getQueryParameter("h").orEmpty().split(',').filter { it.isNotBlank() }
                val port = uri.getQueryParameter("p")?.toIntOrNull() ?: 47390
                val fp = uri.getQueryParameter("fp") ?: error("Code has no fingerprint")
                val secret = uri.getQueryParameter("c") ?: error("Code has no secret")
                require(hosts.isNotEmpty()) { "Code has no address" }
                val pinned = pinnedClient(fp, timeoutSec = 5, readTimeoutSec = 10)
                val body =
                    JSONObject()
                        .put("secret", secret)
                        .put("device_id", prefs.deviceId)
                        .put("device_name", deviceName())
                        .toString()
                        .toRequestBody("application/json".toMediaType())
                var lastError: Throwable? = null
                for (host in hosts) {
                    val h = if (host.contains(':')) "[$host]" else host
                    try {
                        pinned.newCall(Request.Builder().url("https://$h:$port/pair").post(body).build()).execute().use { res ->
                            val text = res.body?.string().orEmpty()
                            if (!res.isSuccessful) error(text.ifBlank { "PC answered ${res.code}" })
                            val json = JSONObject(text)
                            prefs.hosts = hosts
                            prefs.lastHost = host
                            prefs.port = port
                            prefs.fingerprint = fp
                            prefs.token = json.getString("token")
                            prefs.serverId = json.optString("server_id")
                            prefs.serverName = json.optString("server_name", uri.getQueryParameter("n") ?: "PC")
                            client = null
                            state.value = LinkState.Offline
                            return@runCatching prefs.serverName
                        }
                    } catch (e: Exception) {
                        lastError = e
                    }
                }
                throw lastError ?: IllegalStateException("PC not reachable")
            }
        }

    fun unpair() {
        stop()
        prefs.clearPairing()
        client = null
        state.value = LinkState.Unpaired
    }

    // ---------- control connection ----------

    fun start() {
        if (!prefs.paired || controlJob?.isActive == true) return
        controlJob = scope.launch { controlLoop() }
        watchDebugSettings()
    }

    private var debugObserver: ContentObserver? = null

    /** Tells the PC when Developer options or Wireless debugging are turned on or off. */
    private fun watchDebugSettings() {
        if (debugObserver != null) return
        val observer =
            object : ContentObserver(null) {
                override fun onChange(selfChange: Boolean) = sendInfo()
            }
        val resolver = app.contentResolver
        resolver.registerContentObserver(Settings.Global.getUriFor(ADB_WIFI_ENABLED), false, observer)
        resolver.registerContentObserver(Settings.Global.getUriFor(Settings.Global.DEVELOPMENT_SETTINGS_ENABLED), false, observer)
        debugObserver = observer
    }

    private fun globalFlag(name: String) = runCatching { Settings.Global.getInt(app.contentResolver, name, 0) == 1 }.getOrDefault(false)

    /** For casting: the model (to find this phone in `adb devices`) and the debugging switches. */
    private fun sendInfo() {
        val msg =
            JSONObject()
                .put("t", "info")
                .put("model", Build.MODEL)
                .put("adb_wifi", globalFlag(ADB_WIFI_ENABLED))
                .put("dev_options", globalFlag(Settings.Global.DEVELOPMENT_SETTINGS_ENABLED))
        control?.send(msg.toString())
    }

    fun stop() {
        controlJob?.cancel()
        controlJob = null
        control?.close(1000, null)
        control = null
        if (prefs.paired) state.value = LinkState.Offline
    }

    /** Retry right away (network changed, app opened). */
    fun kick() {
        kicks.trySend(Unit)
    }

    private suspend fun controlLoop() {
        var backoff = 1_000L
        while (scope.isActive && prefs.paired) {
            if (state.value !is LinkState.Online) state.value = LinkState.Connecting
            val host = resolveHost()
            if (host != null) {
                val closed = CompletableDeferred<Unit>()
                val ws =
                    http().newWebSocket(
                        request("/ws/control", host).build(),
                        object : WebSocketListener() {
                            override fun onMessage(
                                webSocket: WebSocket,
                                text: String,
                            ) {
                                backoff = 1_000L
                                // Set here too: "hello" (which we answer) can beat the assignment below.
                                control = webSocket
                                onControl(text)
                            }

                            override fun onClosed(
                                webSocket: WebSocket,
                                code: Int,
                                reason: String,
                            ) {
                                closed.complete(Unit)
                            }

                            override fun onFailure(
                                webSocket: WebSocket,
                                t: Throwable,
                                response: Response?,
                            ) {
                                if (response?.code == 401) revoked()
                                closed.complete(Unit)
                            }
                        },
                    )
                control = ws
                try {
                    closed.await()
                } finally {
                    ws.cancel()
                    control = null
                }
            }
            if (!prefs.paired) break
            state.value = LinkState.Offline
            withTimeoutOrNull(backoff) { kicks.receive() }
            backoff = (backoff * 2).coerceAtMost(30_000L)
        }
    }

    /** First reachable address: the last good one, the ones from pairing, then mDNS. */
    private suspend fun resolveHost(): String? {
        val candidates = (listOfNotNull(prefs.lastHost) + prefs.hosts).distinct()
        for (h in candidates) if (ping(h)) return h
        val found = Discovery.find(app, prefs.serverId) ?: return null
        if (!ping(found)) return null
        prefs.lastHost = found
        return found
    }

    private fun ping(host: String): Boolean =
        try {
            val quick = http().newBuilder().connectTimeout(1500, TimeUnit.MILLISECONDS).readTimeout(3, TimeUnit.SECONDS).build()
            quick.newCall(request("/ping", host).build()).execute().use { res ->
                if (res.code == 401) revoked()
                if (res.isSuccessful) prefs.lastHost = host
                res.isSuccessful
            }
        } catch (_: Exception) {
            false
        }

    /** The PC forgot this phone (`dotlink unpair`). */
    private fun revoked() {
        prefs.clearPairing()
        client = null
        state.value = LinkState.Unpaired
        notices.tryEmit("The PC unpaired this phone")
    }

    private fun onControl(text: String) {
        val msg = runCatching { JSONObject(text) }.getOrNull() ?: return
        when (msg.optString("t")) {
            "hello" -> {
                val name = msg.optString("name", prefs.serverName)
                prefs.serverName = name
                state.value = LinkState.Online(name)
                sendInfo()
            }
            "adb_wifi" -> adbWifiRequests.tryEmit(Unit)
            "clip" -> setPhoneClipboard(msg.optString("text"))
            "camera", "mic" -> streamRequests.tryEmit(msg.optString("t") to msg.optBoolean("on"))
            "file" ->
                scope.launch {
                    download(msg.getString("id"), msg.optString("name", "file"), msg.optLong("size", -1))
                }
        }
    }

    // ---------- clipboard ----------

    private fun setPhoneClipboard(text: String) {
        if (text.isEmpty() || text == lastClip) return
        lastClip = text
        Log.i(TAG, "clipboard from PC (${text.length} chars)")
        scope.launch(Dispatchers.Main) {
            val cm = app.getSystemService(ClipboardManager::class.java)
            cm.setPrimaryClip(ClipData.newPlainText("Dotlink", text))
        }
    }

    /** Tells the PC that starting [kind] ("camera" | "mic") waits for a tap on our notification. */
    fun sendStreamNeedsTap(kind: String) {
        control?.send(JSONObject().put("t", "media").put("kind", kind).put("state", "needs_tap").toString())
    }

    /** Sends [text] to the PC clipboard. False when not connected. */
    fun sendClip(
        text: String,
        force: Boolean = true,
    ): Boolean {
        if (text.isEmpty() || (!force && text == lastClip)) return false
        val ws = control ?: return false
        lastClip = text
        return ws.send(JSONObject().put("t", "clip").put("text", text).toString())
    }

    /** Reads the phone clipboard (only works while our window has focus) and sends it. */
    fun sendPhoneClipboard(force: Boolean = true): Boolean {
        val cm = app.getSystemService(ClipboardManager::class.java)
        val text =
            cm.primaryClip
                ?.takeIf { it.itemCount > 0 }
                ?.getItemAt(0)
                ?.coerceToText(app)
                ?.toString() ?: return false
        return sendClip(text, force)
    }

    // ---------- feature WebSockets ----------

    /** Opens `wss://pc/<path>` with auth, or null when the PC is not connected. */
    fun openSocket(
        path: String,
        listener: WebSocketListener,
        headers: Map<String, String> = emptyMap(),
    ): WebSocket? {
        if (state.value !is LinkState.Online) return null
        val req = request(path).apply { headers.forEach { (k, v) -> header(k, v) } }.build()
        return http().newWebSocket(req, listener)
    }

    /** An authenticated request to the PC, or null when it is not connected. */
    fun apiRequest(
        path: String,
        headers: Map<String, String> = emptyMap(),
    ): Request.Builder? {
        if (state.value !is LinkState.Online) return null
        return request(path).apply { headers.forEach { (k, v) -> header(k, v) } }
    }

    // ---------- files ----------

    /** Uploads each document to the PC's download folder, checking its SHA-256 there. */
    suspend fun upload(uris: List<Uri>): Int =
        withContext(Dispatchers.IO) {
            var ok = 0
            for (uri in uris) {
                val (name, size) = queryName(uri)
                val t = Transfer(transferIds.getAndIncrement(), name, incoming = false, size = size)
                transfers.update { listOf(t) + it.take(30) }
                try {
                    val sha = sha256(uri)
                    val body =
                        object : RequestBody() {
                            override fun contentType() = "application/octet-stream".toMediaType()

                            override fun contentLength() = size

                            override fun writeTo(sink: BufferedSink) {
                                app.contentResolver.openInputStream(uri)!!.use { input ->
                                    val buf = ByteArray(256 * 1024)
                                    var sent = 0L
                                    while (true) {
                                        val n = input.read(buf)
                                        if (n < 0) break
                                        sink.write(buf, 0, n)
                                        sent += n
                                        progress(t.id, sent)
                                    }
                                }
                            }
                        }
                    val req =
                        request("/upload?name=" + URLEncoder.encode(name, "UTF-8"))
                            .header("X-Sha256", sha)
                            .post(body)
                            .build()
                    transferClient().newCall(req).execute().use { res ->
                        if (!res.isSuccessful) error(res.body?.string()?.ifBlank { null } ?: "PC answered ${res.code}")
                    }
                    finish(t.id, null)
                    ok++
                } catch (e: Exception) {
                    Log.w(TAG, "upload $name failed", e)
                    finish(t.id, e.message ?: e.javaClass.simpleName)
                }
            }
            ok
        }

    private suspend fun download(
        id: String,
        name: String,
        size: Long,
    ) {
        val t = Transfer(transferIds.getAndIncrement(), name, incoming = true, size = size)
        transfers.update { listOf(t) + it.take(30) }
        val resolver = app.contentResolver
        val mime = MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase()) ?: "application/octet-stream"
        val values =
            ContentValues().apply {
                put(MediaStore.Downloads.DISPLAY_NAME, name)
                put(MediaStore.Downloads.MIME_TYPE, mime)
                put(MediaStore.Downloads.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Dotlink")
                put(MediaStore.Downloads.IS_PENDING, 1)
            }
        val item = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
        if (item == null) {
            finish(t.id, "can't create file")
            return
        }
        try {
            transferClient().newCall(request("/download/$id").build()).execute().use { res ->
                if (!res.isSuccessful) error("PC answered ${res.code}")
                resolver.openOutputStream(item)!!.use { out ->
                    val input = res.body!!.byteStream()
                    val buf = ByteArray(256 * 1024)
                    var got = 0L
                    while (true) {
                        val n = input.read(buf)
                        if (n < 0) break
                        out.write(buf, 0, n)
                        got += n
                        progress(t.id, got)
                    }
                }
            }
            resolver.update(item, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            finish(t.id, null)
            notifyReceived(name, item, mime)
        } catch (e: Exception) {
            resolver.delete(item, null, null)
            Log.w(TAG, "download $name failed", e)
            finish(t.id, e.message ?: e.javaClass.simpleName)
        }
    }

    private fun transferClient() =
        http()
            .newBuilder()
            .readTimeout(60, TimeUnit.SECONDS)
            .writeTimeout(60, TimeUnit.SECONDS)
            .build()

    private var lastProgress = 0L

    private fun progress(
        id: Long,
        done: Long,
    ) {
        val now = System.currentTimeMillis()
        if (now - lastProgress < 150) return
        lastProgress = now
        transfers.update { list -> list.map { if (it.id == id) it.copy(done = done) else it } }
    }

    private fun finish(
        id: Long,
        error: String?,
    ) {
        transfers.update { list ->
            list.map {
                if (it.id != id) {
                    it
                } else if (error == null) {
                    it.copy(done = it.size.coerceAtLeast(it.done), status = Transfer.Status.Done)
                } else {
                    it.copy(status = Transfer.Status.Failed, error = error)
                }
            }
        }
    }

    @SuppressLint("MissingPermission") // Checked by NotificationManager.areNotificationsEnabled.
    private fun notifyReceived(
        name: String,
        uri: Uri,
        mime: String,
    ) {
        val nm = app.getSystemService(NotificationManager::class.java)
        if (!nm.areNotificationsEnabled()) return
        val open =
            Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        val pi = PendingIntent.getActivity(app, uri.hashCode(), open, PendingIntent.FLAG_IMMUTABLE)
        val n =
            NotificationCompat
                .Builder(app, DotlinkApp.CHANNEL_FILES)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle("Received from ${prefs.serverName}")
                .setContentText(name)
                .setContentIntent(pi)
                .setAutoCancel(true)
                .build()
        nm.notify(uri.hashCode(), n)
    }

    private fun queryName(uri: Uri): Pair<String, Long> {
        var name = uri.lastPathSegment?.substringAfterLast('/') ?: "file"
        var size = -1L
        app.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                c.getString(0)?.let { name = it }
                if (!c.isNull(1)) size = c.getLong(1)
            }
        }
        return name to size
    }

    private fun sha256(uri: Uri): String {
        val md = MessageDigest.getInstance("SHA-256")
        app.contentResolver.openInputStream(uri)!!.use { input ->
            val buf = ByteArray(256 * 1024)
            while (true) {
                val n = input.read(buf)
                if (n < 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().toHex()
    }

    private fun deviceName(): String =
        Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME)
            ?: "${Build.MANUFACTURER} ${Build.MODEL}"

    companion object {
        private const val TAG = "Dotlink"

        /** Settings.Global.ADB_WIFI_ENABLED, hidden from the public SDK. */
        private const val ADB_WIFI_ENABLED = "adb_wifi_enabled"

        /** Trusts only the certificate whose SHA-256 is [fingerprint]; host names are ignored. */
        fun pinnedClient(
            fingerprint: String,
            timeoutSec: Long = 10,
            /** 0 = no limit: idle WebSockets would otherwise time out; the ping interval detects dead links. */
            readTimeoutSec: Long = 0,
        ): OkHttpClient {
            val tm =
                object : X509TrustManager {
                    override fun checkClientTrusted(
                        chain: Array<out X509Certificate>,
                        authType: String,
                    ) = throw CertificateException("client certs not used")

                    override fun checkServerTrusted(
                        chain: Array<out X509Certificate>,
                        authType: String,
                    ) {
                        val actual = MessageDigest.getInstance("SHA-256").digest(chain.first().encoded).toHex()
                        if (!actual.equals(fingerprint, ignoreCase = true)) {
                            throw CertificateException("PC certificate changed. Pair again.")
                        }
                    }

                    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
                }
            val ssl = SSLContext.getInstance("TLS").apply { init(null, arrayOf(tm), null) }
            return OkHttpClient
                .Builder()
                .sslSocketFactory(ssl.socketFactory, tm)
                .hostnameVerifier { _, _ -> true } // Safe: the certificate itself is pinned.
                .connectTimeout(timeoutSec, TimeUnit.SECONDS)
                .readTimeout(readTimeoutSec, TimeUnit.SECONDS)
                .pingInterval(15, TimeUnit.SECONDS)
                .build()
        }
    }
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }
