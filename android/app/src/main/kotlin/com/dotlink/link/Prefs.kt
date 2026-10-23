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

    var activeId: String?
        get() = sp.getString("active_pc", null)
        set(v) = sp.edit { putString("active_pc", v) }

    val active: Pc? get() = pcs.let { all -> all.find { it.id == activeId } ?: all.firstOrNull() }

    val paired: Boolean get() = active != null

    /** Adds [pc] (replacing an older pairing with the same PC) and makes it the active one. */
    fun addPc(pc: Pc) {
        pcs = pcs.filter { it.id != pc.id } + pc
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
    var fps: Int
        get() = sp.getInt("fps", 30)
        set(v) = sp.edit { putInt("fps", v) }
    /** Webcam picture rotation in degrees (0, 90, 180, 270). 90 = landscape, the old default. */
    var rotation: Int
        get() = sp.getInt("rotation", if (sp.getBoolean("landscape", true)) 90 else 0)
        set(v) = sp.edit { putInt("rotation", v) }

    /** Flip the webcam picture left-right. */
    var mirror: Boolean
        get() = sp.getBoolean("mirror", false)
        set(v) = sp.edit { putBoolean("mirror", v) }
    var autoClipboard: Boolean
        get() = sp.getBoolean("auto_clipboard", true)
        set(v) = sp.edit { putBoolean("auto_clipboard", v) }
    var terminalPin: String
        get() = sp.getString("terminal_pin", "")!!
        set(v) = sp.edit { putString("terminal_pin", v) }
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
}
