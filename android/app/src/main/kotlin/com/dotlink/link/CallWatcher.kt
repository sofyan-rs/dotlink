package com.dotlink.link

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.telephony.PhoneStateListener
import android.telephony.TelephonyCallback
import android.telephony.TelephonyManager
import androidx.core.content.ContextCompat
import com.dotlink.DotlinkApp
import org.json.JSONObject

/**
 * "Pause PC media during calls": tells the PC when a call rings, is answered and ends
 * (`call`: ringing, offhook, idle). The PC pauses its player if it plays and resumes it after,
 * only when it was the one that paused it. Needs READ_PHONE_STATE, asked when the setting is
 * turned on.
 */
object CallWatcher {
    private var registered: Any? = null

    fun allowed(context: Context) = ContextCompat.checkSelfPermission(context, Manifest.permission.READ_PHONE_STATE) == PackageManager.PERMISSION_GRANTED

    private fun send(state: Int) {
        val name =
            when (state) {
                TelephonyManager.CALL_STATE_RINGING -> "ringing"
                TelephonyManager.CALL_STATE_OFFHOOK -> "offhook"
                else -> "idle"
            }
        DotlinkApp.link.sendControl(JSONObject().put("t", "call").put("state", name))
    }

    /** Follows the setting: listens while it's on and allowed. */
    @Suppress("DEPRECATION") // PhoneStateListener before Android 12.
    fun update(context: Context) {
        val tm = context.getSystemService(TelephonyManager::class.java) ?: return
        val want = DotlinkApp.link.prefs.pauseOnCalls && allowed(context)
        if (want == (registered != null)) return
        if (!want) {
            when (val r = registered) {
                is TelephonyCallback -> tm.unregisterTelephonyCallback(r)
                is PhoneStateListener -> tm.listen(r, PhoneStateListener.LISTEN_NONE)
            }
            registered = null
            return
        }
        registered =
            if (Build.VERSION.SDK_INT >= 31) {
                object : TelephonyCallback(), TelephonyCallback.CallStateListener {
                    override fun onCallStateChanged(state: Int) = send(state)
                }.also { tm.registerTelephonyCallback(context.mainExecutor, it) }
            } else {
                object : PhoneStateListener() {
                    @Deprecated("Deprecated in Java")
                    override fun onCallStateChanged(
                        state: Int,
                        phoneNumber: String?,
                    ) = send(state)
                }.also { tm.listen(it, PhoneStateListener.LISTEN_CALL_STATE) }
            }
    }
}
