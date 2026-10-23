package com.dotlink.link

import android.annotation.SuppressLint
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat
import com.dotlink.DotlinkApp
import com.dotlink.R
import com.dotlink.ui.MainActivity
import com.dotlink.ui.formatSize

/**
 * One notification per file while it goes to or comes from the PC, with a progress bar.
 * Sent files end with "Sent to …"; received ones are replaced by Link's "Received from …".
 */
class TransferNotifier(
    private val context: Context,
    private val link: Link,
) {
    private val nm = context.getSystemService(NotificationManager::class.java)

    /** Transfers we have shown while running, and when we last updated each. */
    private val running = mutableMapOf<Long, Long>()

    @SuppressLint("MissingPermission") // Checked by areNotificationsEnabled.
    fun update(transfers: List<Transfer>) {
        if (!nm.areNotificationsEnabled()) return
        val now = System.currentTimeMillis()
        for (t in transfers) {
            val id = NOTIFICATION_BASE + t.id.toInt()
            when (t.status) {
                Transfer.Status.Running -> {
                    // The system drops updates that come faster than a few per second.
                    val last = running[t.id]
                    if (last != null && now - last < 500) continue
                    running[t.id] = now
                    nm.notify(id, progress(t))
                }
                Transfer.Status.Done ->
                    if (running.remove(t.id) != null) {
                        if (t.incoming) nm.cancel(id) else nm.notify(id, finished(t, "Sent to ${link.prefs.serverName}", t.name))
                    }
                Transfer.Status.Failed ->
                    if (running.remove(t.id) != null) {
                        val verb = if (t.incoming) "receive" else "send"
                        nm.notify(id, finished(t, "Couldn't $verb ${t.name}", t.error ?: "Failed"))
                    }
            }
        }
    }

    private fun builder() =
        NotificationCompat
            .Builder(context, DotlinkApp.CHANNEL_TRANSFERS)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentIntent(
                PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE),
            ).setSilent(true)

    private fun progress(t: Transfer) =
        builder()
            .setContentTitle(if (t.incoming) "Receiving ${t.name}" else "Sending ${t.name}")
            .setContentText(
                if (t.size > 0) "${formatSize(t.done)} of ${formatSize(t.size)}" else formatSize(t.done),
            ).setProgress(100, if (t.size > 0) (t.done * 100 / t.size).toInt().coerceIn(0, 100) else 0, t.size <= 0)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()

    private fun finished(
        t: Transfer,
        title: String,
        text: String,
    ) = builder()
        .setContentTitle(title)
        .setContentText(text)
        .setAutoCancel(true)
        .apply { if (t.status == Transfer.Status.Done) setTimeoutAfter(5_000) }
        .build()

    private companion object {
        const val NOTIFICATION_BASE = 1000
    }
}
