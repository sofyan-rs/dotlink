package com.dotlink.link

import android.content.Context
import androidx.core.content.edit
import java.util.UUID

/** Pairing data and user settings, in private SharedPreferences. */
class Prefs(
    context: Context,
) {
    private val sp = context.getSharedPreferences("dotlink", Context.MODE_PRIVATE)

    /** Random id for this phone; the PC uses it to replace the old token when re-pairing. */
    val deviceId: String =
        sp.getString("device_id", null) ?: UUID.randomUUID().toString().also { sp.edit { putString("device_id", it) } }

    val paired: Boolean get() = token != null && fingerprint != null

    var hosts: List<String>
        get() = sp.getString("hosts", "")!!.split(',').filter { it.isNotBlank() }
        set(v) = sp.edit { putString("hosts", v.joinToString(",")) }
    var lastHost: String?
        get() = sp.getString("last_host", null)
        set(v) = sp.edit { putString("last_host", v) }
    var port: Int
        get() = sp.getInt("port", 47390)
        set(v) = sp.edit { putInt("port", v) }
    var fingerprint: String?
        get() = sp.getString("fingerprint", null)
        set(v) = sp.edit { putString("fingerprint", v) }
    var token: String?
        get() = sp.getString("token", null)
        set(v) = sp.edit { putString("token", v) }
    var serverName: String
        get() = sp.getString("server_name", "PC")!!
        set(v) = sp.edit { putString("server_name", v) }
    var serverId: String
        get() = sp.getString("server_id", "")!!
        set(v) = sp.edit { putString("server_id", v) }

    // Settings
    var frontCamera: Boolean
        get() = sp.getBoolean("front_camera", false)
        set(v) = sp.edit { putBoolean("front_camera", v) }
    var fullHd: Boolean
        get() = sp.getBoolean("full_hd", false)
        set(v) = sp.edit { putBoolean("full_hd", v) }
    var fps: Int
        get() = sp.getInt("fps", 30)
        set(v) = sp.edit { putInt("fps", v) }
    var landscape: Boolean
        get() = sp.getBoolean("landscape", true)
        set(v) = sp.edit { putBoolean("landscape", v) }
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

    fun clearPairing() =
        sp.edit {
            remove("hosts")
            remove("last_host")
            remove("fingerprint")
            remove("token")
            remove("server_name")
            remove("server_id")
        }
}
