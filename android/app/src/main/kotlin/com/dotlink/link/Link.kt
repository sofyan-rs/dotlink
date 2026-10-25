package com.dotlink.link

import android.annotation.SuppressLint
import android.app.Application
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.database.ContentObserver
import android.net.Uri
import android.os.BatteryManager
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.provider.Settings
import android.util.Log
import android.webkit.MimeTypeMap
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
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
import java.net.InetAddress
import java.net.Socket
import java.net.URLEncoder
import java.security.MessageDigest
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.UUID
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicLong
import javax.net.SocketFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.X509TrustManager

sealed interface LinkState {
    data object Unpaired : LinkState

    data object Offline : LinkState

    /** "Disconnect for now": no retrying, and opening the app doesn't reconnect. */
    data object Disconnected : LinkState

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
    /** The other phone's name; null for a transfer with the PC. */
    val phone: String? = null,
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

    /** Several PCs are paired and none was picked since the app started: ask which one. */
    val choosePc = MutableStateFlow(prefs.pcs.size > 1)
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

    /** The address the link uses right now; [USB_HOST] over the cable. */
    @Volatile private var currentHost: String? = null

    val host: String? get() = currentHost ?: prefs.lastHost

    /** Connected through the USB cable (`adb reverse` on the PC) rather than Wi-Fi. */
    val viaUsb: Boolean get() = currentHost == USB_HOST

    // ---------- HTTP ----------

    /** OkHttp client that trusts exactly the certificate pinned at pairing. */
    fun http(): OkHttpClient = client ?: pinnedClient(prefs.fingerprint ?: error("not paired")).also { client = it }

    private fun url(
        path: String,
        host: String = currentHost ?: prefs.lastHost ?: prefs.hosts.first(),
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
                // The cable first: works without Wi-Fi when the PC has set up `adb reverse`.
                for (host in listOf(USB_HOST) + hosts) {
                    val h = if (host.contains(':')) "[$host]" else host
                    try {
                        pinned.newCall(Request.Builder().url("https://$h:$port/pair").post(body).build()).execute().use { res ->
                            val text = res.body?.string().orEmpty()
                            if (!res.isSuccessful) error(text.ifBlank { "PC answered ${res.code}" })
                            val json = JSONObject(text)
                            val serverId = json.optString("server_id")
                            prefs.addPc(
                                Pc(
                                    id = serverId.ifEmpty { fp },
                                    name = json.optString("server_name", uri.getQueryParameter("n") ?: "PC"),
                                    serverId = serverId,
                                    hosts = hosts,
                                    lastHost = if (host == USB_HOST) hosts.first() else host,
                                    port = port,
                                    fingerprint = fp,
                                    token = json.getString("token"),
                                ),
                            )
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

    // ---------- pairing over the cable ----------

    /** Set while this phone waits for the PC on the cable to send back a pairing link. */
    @Volatile private var usbPairNonce: String? = null

    /**
     * The name of the Dotlink PC on the other end of a USB cable (`adb reverse` set up), or
     * null. The certificate can't be pinned yet (nothing is paired), so nothing secret is
     * sent on this connection: see [requestUsbPairing].
     */
    suspend fun usbPc(): String? =
        withContext(Dispatchers.IO) {
            runCatching {
                probeClient.newCall(Request.Builder().url("https://$USB_HOST:$DEFAULT_PORT/pair/usb").build()).execute().use { res ->
                    if (res.isSuccessful) JSONObject(res.body!!.string()).optString("name").ifEmpty { "PC" } else null
                }
            }.getOrNull()
        }

    /**
     * Asks the PC on the cable to pair. It doesn't answer with the code: it opens the
     * pairing link in this app through adb (only possible from a PC this phone allowed USB
     * debugging for), carrying our nonce, and [UsbPairReceiver] hands it to the pair screen, which pairs without a tap.
     */
    suspend fun requestUsbPairing(): Result<Unit> =
        withContext(Dispatchers.IO) {
            runCatching {
                val nonce = UUID.randomUUID().toString().replace("-", "")
                usbPairNonce = nonce
                val body = JSONObject().put("nonce", nonce).toString().toRequestBody("application/json".toMediaType())
                probeClient
                    .newBuilder()
                    .readTimeout(30, TimeUnit.SECONDS) // the PC runs adb before answering
                    .build()
                    .newCall(Request.Builder().url("https://$USB_HOST:$DEFAULT_PORT/pair/usb").post(body).build())
                    .execute()
                    .use { res -> if (!res.isSuccessful) error(res.body?.string().orEmpty().ifBlank { "PC answered ${res.code}" }) }
            }.onFailure { usbPairNonce = null }
        }

    /** The link the PC sent back for [requestUsbPairing]; the pair screen pairs with it. */
    val usbPairLink = MutableStateFlow<String?>(null)

    /** From [UsbPairReceiver]: kept only when it answers this phone's own request. */
    fun offerUsbPairLink(link: String) {
        val nonce = usbPairNonce ?: return
        if (runCatching { Uri.parse(link.trim()).getQueryParameter("r") == nonce }.getOrDefault(false)) {
            usbPairNonce = null
            usbPairLink.value = link
        }
    }

    /** Forgets the active PC. Another paired PC, if any, becomes active (not connected yet). */
    fun unpair() {
        stop()
        prefs.clearPairing()
        client = null
        state.value = if (prefs.paired) LinkState.Offline else LinkState.Unpaired
    }

    /**
     * Forgets the PC [id]. Forgetting the active one moves to another paired PC, or back to
     * pairing when it was the last. Call from the foreground (it may start [LinkService]).
     */
    fun forget(id: String) {
        if (id != prefs.active?.id) {
            prefs.removePc(id)
        } else if (prefs.pcs.size > 1) {
            unpair()
            LinkService.send(app, LinkService.ACTION_SWITCH)
        } else {
            LinkService.send(app, LinkService.ACTION_DISCONNECT)
            unpair()
        }
    }

    /** Makes [id] the active PC. Call with the link stopped; [start] connects to it. */
    fun switchTo(id: String) {
        prefs.activeId = id
        client = null
        state.value = LinkState.Offline
    }

    // ---------- control connection ----------

    fun start() {
        if (!prefs.paired || controlJob?.isActive == true) return
        choosePc.value = false
        controlJob = scope.launch { controlLoop() }
        watchDebugSettings()
        watchUsb()
        watchBattery()
    }

    private var batteryReceiver: BroadcastReceiver? = null

    /** The battery as last sent: (percent, charging). Null until sent on this connection. */
    @Volatile private var sentBattery: Pair<Int, Boolean>? = null

    /**
     * Tells the PC the battery level and whether it's charging (`battery`), for its Devices
     * page. ACTION_BATTERY_CHANGED comes every few seconds for voltage and temperature alone,
     * so only a new percentage or charging state is sent.
     */
    private fun watchBattery() {
        if (batteryReceiver != null) return
        val receiver =
            object : BroadcastReceiver() {
                override fun onReceive(
                    context: Context,
                    intent: Intent,
                ) = sendBattery(intent)
            }
        ContextCompat.registerReceiver(app, receiver, IntentFilter(Intent.ACTION_BATTERY_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED)
        batteryReceiver = receiver
    }

    /** Sends the battery from [intent] (the sticky one when null) if it changed, or [force]d. */
    private fun sendBattery(
        intent: Intent? = null,
        force: Boolean = false,
    ) {
        val battery =
            intent ?: ContextCompat.registerReceiver(app, null, IntentFilter(Intent.ACTION_BATTERY_CHANGED), ContextCompat.RECEIVER_NOT_EXPORTED)
                ?: return
        val level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, 100)
        if (level < 0 || scale <= 0) return
        val percent = level * 100 / scale
        val status = battery.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
        // Full while plugged in counts as charging: the phone is on power.
        val charging = status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL
        val now = percent to charging
        if (!force && now == sentBattery) return
        val ws = control ?: return
        if (ws.send(JSONObject().put("t", "battery").put("level", percent).put("charging", charging).toString())) sentBattery = now
    }

    private var usbReceiver: BroadcastReceiver? = null

    /**
     * Retries right away when a USB cable is plugged in, instead of after the offline backoff
     * (up to 30 s): the PC sets up `adb reverse` within a few seconds of seeing the phone.
     * Already online over Wi-Fi, the control loop's own probe moves the link to the cable.
     */
    private fun watchUsb() {
        if (usbReceiver != null) return
        val receiver =
            object : BroadcastReceiver() {
                override fun onReceive(
                    context: Context,
                    intent: Intent,
                ) {
                    if (!intent.getBooleanExtra("connected", false)) return
                    scope.launch {
                        repeat(4) {
                            delay(2_000)
                            if (state.value !is LinkState.Online) kicks.trySend(Unit)
                        }
                    }
                }
            }
        // A system broadcast (sticky, hidden from the SDK constants); delivered to unexported receivers too.
        ContextCompat.registerReceiver(app, receiver, IntentFilter(USB_STATE), ContextCompat.RECEIVER_NOT_EXPORTED)
        usbReceiver = receiver
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
                .put("name", deviceName())
        control?.send(msg.toString())
    }

    fun stop() {
        controlJob?.cancel()
        controlJob = null
        control?.close(1000, null)
        control = null
        if (prefs.paired) state.value = LinkState.Offline
    }

    /** Stops the link until the user reconnects (see [LinkState.Disconnected]). */
    fun disconnect() {
        stop()
        if (prefs.paired) state.value = LinkState.Disconnected
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
                // The token this connection uses: a 401 for it says nothing about a newer one.
                val token = prefs.token
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
                                if (response?.code == 401) revoked(token)
                                closed.complete(Unit)
                            }
                        },
                    )
                control = ws
                currentHost = host
                // On Wi-Fi, switch to the cable as soon as it's plugged in.
                val upgrade =
                    if (host == USB_HOST) {
                        null
                    } else {
                        scope.launch {
                            while (isActive) {
                                delay(USB_PROBE_MS)
                                if (ping(USB_HOST)) {
                                    Log.i(TAG, "USB link available, switching")
                                    ws.close(1000, "switching to USB")
                                    closed.complete(Unit)
                                    kicks.trySend(Unit)
                                    break
                                }
                            }
                        }
                    }
                try {
                    closed.await()
                } finally {
                    upgrade?.cancel()
                    ws.cancel()
                    control = null
                    currentHost = null
                }
            }
            if (!prefs.paired) break
            state.value = LinkState.Offline
            withTimeoutOrNull(backoff) { kicks.receive() }
            backoff = (backoff * 2).coerceAtMost(30_000L)
        }
    }

    /** First reachable address: the USB cable, the last good one, the ones from pairing, then mDNS. */
    private suspend fun resolveHost(): String? {
        val candidates = (listOf(USB_HOST) + listOfNotNull(prefs.lastHost) + prefs.hosts).distinct()
        for (h in candidates) if (ping(h)) return h
        val found = Discovery.find(app, prefs.serverId) ?: return null
        if (!ping(found)) return null
        prefs.lastHost = found
        return found
    }

    private fun ping(host: String): Boolean =
        try {
            val quick = http().newBuilder().connectTimeout(1500, TimeUnit.MILLISECONDS).readTimeout(3, TimeUnit.SECONDS).build()
            val token = prefs.token
            quick.newCall(request("/ping", host).build()).execute().use { res ->
                if (res.code == 401) revoked(token)
                // The cable isn't an address to come back to over Wi-Fi.
                if (res.isSuccessful && host != USB_HOST) prefs.lastHost = host
                res.isSuccessful
            }
        } catch (_: Exception) {
            false
        }

    /**
     * The PC forgot this phone (`dotlink unpair`): it refused [token]. Forgets the PC holding
     * that token, not simply the active one. A reply to an old request can arrive after the
     * phone switched PCs or paired again (a new token), and must not remove those.
     */
    private fun revoked(token: String?) {
        val pc = prefs.pcs.find { it.token == token } ?: return
        if (pc.id != prefs.active?.id) {
            prefs.removePc(pc.id)
            notices.tryEmit("${pc.name} forgot this phone")
            return
        }
        prefs.clearPairing()
        client = null
        state.value = if (prefs.paired) LinkState.Offline else LinkState.Unpaired
        notices.tryEmit("${pc.name} forgot this phone")
    }

    private fun onControl(text: String) {
        val msg = runCatching { JSONObject(text) }.getOrNull() ?: return
        when (msg.optString("t")) {
            "hello" -> {
                val name = msg.optString("name", prefs.serverName)
                prefs.serverName = name
                msg.optString("os").takeIf { it.isNotEmpty() }?.let { prefs.pcOs = it }
                state.value = LinkState.Online(name)
                sendInfo()
                sendBattery(force = true)
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
        val t = newTransfer(name, incoming = true, size = size)
        try {
            transferClient().newCall(request("/download/$id").build()).execute().use { res ->
                if (!res.isSuccessful) error("PC answered ${res.code}")
                saveIncoming(t, prefs.serverName, res.body!!.byteStream())
            }
        } catch (e: Exception) {
            Log.w(TAG, "download $name failed", e)
            finish(t.id, e.message ?: e.javaClass.simpleName)
        }
    }

    /** A transfer for the list (and the progress notification). */
    fun newTransfer(
        name: String,
        incoming: Boolean,
        size: Long,
        phone: String? = null,
    ): Transfer {
        val t = Transfer(transferIds.getAndIncrement(), name, incoming = incoming, size = size, phone = phone)
        transfers.update { listOf(t) + it.take(30) }
        return t
    }

    /**
     * Saves a received file into Download/Dotlink from [input] and says so in a notification
     * ("Received from [from]"). Throws when it fails, after removing the partial file and
     * marking [t] failed.
     */
    fun saveIncoming(
        t: Transfer,
        from: String,
        input: java.io.InputStream,
    ) {
        val name = t.name
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
            error("can't create file")
        }
        try {
            resolver.openOutputStream(item)!!.use { out ->
                val buf = ByteArray(256 * 1024)
                var got = 0L
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    got += n
                    progress(t.id, got)
                }
                if (t.size >= 0 && got != t.size) error("cut off after ${got} of ${t.size} bytes")
            }
            resolver.update(item, ContentValues().apply { put(MediaStore.Downloads.IS_PENDING, 0) }, null, null)
            finish(t.id, null)
            notifyReceived(from, name, item, mime)
        } catch (e: Exception) {
            resolver.delete(item, null, null)
            finish(t.id, e.message ?: e.javaClass.simpleName)
            throw e
        }
    }

    private fun transferClient() =
        http()
            .newBuilder()
            .readTimeout(60, TimeUnit.SECONDS)
            .writeTimeout(60, TimeUnit.SECONDS)
            .build()

    private var lastProgress = 0L

    fun progress(
        id: Long,
        done: Long,
    ) {
        val now = System.currentTimeMillis()
        if (now - lastProgress < 150) return
        lastProgress = now
        transfers.update { list -> list.map { if (it.id == id) it.copy(done = done) else it } }
    }

    fun finish(
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
        from: String,
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
                .setContentTitle("Received from $from")
                .setContentText(name)
                .setContentIntent(pi)
                .setAutoCancel(true)
                .build()
        nm.notify(uri.hashCode(), n)
    }

    fun queryName(uri: Uri): Pair<String, Long> {
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

    fun sha256(uri: Uri): String {
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

    /** What the PCs call this phone: the name set in Settings, or the system one. */
    fun deviceName(): String = prefs.phoneName.trim().ifEmpty { systemDeviceName() }

    fun systemDeviceName(): String =
        Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME)
            ?: "${Build.MANUFACTURER} ${Build.MODEL}"

    /** Saves this phone's name and tells the connected PC (`info`). */
    fun renamePhone(name: String) {
        prefs.phoneName = name
        sendInfo()
    }

    companion object {
        private const val TAG = "Dotlink"

        /**
         * This phone's loopback: with USB debugging allowed, the PC runs `adb reverse` for its
         * port, and connections here reach it through the cable. The pinned certificate still
         * decides whether it is the right PC.
         */
        const val USB_HOST = "127.0.0.1"

        /** The PC's default port; `adb reverse` maps the same port on the phone. */
        private const val DEFAULT_PORT = 47390

        /**
         * For [usbPc] / [requestUsbPairing] only: accepts any certificate, because before
         * pairing there is none to pin. Only a nonce goes over it, never a secret.
         */
        private val probeClient: OkHttpClient by lazy {
            val tm =
                object : X509TrustManager {
                    override fun checkClientTrusted(
                        chain: Array<out X509Certificate>,
                        authType: String,
                    ) = throw CertificateException("client certs not used")

                    override fun checkServerTrusted(
                        chain: Array<out X509Certificate>,
                        authType: String,
                    ) = Unit

                    override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
                }
            val ssl = SSLContext.getInstance("TLS").apply { init(null, arrayOf(tm), null) }
            OkHttpClient
                .Builder()
                .sslSocketFactory(ssl.socketFactory, tm)
                .hostnameVerifier { _, _ -> true }
                .connectTimeout(2, TimeUnit.SECONDS)
                .readTimeout(3, TimeUnit.SECONDS)
                .build()
        }

        /** How often a Wi-Fi link checks whether the cable has become available. */
        private const val USB_PROBE_MS = 5_000L

        /** UsbManager.ACTION_USB_STATE: a cable was plugged in or out. */
        private const val USB_STATE = "android.hardware.usb.action.USB_STATE"

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
                .socketFactory(NoDelaySocketFactory)
                .connectTimeout(timeoutSec, TimeUnit.SECONDS)
                .readTimeout(readTimeoutSec, TimeUnit.SECONDS)
                .pingInterval(15, TimeUnit.SECONDS)
                .build()
        }
    }
}

fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

/**
 * Sockets with TCP_NODELAY. Remote moves, Shell keystrokes and control messages are tiny;
 * Nagle's algorithm would hold each one back until the previous was acknowledged.
 */
private object NoDelaySocketFactory : SocketFactory() {
    private val system = getDefault()

    private fun Socket.noDelay() = apply { tcpNoDelay = true }

    override fun createSocket(): Socket = system.createSocket().noDelay()

    override fun createSocket(
        host: String,
        port: Int,
    ): Socket = system.createSocket(host, port).noDelay()

    override fun createSocket(
        host: String,
        port: Int,
        localHost: InetAddress,
        localPort: Int,
    ): Socket = system.createSocket(host, port, localHost, localPort).noDelay()

    override fun createSocket(
        host: InetAddress,
        port: Int,
    ): Socket = system.createSocket(host, port).noDelay()

    override fun createSocket(
        address: InetAddress,
        port: Int,
        localAddress: InetAddress,
        localPort: Int,
    ): Socket = system.createSocket(address, port, localAddress, localPort).noDelay()
}
