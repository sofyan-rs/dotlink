package com.dotlink.ui

import android.app.Activity
import android.widget.Toast
import com.dotlink.DotlinkApp

/**
 * Invisible activity behind the notification's "Send clipboard" button. Android only lets the
 * focused app read the clipboard, so we read it once our (empty) window gets focus.
 */
class ClipboardActivity : Activity() {
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus) return
        val link = DotlinkApp.link
        val message =
            when {
                link.sendPhoneClipboard() -> "Clipboard sent to ${link.prefs.serverName}"
                else -> "Not connected, or the clipboard is empty"
            }
        Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
        finish()
    }
}
