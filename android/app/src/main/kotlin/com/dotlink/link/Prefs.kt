package com.dotlink.link

import android.content.Context
import androidx.core.content.edit
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/** A paired PC. [id] is its server id, or its fingerprint for PCs that don't send one. */
data class Pc(
    val id: String,
    val name: String,
    val serverId: String,
    val hosts: List<String>,
    val lastHost: String?,
    val port: Int,
    val fingerprint: String,
    val token: String,
    /** The PC's OS from its hello ("linux", "windows", "macos"); empty until it says. */
    val os: String = "",
    /** The Shell PIN this PC asks for (`terminal_pin` in its config); empty when it asks none. */
    val terminalPin: String = "",
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("id", id)
            .put("name", name)
            .put("server_id", serverId)
            .put("hosts", JSONArray(hosts))
            .put("last_host", lastHost)
            .put("port", port)
            .put("fingerprint", fingerprint)
            .put("token", token)
            .put("os", os)
            .put("terminal_pin", terminalPin)

    companion object {
        fun fromJson(o: JSONObject) =
            Pc(
                id = o.getString("id"),
                name = o.optString("name", "PC"),
                serverId = o.optString("server_id"),
                hosts = o.optJSONArray("hosts")?.let { a -> List(a.length()) { a.getString(it) } }.orEmpty(),
                lastHost = o.optString("last_host").ifEmpty { null },
                port = o.optInt("port", 47390),
                fingerprint = o.getString("fingerprint"),
                token = o.getString("token"),
                os = o.optString("os"),
                terminalPin = o.optString("terminal_pin"),
            )
    }
}

/**
 * Another phone paired with this one (files and the clipboard, sent directly; see
 * `com.dotlink.peer.Phones`). Each side keeps the token it presents ([tokenOut]) and the
 * SHA-256 of the one the other presents ([tokenInSha256]).
 */
data class PhonePeer(
    val id: String,
    val name: String,
    val fingerprint: String,
    val hosts: List<String>,
    val port: Int,
    val tokenOut: String,
    val tokenInSha256: String,
) {
    fun toJson(): JSONObject =
        JSONObject()
            .put("id", id)
            .put("name", name)
            .put("fingerprint", fingerprint)
            .put("hosts", JSONArray(hosts))
            .put("port", port)
            .put("token_out", tokenOut)
            .put("token_in_sha256", tokenInSha256)

    companion object {
        fun fromJson(o: JSONObject) =
            PhonePeer(
                id = o.getString("id"),
                name = o.optString("name", "Phone"),
                fingerprint = o.getString("fingerprint"),
                hosts = o.optJSONArray("hosts")?.let { a -> List(a.length()) { a.getString(it) } }.orEmpty(),
                port = o.optInt("port", 47391),
                tokenOut = o.getString("token_out"),
                tokenInSha256 = o.getString("token_in_sha256"),
            )
    }
}

/**
 * Pairing data and user settings, in private SharedPreferences. Several PCs can be paired;
 * one is active at a time, and the pairing properties ([token], [hosts]…) are the active one's.
 */
class Prefs(
    context: Context,
) {
    private val sp = context.getSharedPreferences("dotlink", Context.MODE_PRIVATE)

    /**
     * [pcs], parsed. Every pairing property ([token], [lastHost], [serverName]…) reads it, on
     * each request and in screens, so the JSON is parsed only when it changes.
     */
    @Volatile private var pcsCache: List<Pc>? = null

    init {
        migrateSinglePc()
        migrateTerminalPin()
    }

    /** Random id for this phone; the PC uses it to replace the old token when re-pairing. */
    val deviceId: String =
        sp.getString("device_id", null) ?: UUID.randomUUID().toString().also { sp.edit { putString("device_id", it) } }

    /** Every paired PC, in pairing order. */
    var pcs: List<Pc>
        get() =
            pcsCache ?: runCatching {
                val a = JSONArray(sp.getString("pcs", "[]"))
                List(a.length()) { Pc.fromJson(a.getJSONObject(it)) }
            }.getOrDefault(emptyList()).also { pcsCache = it }
        private set(v) {
            pcsCache = v
            sp.edit { putString("pcs", JSONArray(v.map { it.toJson() }).toString()) }
        }

    /** Other phones paired with this one. */
    var phones: List<PhonePeer>
        get() =
            runCatching {
                val a = JSONArray(sp.getString("phones", "[]"))
                List(a.length()) { PhonePeer.fromJson(a.getJSONObject(it)) }
            }.getOrDefault(emptyList())
        set(v) = sp.edit { putString("phones", JSONArray(v.map { it.toJson() }).toString()) }

    var activeId: String?
        get() = sp.getString("active_pc", null)
        set(v) = sp.edit { putString("active_pc", v) }

    val active: Pc? get() = pcs.let { all -> all.find { it.id == activeId } ?: all.firstOrNull() }

    val paired: Boolean get() = active != null

    /** Adds [pc] (replacing an older pairing with the same PC) and makes it the active one. */
    fun addPc(pc: Pc) {
        // Re-pairing the same PC keeps its Shell PIN: the PIN lives on the PC, not in the pairing.
        val pin = pc.terminalPin.ifEmpty { pcs.find { it.id == pc.id }?.terminalPin.orEmpty() }
        pcs = pcs.filter { it.id != pc.id } + pc.copy(terminalPin = pin)
        activeId = pc.id
    }

    private fun updateActive(change: (Pc) -> Pc) {
        val current = active ?: return
        val changed = change(current)
        if (changed != current) pcs = pcs.map { if (it.id == current.id) changed else it }
    }

    val hosts: List<String> get() = active?.hosts.orEmpty()
    var lastHost: String?
        get() = active?.lastHost
        set(v) = updateActive { it.copy(lastHost = v) }
    val port: Int get() = active?.port ?: 47390
    val fingerprint: String? get() = active?.fingerprint
    val token: String? get() = active?.token
    var serverName: String
        get() = active?.name ?: "PC"
        set(v) = updateActive { it.copy(name = v) }
    val serverId: String get() = active?.serverId.orEmpty()
    var pcOs: String
        get() = active?.os.orEmpty()
        set(v) = updateActive { it.copy(os = v) }

    /** The active PC's Shell PIN. Each PC sets its own, so it is kept per PC. */
    val terminalPin: String get() = active?.terminalPin.orEmpty()

    fun setTerminalPin(pcId: String, pin: String) {
        pcs = pcs.map { if (it.id == pcId) it.copy(terminalPin = pin) else it }
    }

    // Settings
    /** What the PCs call this phone; empty means the system device name. */
    var phoneName: String
        get() = sp.getString("phone_name", "")!!
        set(v) = sp.edit { putString("phone_name", v) }
    var frontCamera: Boolean
        get() = sp.getBoolean("front_camera", false)
        set(v) = sp.edit { putBoolean("front_camera", v) }
    var fullHd: Boolean
        get() = sp.getBoolean("full_hd", false)
        set(v) = sp.edit { putBoolean("full_hd", v) }
    /** Webcam frame rate: 30 or 60. Older versions offered 15, which now reads as 30. */
    var fps: Int
        get() = if (sp.getInt("fps", 30) >= 60) 60 else 30
        set(v) = sp.edit { putInt("fps", v) }
    /** Webcam picture rotation in degrees (0, 90, 180, 270). 90 = landscape, the old default. */
    var rotation: Int
        get() = sp.getInt("rotation", if (sp.getBoolean("landscape", true)) 90 else 0)
        set(v) = sp.edit { putInt("rotation", v) }

    /** Flip the webcam picture left-right. */
    var mirror: Boolean
        get() = sp.getBoolean("mirror", false)
        set(v) = sp.edit { putBoolean("mirror", v) }
    /** Ask (a notification with Accept / Decline) before taking a file; off: take it. */
    var askBeforeReceiving: Boolean
        get() = sp.getBoolean("ask_before_receiving", false)
        set(v) = sp.edit { putBoolean("ask_before_receiving", v) }
    var autoClipboard: Boolean
        get() = sp.getBoolean("auto_clipboard", true)
        set(v) = sp.edit { putBoolean("auto_clipboard", v) }
    var terminalFontSize: Int
        get() = sp.getInt("terminal_font", 13)
        set(v) = sp.edit { putInt("terminal_font", v) }
    /** 0 = follow the system, 1 = light, 2 = dark. */
    var themeMode: Int
        get() = sp.getInt("theme_mode", 0)
        set(v) = sp.edit { putInt("theme_mode", v) }
    var pointerSpeed: Float
        get() = sp.getFloat("pointer_speed", 1.4f)
        set(v) = sp.edit { putFloat("pointer_speed", v) }

    /** "Show notifications on PC" (it also needs Notification access, see NotifListener). */
    var notificationsToPc: Boolean
        get() = sp.getBoolean("notifications_to_pc", false)
        set(v) = sp.edit { putBoolean("notifications_to_pc", v) }

    /** Apps turned on or off for the PC by hand: package → on. */
    private var appChoices: Map<String, Boolean>
        get() = runCatching { JSONObject(sp.getString("notification_apps", "{}")!!) }.getOrDefault(JSONObject()).let { o ->
            o.keys().asSequence().associateWith { o.getBoolean(it) }
        }
        set(v) = sp.edit { putString("notification_apps", JSONObject(v).toString()) }

    /** Whether [pkg]'s notifications go to the PC: chat apps start off, the rest on. */
    fun forwardsApp(pkg: String): Boolean = appChoices[pkg] ?: (pkg !in CHAT_APPS)

    fun setForwardsApp(
        pkg: String,
        on: Boolean,
    ) {
        appChoices = appChoices + (pkg to on)
    }

    /** Apps that posted a notification since it was turned on: package → name, for the list. */
    val seenApps: Map<String, String>
        get() = runCatching { JSONObject(sp.getString("notification_seen", "{}")!!) }.getOrDefault(JSONObject()).let { o ->
            o.keys().asSequence().associateWith { o.getString(it) }
        }

    fun rememberApp(
        pkg: String,
        label: String,
    ) {
        val seen = seenApps
        if (seen[pkg] == label || seen.size >= 300) return
        sp.edit { putString("notification_seen", JSONObject(seen + (pkg to label)).toString()) }
    }

    /** Forgets a PC that isn't the active one. */
    fun removePc(id: String) {
        pcs = pcs.filter { it.id != id }
    }

    /** Forgets the active PC; the first of the others (if any) becomes active. */
    fun clearPairing() {
        val id = active?.id ?: return
        pcs = pcs.filter { it.id != id }
        activeId = pcs.firstOrNull()?.id
    }

    /** Older versions kept one Shell PIN for every PC: give it to each PC paired then. */
    private fun migrateTerminalPin() {
        val pin = sp.getString("terminal_pin", null) ?: return
        if (pin.isNotEmpty()) pcs = pcs.map { if (it.terminalPin.isEmpty()) it.copy(terminalPin = pin) else it }
        sp.edit { remove("terminal_pin") }
    }

    /** Older versions kept one PC in separate keys. */
    private fun migrateSinglePc() {
        if (sp.contains("pcs")) return
        val token = sp.getString("token", null)
        val fingerprint = sp.getString("fingerprint", null)
        if (token != null && fingerprint != null) {
            val serverId = sp.getString("server_id", "")!!
            addPc(
                Pc(
                    id = serverId.ifEmpty { fingerprint },
                    name = sp.getString("server_name", "PC")!!,
                    serverId = serverId,
                    hosts = sp.getString("hosts", "")!!.split(',').filter { it.isNotBlank() },
                    lastHost = sp.getString("last_host", null),
                    port = sp.getInt("port", 47390),
                    fingerprint = fingerprint,
                    token = token,
                ),
            )
        } else {
            pcs = emptyList()
        }
        sp.edit { listOf("hosts", "last_host", "port", "fingerprint", "token", "server_name", "server_id").forEach(::remove) }
    }

    companion object {
        /** Chat apps: every message is a notification, so they start off. */
        val CHAT_APPS =
            setOf(
                "com.whatsapp",
                "com.whatsapp.w4b",
                "org.telegram.messenger",
                "org.telegram.messenger.web",
                "org.thunderdog.challegram",
                "org.thoughtcrime.securesms",
                "com.facebook.orca",
                "com.instagram.android",
                "jp.naver.line.android",
                "com.discord",
                "com.Slack",
                "com.tencent.mm",
                "com.viber.voip",
                "com.google.android.apps.dynamite",
                "com.microsoft.teams",
                "com.snapchat.android",
            )
    }
}
