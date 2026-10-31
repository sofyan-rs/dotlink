package com.dotlink.link

import android.annotation.SuppressLint
import android.app.Application
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.MediaPlayer
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import android.util.Log
import androidx.core.app.NotificationCompat
import com.dotlink.DotlinkApp
import com.dotlink.R
import com.dotlink.ui.RingActivity
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * "Ring" from the PC (`ring`): the default ringtone at full volume, looping, until it's stopped
 * here or on the PC, or [TIMEOUT_MS] passed. It plays as an alarm (`USAGE_ALARM`), which silent
 * and vibrate modes don't mute; the alarm volume is raised to its maximum and put back after.
 * [onChange] tells the PC whether it rings.
 */
class Ringer(
    private val app: Application,
    private val onChange: (Boolean) -> Unit,
) {
    val ringing = MutableStateFlow(false)

    /** Who rang, for [RingActivity]. */
    @Volatile var from: String = ""
        private set

    private var player: MediaPlayer? = null
    private var savedVolume: Int? = null
    private val main = Handler(Looper.getMainLooper())
    private val timeout = Runnable { stop() }

    @Synchronized
    fun start(from: String) {
        if (player != null) return
        this.from = from
        val am = app.getSystemService(AudioManager::class.java)
        savedVolume = am.getStreamVolume(AudioManager.STREAM_ALARM)
        runCatching { am.setStreamVolume(AudioManager.STREAM_ALARM, am.getStreamMaxVolume(AudioManager.STREAM_ALARM), 0) }
        // The ringtone, else the alarm sound (a phone set to no ringtone).
        player =
            listOf(Settings.System.DEFAULT_RINGTONE_URI, Settings.System.DEFAULT_ALARM_ALERT_URI).firstNotNullOfOrNull { play(it) }
        if (player == null) {
            Log.w(TAG, "no ringtone to play")
            restoreVolume()
            return
        }
        Log.i(TAG, "ringing for $from")
        main.postDelayed(timeout, TIMEOUT_MS)
        notify(from)
        ringing.value = true
        onChange(true)
    }

    @Synchronized
    fun stop() {
        val p = player ?: return
        player = null
        main.removeCallbacks(timeout)
        runCatching { p.stop() }
        p.release()
        restoreVolume()
        app.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION_ID)
        ringing.value = false
        onChange(false)
    }

    private fun play(uri: Uri): MediaPlayer? =
        try {
            MediaPlayer().apply {
                setAudioAttributes(
                    AudioAttributes
                        .Builder()
                        .setUsage(AudioAttributes.USAGE_ALARM)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                        .build(),
                )
                setDataSource(app, uri)
                isLooping = true
                prepare()
                start()
            }
        } catch (e: Exception) {
            Log.w(TAG, "can't play $uri", e)
            null
        }

    private fun restoreVolume() {
        val volume = savedVolume ?: return
        savedVolume = null
        runCatching { app.getSystemService(AudioManager::class.java).setStreamVolume(AudioManager.STREAM_ALARM, volume, 0) }
    }

    /**
     * "Ringing from …" with Stop, full screen over the lock screen (Android 14+ may show it as a
     * heads-up notification when full-screen notifications aren't allowed for the app).
     */
    @SuppressLint("MissingPermission") // Checked by NotificationManager.areNotificationsEnabled.
    private fun notify(from: String) {
        val nm = app.getSystemService(NotificationManager::class.java)
        if (!nm.areNotificationsEnabled()) return
        val screen =
            PendingIntent.getActivity(
                app,
                NOTIFICATION_ID,
                Intent(app, RingActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_IMMUTABLE,
            )
        val stop =
            PendingIntent.getService(
                app,
                NOTIFICATION_ID,
                Intent(app, LinkService::class.java).setAction(LinkService.ACTION_RING_STOP),
                PendingIntent.FLAG_IMMUTABLE,
            )
        val n =
            NotificationCompat
                .Builder(app, DotlinkApp.CHANNEL_RING)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle("Ringing from $from")
                .setContentText("Tap Stop to silence it")
                .setCategory(NotificationCompat.CATEGORY_ALARM)
                .setPriority(NotificationCompat.PRIORITY_MAX)
                .setOngoing(true)
                .setSilent(true)
                .setContentIntent(screen)
                .setFullScreenIntent(screen, true)
                .addAction(0, "Stop", stop)
                .build()
        nm.notify(NOTIFICATION_ID, n)
    }

    companion object {
        private const val TAG = "Dotlink"
        private const val NOTIFICATION_ID = 40
        const val TIMEOUT_MS = 60_000L
    }
}
