package com.dotlink.peer

import android.app.Application
import android.content.ClipData
import android.content.ClipboardManager
import android.net.Uri
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.util.Log
import android.widget.Toast
import com.dotlink.link.Discovery
import com.dotlink.link.Link
import com.dotlink.link.PhonePeer
import com.dotlink.link.toHex
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okio.BufferedSink
import org.json.JSONArray
import org.json.JSONObject
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.URLEncoder
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume

/**
 * Phones linked with this one: each phone runs a small HTTPS server ([PhoneServer]) and the
 * others send to it directly, a file or a clipboard text at a time; there is no lasting
 * connection. Pairing: one phone shows a QR code (`dotlink://phone?…`, [invite]), the other
 * scans it ([pair]); both then pin each other's certificate and hold a token for each other.
 */
class Phones(
    private val app: Application,
    private val link: Link,
) {
    private val prefs get() = link.prefs

    /** The paired phones, for the UI. */
    val list = MutableStateFlow(prefs.phones)

    /** The QR code being shown: its one-time secret and expiry (ms). */
    private data class Invite(
        val secret: String,
        val expires: Long,
    )

    @Volatile private var invite: Invite? = null

    /** Paired phones seen on the network right now, by id ([watch] keeps it current). */
    val online = MutableStateFlow<Set<String>>(emptySet())

    /** Set when a phone paired by scanning this one's code: its name (the pair screen closes). */
    val pairedByScan = MutableStateFlow<String?>(null)

    private val server = PhoneServer(link.scope, ::handle)
    private val nsd by lazy { app.getSystemService(NsdManager::class.java) }
    private var registration: NsdManager.RegistrationListener? = null

    /** The server has to run: a phone is paired, or this one's code is being shown. */
    val needed: Boolean get() = prefs.phones.isNotEmpty() || invite.let { it != null && it.expires > System.currentTimeMillis() }

    /** Starts listening and advertising (LinkService, while [needed]). */
    @Synchronized
    fun startServer(): Boolean {
        if (server.port != 0) return true
        if (!runCatching { server.start() }.onFailure { Log.w(TAG, "phone server", it) }.getOrDefault(false)) return false
        advertise()
        return true
    }

    @Synchronized
    fun stopServer() {
        registration?.let { runCatching { nsd.unregisterService(it) } }
        registration = null
        server.stop()
    }

    private fun advertise() {
        val info =
            NsdServiceInfo().apply {
                serviceName = "dotlink-" + prefs.deviceId.take(8)
                serviceType = SERVICE
                port = server.port
                setAttribute("id", prefs.deviceId)
                setAttribute("fp", PhoneTls.fingerprint.take(16))
            }
        val listener =
            object : NsdManager.RegistrationListener {
                override fun onServiceRegistered(info: NsdServiceInfo) = Unit

                override fun onRegistrationFailed(
                    info: NsdServiceInfo,
                    errorCode: Int,
                ) {
                    Log.w(TAG, "NSD registration failed: $errorCode")
                }

                override fun onServiceUnregistered(info: NsdServiceInfo) = Unit

                override fun onUnregistrationFailed(
                    info: NsdServiceInfo,
                    errorCode: Int,
                ) = Unit
            }
        runCatching { nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener) }.onSuccess { registration = listener }
    }

    /**
     * Looks for the paired phones on the network until cancelled (while the Phones list is on
     * screen): [online] follows them, and where they are now is remembered for sending.
     */
    @Suppress("DEPRECATION") // resolveService: the replacement needs API 34.
    suspend fun watch() {
        val wifi = app.getSystemService(WifiManager::class.java)
        val lock = wifi.createMulticastLock("dotlink-phones").apply { setReferenceCounted(false) }
        val found = Channel<NsdServiceInfo>(Channel.UNLIMITED)
        // Service name → phone id, to know which phone left when its service is lost.
        val names = ConcurrentHashMap<String, String>()
        val listener =
            object : NsdManager.DiscoveryListener {
                override fun onServiceFound(info: NsdServiceInfo) {
                    found.trySend(info)
                }

                override fun onServiceLost(info: NsdServiceInfo) {
                    names.remove(info.serviceName)?.let { id -> online.update { it - id } }
                }

                override fun onStartDiscoveryFailed(
                    serviceType: String,
                    errorCode: Int,
                ) {
                    found.close()
                }

                override fun onDiscoveryStarted(serviceType: String) = Unit

                override fun onDiscoveryStopped(serviceType: String) = Unit

                override fun onStopDiscoveryFailed(
                    serviceType: String,
                    errorCode: Int,
                ) = Unit
            }
        lock.acquire()
        try {
            nsd.discoverServices(SERVICE, NsdManager.PROTOCOL_DNS_SD, listener)
            // One resolve at a time: older Android refuses a second while one runs.
            for (info in found) {
                val resolved = withTimeoutOrNull(4000) { resolve(info) } ?: continue
                val id = resolved.attributes["id"]?.decodeToString() ?: continue
                val host = resolved.host?.hostAddress ?: continue
                val peer = prefs.phones.find { it.id == id } ?: continue
                names[info.serviceName] = id
                online.update { it + id }
                if (peer.hosts.firstOrNull() != host || peer.port != resolved.port) {
                    save(peer.copy(hosts = listOf(host) + peer.hosts.filter { it != host }, port = resolved.port))
                }
            }
        } finally {
            runCatching { nsd.stopServiceDiscovery(listener) }
            lock.release()
            online.value = emptySet()
        }
    }

    @Suppress("DEPRECATION")
    private suspend fun resolve(info: NsdServiceInfo): NsdServiceInfo? =
        suspendCancellableCoroutine { cont ->
            nsd.resolveService(
                info,
                object : NsdManager.ResolveListener {
                    override fun onResolveFailed(
                        info: NsdServiceInfo,
                        errorCode: Int,
                    ) {
                        if (cont.isActive) cont.resume(null)
                    }

                    override fun onServiceResolved(info: NsdServiceInfo) {
                        if (cont.isActive) cont.resume(info)
                    }
                },
            )
        }

    // ---------- pairing ----------

    /** A new `dotlink://phone?…` link for the QR code, valid for 5 minutes. */
    fun invite(): String {
        val secret = randomHex(16)
        invite = Invite(secret, System.currentTimeMillis() + INVITE_MS)
        startServer()
        val q =
            listOf(
                "h" to localIps().joinToString(","),
                "p" to server.port.toString(),
                "fp" to PhoneTls.fingerprint,
                "c" to secret,
                "id" to prefs.deviceId,
                "n" to link.deviceName(),
            ).joinToString("&") { (k, v) -> "$k=" + URLEncoder.encode(v, "UTF-8") }
        return "dotlink://phone?$q"
    }

    fun cancelInvite() {
        invite = null
    }

    /** Pairs with the phone whose code was scanned. Returns its name. */
    suspend fun pair(uri: String): Result<String> =
        withContext(Dispatchers.IO) {
            runCatching {
                val u = Uri.parse(uri.trim())
                require(u.scheme == "dotlink" && u.host == "phone") { "Not a Dotlink phone code" }
                val hosts = u.getQueryParameter("h").orEmpty().split(',').filter { it.isNotBlank() }
                val port = u.getQueryParameter("p")?.toIntOrNull() ?: error("The code has no port")
                val fingerprint = u.getQueryParameter("fp")?.lowercase() ?: error("The code has no fingerprint")
                val secret = u.getQueryParameter("c") ?: error("The code has no secret")
                val id = u.getQueryParameter("id").orEmpty()
                require(id != prefs.deviceId) { "That's this phone's own code: scan it with the other phone" }
                // What the other phone will present to this one.
                val tokenIn = randomHex(32)
                val body =
                    JSONObject()
                        .put("secret", secret)
                        .put("id", prefs.deviceId)
                        .put("name", link.deviceName())
                        .put("fingerprint", PhoneTls.fingerprint)
                        .put("port", server.port.takeIf { it != 0 } ?: PhoneServer.PORT)
                        .put("hosts", JSONArray(localIps()))
                        .put("token", tokenIn)
                startServer()
                val client = Link.pinnedClient(fingerprint, timeoutSec = 4, readTimeoutSec = 10)
                var last: Exception = IllegalStateException("The phone has no address")
                for (host in hosts) {
                    try {
                        val req =
                            Request
                                .Builder()
                                .url(url(host, port, "/pair"))
                                .post(body.toString().toRequestBody(JSON))
                                .build()
                        client.newCall(req).execute().use { res ->
                            val text = res.body?.string().orEmpty()
                            if (!res.isSuccessful) error(text.ifBlank { "The phone answered ${res.code}" })
                            val v = JSONObject(text)
                            val peer =
                                PhonePeer(
                                    id = v.getString("id"),
                                    name = v.optString("name", u.getQueryParameter("n") ?: "Phone"),
                                    fingerprint = fingerprint,
                                    hosts = listOf(host) + hosts.filter { it != host },
                                    port = port,
                                    tokenOut = v.getString("token"),
                                    tokenInSha256 = sha256(tokenIn),
                                )
                            save(peer)
                            return@runCatching peer.name
                        }
                    } catch (e: Exception) {
                        // A refusal (wrong or used code) won't change at another address.
                        if (e is IllegalStateException) throw e
                        last = e
                    }
                }
                throw last
            }
        }

    private fun save(peer: PhonePeer) {
        prefs.phones = prefs.phones.filter { it.id != peer.id } + peer
        list.value = prefs.phones
    }

    /** Forgets a phone here and tells it to forget this one (when it can be reached). */
    suspend fun forget(id: String) {
        val peer = prefs.phones.find { it.id == id } ?: return
        prefs.phones = prefs.phones.filter { it.id != id }
        list.value = prefs.phones
        withContext(Dispatchers.IO) {
            runCatching { call(peer, "/forget", "{}".toRequestBody(JSON), timeoutSec = 3) }
        }
    }

    // ---------- sending ----------

    /** Sends files to a paired phone; returns how many arrived. */
    suspend fun sendFiles(
        id: String,
        uris: List<Uri>,
    ): Int =
        withContext(Dispatchers.IO) {
            val peer = prefs.phones.find { it.id == id } ?: return@withContext 0
            var ok = 0
            for (uri in uris) {
                val (name, size) = link.queryName(uri)
                val t = link.newTransfer(name, incoming = false, size = size, phone = peer.name)
                try {
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
                                        link.progress(t.id, sent)
                                    }
                                }
                            }
                        }
                    // Long enough for the other phone to answer "Ask before receiving files".
                    call(peer, "/upload?name=" + URLEncoder.encode(name, "UTF-8"), body, timeoutSec = 180)
                    link.finish(t.id, null)
                    ok++
                } catch (e: Exception) {
                    Log.w(TAG, "send $name to ${peer.name} failed", e)
                    link.finish(t.id, e.message ?: e.javaClass.simpleName)
                }
            }
            ok
        }

    /** Sends a text for the other phone's clipboard. */
    suspend fun sendText(
        id: String,
        text: String,
    ): Result<Unit> =
        withContext(Dispatchers.IO) {
            runCatching {
                val peer = prefs.phones.find { it.id == id } ?: error("Not paired")
                call(peer, "/clip", JSONObject().put("text", text).toString().toRequestBody(JSON), timeoutSec = 4)
            }
        }

    /** One authenticated request to [peer]: its known addresses first, then a mDNS lookup. */
    private fun call(
        peer: PhonePeer,
        path: String,
        body: RequestBody,
        timeoutSec: Long,
    ) {
        val client =
            Link
                .pinnedClient(peer.fingerprint, timeoutSec = 3, readTimeoutSec = timeoutSec)
                .newBuilder()
                .writeTimeout(timeoutSec, TimeUnit.SECONDS)
                .build()

        fun attempt(
            host: String,
            port: Int,
        ) {
            val req =
                Request
                    .Builder()
                    .url(url(host, port, path))
                    .header("Authorization", "Bearer ${peer.tokenOut}")
                    .post(body)
                    .build()
            client.newCall(req).execute().use { res ->
                if (res.code == 401) {
                    // It forgot this phone.
                    prefs.phones = prefs.phones.filter { it.id != peer.id }
                    list.value = prefs.phones
                    error("${peer.name} doesn't know this phone any more. Pair again.")
                }
                if (!res.isSuccessful) error(res.body?.string()?.ifBlank { null } ?: "${peer.name} answered ${res.code}")
            }
        }
        var last: Exception = IllegalStateException("${peer.name} isn't reachable. Open Dotlink on it, on the same Wi-Fi.")
        for (host in peer.hosts) {
            try {
                attempt(host, peer.port)
                return
            } catch (e: IllegalStateException) {
                throw e
            } catch (e: Exception) {
                last = e
            }
        }
        // Its address changed: find it again over mDNS and remember where.
        val found = kotlinx.coroutines.runBlocking { Discovery.findService(app, SERVICE, peer.id) }
        if (found != null) {
            val (host, port) = found
            attempt(host, port)
            save(peer.copy(hosts = listOf(host) + peer.hosts.filter { it != host }, port = port))
            return
        }
        throw IllegalStateException("${peer.name} isn't reachable. Open Dotlink on it, on the same Wi-Fi.", last)
    }

    // ---------- the server ----------

    private suspend fun handle(req: PhoneRequest): PhoneResponse {
        if (req.method == "POST" && req.path == "/pair") return pairRequest(req)
        val peer =
            req.bearer?.let { token -> sha256(token).let { h -> prefs.phones.find { it.tokenInSha256 == h } } }
                ?: return PhoneResponse.error(401, "not paired")
        return when {
            req.method == "POST" && req.path == "/upload" -> receiveFile(peer, req)
            req.method == "POST" && req.path == "/clip" -> receiveClip(peer, req)
            req.method == "POST" && req.path == "/forget" -> {
                prefs.phones = prefs.phones.filter { it.id != peer.id }
                list.value = prefs.phones
                PhoneResponse.json(JSONObject().put("ok", true))
            }
            else -> PhoneResponse.error(404, "not found")
        }
    }

    private fun pairRequest(req: PhoneRequest): PhoneResponse {
        val v = runCatching { req.json() }.getOrNull() ?: return PhoneResponse.error(400, "bad request")
        val current = invite
        val secret = v.optString("secret")
        if (current == null || current.expires < System.currentTimeMillis() || !MessageDigest.isEqual(secret.toByteArray(), current.secret.toByteArray())) {
            return PhoneResponse.error(403, "The code expired or was already used. Show a new one.")
        }
        val id = v.optString("id")
        if (id.isEmpty() || id == prefs.deviceId) return PhoneResponse.error(400, "bad request")
        invite = null
        // What this phone will present to the other one.
        val tokenOther = randomHex(32)
        val peer =
            PhonePeer(
                id = id,
                name = v.optString("name", "Phone").take(64),
                fingerprint = v.getString("fingerprint").lowercase(),
                hosts = v.optJSONArray("hosts")?.let { a -> List(a.length()) { a.getString(it) } }.orEmpty(),
                port = v.optInt("port", PhoneServer.PORT),
                tokenOut = v.getString("token"),
                tokenInSha256 = sha256(tokenOther),
            )
        save(peer)
        pairedByScan.value = peer.name
        return PhoneResponse.json(
            JSONObject()
                .put("id", prefs.deviceId)
                .put("name", link.deviceName())
                .put("token", tokenOther),
        )
    }

    private suspend fun receiveFile(
        peer: PhonePeer,
        req: PhoneRequest,
    ): PhoneResponse {
        val name =
            req.query["name"]
                ?.substringAfterLast('/')
                ?.substringAfterLast('\\')
                ?.trim()
                ?.takeIf { it.isNotEmpty() && it != "." && it != ".." }
                ?: return PhoneResponse.error(400, "bad file name")
        // Nothing is read until it's accepted: the sender waits.
        if (!link.askIncoming(peer.name, name, req.contentLength)) return PhoneResponse.error(403, "${link.deviceName()} declined it")
        val t = link.newTransfer(name, incoming = true, size = req.contentLength, phone = peer.name)
        return runCatching { link.saveIncoming(t, peer.name, req.body) }.fold(
            onSuccess = { PhoneResponse.json(JSONObject().put("ok", true)) },
            onFailure = { PhoneResponse.error(500, it.message ?: "couldn't save it") },
        )
    }

    private suspend fun receiveClip(
        peer: PhonePeer,
        req: PhoneRequest,
    ): PhoneResponse {
        val text = runCatching { req.json().getString("text") }.getOrNull() ?: return PhoneResponse.error(400, "bad request")
        withContext(Dispatchers.Main) {
            app.getSystemService(ClipboardManager::class.java).setPrimaryClip(ClipData.newPlainText("Dotlink", text))
            Toast.makeText(app, "Clipboard from ${peer.name}", Toast.LENGTH_SHORT).show()
        }
        return PhoneResponse.json(JSONObject().put("ok", true))
    }

    companion object {
        private const val TAG = "Dotlink"
        const val SERVICE = "_dotlink-phone._tcp."
        private const val INVITE_MS = 5 * 60_000L
        private val JSON = "application/json".toMediaType()

        private fun url(
            host: String,
            port: Int,
            path: String,
        ) = "https://${if (host.contains(':')) "[$host]" else host}:$port$path"

        fun sha256(s: String): String = MessageDigest.getInstance("SHA-256").digest(s.toByteArray()).toHex()

        private fun randomHex(bytes: Int) = ByteArray(bytes).also { SecureRandom().nextBytes(it) }.toHex()

        /** This phone's Wi-Fi (and hotspot) IPv4 addresses; not mobile data. */
        fun localIps(): List<String> =
            runCatching {
                NetworkInterface
                    .getNetworkInterfaces()
                    .toList()
                    .filter { it.isUp && !it.isLoopback && !it.name.startsWith("rmnet") && !it.name.startsWith("dummy") }
                    .flatMap { i -> i.inetAddresses.toList().filterIsInstance<Inet4Address>().map { it.hostAddress!! } }
            }.getOrDefault(emptyList())
    }
}
