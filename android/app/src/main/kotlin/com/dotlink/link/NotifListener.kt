package com.dotlink.link

import android.app.AppOpsManager
import android.app.Notification
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Build
import android.os.Process
import android.service.notification.NotificationListenerService
import android.service.notification.StatusBarNotification
import android.util.Base64
import androidx.core.app.NotificationManagerCompat
import com.dotlink.DotlinkApp
import org.json.JSONObject
import java.io.ByteArrayOutputStream

/**
 * "Show notifications on PC": reads this phone's notifications (Notification access, granted in
 * the system's settings) and hands them to [NotificationForwarder].
 */
class NotifListener : NotificationListenerService() {
    override fun onListenerConnected() {
        instance = this
    }

    override fun onListenerDisconnected() {
        if (instance === this) instance = null
    }

    override fun onNotificationPosted(sbn: StatusBarNotification) = NotificationForwarder.posted(this, sbn)

    override fun onNotificationRemoved(sbn: StatusBarNotification) = NotificationForwarder.removed(sbn.key)

    companion object {
        @Volatile private var instance: NotifListener? = null

        /** Notification access is granted to Dotlink. */
        fun enabled(context: Context) = context.packageName in NotificationManagerCompat.getEnabledListenerPackages(context)

        /** Dismiss on the PC: dismisses it here too. */
        fun dismiss(key: String) {
            runCatching { instance?.cancelNotification(key) }
        }

        /**
         * Android 15+ hides the text of notifications holding codes or private messages from
         * listeners, unless the RECEIVE_SENSITIVE_NOTIFICATIONS app op is allowed (only over adb:
         * the PC's Allow). True before Android 15, where nothing is hidden.
         */
        fun seesSensitive(context: Context): Boolean {
            if (Build.VERSION.SDK_INT < 35) return true
            return runCatching {
                val ops = context.getSystemService(AppOpsManager::class.java)
                ops.unsafeCheckOpNoThrow("android:receive_sensitive_notifications", Process.myUid(), context.packageName) ==
                    AppOpsManager.MODE_ALLOWED
            }.getOrDefault(false)
        }
    }
}

/**
 * Sends the phone's notifications to the PC (`notif`, `notif_removed`) while linked and
 * "Show notifications on PC" is on. Skips ongoing ones (music, downloads, this app's own),
 * group summaries, and apps turned off ([Prefs.forwardsApp]: chat apps start off, since they
 * repeat every message). Nothing is kept but what was sent, to send its removal too.
 */
object NotificationForwarder {
    /** Key → title and text as last sent: an update that changes neither isn't sent again. */
    private val sent = LinkedHashMap<String, String>()

    /** Packages whose icon the PC got on this connection. */
    private val iconsSent = HashSet<String>()

    /** A new connection: the PC gets the icons again. */
    @Synchronized
    fun reset() {
        sent.clear()
        iconsSent.clear()
    }

    @Synchronized
    fun posted(
        context: Context,
        sbn: StatusBarNotification,
    ) {
        val link = DotlinkApp.link
        val prefs = link.prefs
        val n = sbn.notification
        if (!prefs.notificationsToPc || sbn.packageName == context.packageName) return
        if (sbn.isOngoing || n.flags and Notification.FLAG_GROUP_SUMMARY != 0 || n.flags and Notification.FLAG_FOREGROUND_SERVICE != 0) return
        val pkg = sbn.packageName
        val label = appLabel(context, pkg)
        prefs.rememberApp(pkg, label)
        if (!prefs.forwardsApp(pkg)) return
        val extras = n.extras
        val title = extras.getCharSequence(Notification.EXTRA_TITLE)?.toString().orEmpty().take(200)
        val text =
            (extras.getCharSequence(Notification.EXTRA_BIG_TEXT) ?: extras.getCharSequence(Notification.EXTRA_TEXT))
                ?.toString()
                .orEmpty()
                .take(2000)
        if (title.isBlank() && text.isBlank()) return
        val content = "$title\n$text"
        if (sent[sbn.key] == content) return
        val msg =
            JSONObject()
                .put("t", "notif")
                .put("key", sbn.key)
                .put("app", label)
                .put("pkg", pkg)
                .put("title", title)
                .put("text", text)
                .put("time", sbn.postTime)
        val withIcon = pkg !in iconsSent
        if (withIcon) appIcon(context, pkg)?.let { msg.put("icon", it) }
        if (!link.sendControl(msg)) return
        if (withIcon) iconsSent += pkg
        sent[sbn.key] = content
        // The PC keeps up to 500; older ones aren't taken away from it any more.
        while (sent.size > 500) sent.remove(sent.keys.first())
    }

    @Synchronized
    fun removed(key: String) {
        if (sent.remove(key) == null) return
        DotlinkApp.link.sendControl(JSONObject().put("t", "notif_removed").put("key", key))
    }

    private fun appLabel(
        context: Context,
        pkg: String,
    ): String =
        runCatching {
            val pm = context.packageManager
            pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
        }.getOrDefault(pkg)

    /** The app's icon as a 96 px PNG, base64. */
    private fun appIcon(
        context: Context,
        pkg: String,
    ): String? =
        runCatching {
            val drawable = context.packageManager.getApplicationIcon(pkg)
            val size = 96
            val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
            drawable.setBounds(0, 0, size, size)
            drawable.draw(Canvas(bitmap))
            val out = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, out)
            Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
        }.getOrNull()
}
