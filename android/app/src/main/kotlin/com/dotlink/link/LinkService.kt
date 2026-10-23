package com.dotlink.link

import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import com.dotlink.DotlinkApp
import com.dotlink.R
import com.dotlink.ui.ClipboardActivity
import com.dotlink.ui.MainActivity
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch

/**
 * Keeps the link to the PC alive in the background (clipboard, incoming files), and hosts the
 * camera and microphone streams so they keep running with the screen off.
 */
class LinkService : LifecycleService() {
    private val link get() = DotlinkApp.link
    private var camera: CameraStreamer? = null

    /** Start the camera again as soon as the current one has fully stopped (new settings). */
    private var restartCamera = false
    private var mic: MicStreamer? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null

    override fun onCreate() {
        super.onCreate()
        promote()
        link.start()
        lifecycleScope.launch {
            combine(link.state, link.cameraOn, link.micOn) { s, c, m -> Triple(s, c, m) }.collect { (s) ->
                if (s is LinkState.Unpaired) stopSelf() else promote()
            }
        }
        val cm = getSystemService(ConnectivityManager::class.java)
        networkCallback =
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) = link.kick()
            }.also { cm.registerDefaultNetworkCallback(it) }
    }

    override fun onStartCommand(
        intent: Intent?,
        flags: Int,
        startId: Int,
    ): Int {
        super.onStartCommand(intent, flags, startId)
        when (intent?.action) {
            ACTION_CAMERA_ON -> startCamera()
            ACTION_CAMERA_OFF -> camera?.stop()
            ACTION_CAMERA_REBIND -> camera?.rebind()
            ACTION_CAMERA_RESTART ->
                camera?.let {
                    restartCamera = true
                    it.stop()
                }
            ACTION_MIC_ON -> startMic()
            ACTION_MIC_OFF -> mic?.stop()
            ACTION_DISCONNECT -> {
                camera?.stop()
                mic?.stop()
                link.stop()
                stopSelf()
            }
            else -> link.kick()
        }
        return START_STICKY
    }

    private fun startCamera() {
        if (camera != null) return
        link.cameraOn.value = true
        promote()
        camera =
            CameraStreamer(this, this, link) { error ->
                camera = null
                if (restartCamera && error == null) {
                    restartCamera = false
                    startCamera()
                    return@CameraStreamer
                }
                restartCamera = false
                link.cameraOn.value = false
                error?.let { link.notices.tryEmit("Webcam stopped: $it") }
            }.also { it.start() }
    }

    private fun startMic() {
        if (mic != null) return
        link.micOn.value = true
        promote()
        mic =
            MicStreamer(link) { error ->
                mic = null
                link.micOn.value = false
                error?.let { link.notices.tryEmit("Microphone stopped: $it") }
            }.also { it.start() }
    }

    /** (Re)starts foreground mode with the types we currently need. */
    private fun promote() {
        var types = ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        if (link.cameraOn.value) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA
        if (link.micOn.value) types = types or ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification(), types)
    }

    private fun notification(): Notification {
        val state = link.state.value
        val title =
            when (state) {
                is LinkState.Online -> "Connected to ${state.pcName}"
                LinkState.Connecting -> "Connecting to ${link.prefs.serverName}…"
                else -> "${link.prefs.serverName} is offline"
            }
        val extras = listOfNotNull("Webcam on".takeIf { link.cameraOn.value }, "Mic on".takeIf { link.micOn.value })
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val clip =
            PendingIntent.getActivity(
                this,
                1,
                Intent(this, ClipboardActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
                PendingIntent.FLAG_IMMUTABLE,
            )
        val disconnect = PendingIntent.getService(this, 2, intent(this, ACTION_DISCONNECT), PendingIntent.FLAG_IMMUTABLE)
        return NotificationCompat
            .Builder(this, DotlinkApp.CHANNEL_LINK)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(extras.joinToString(" · ").ifEmpty { null })
            .setContentIntent(open)
            .setOngoing(true)
            .setSilent(true)
            .addAction(0, "Send clipboard", clip)
            .addAction(0, "Disconnect", disconnect)
            .build()
    }

    override fun onDestroy() {
        networkCallback?.let { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) }
        camera?.stop()
        mic?.stop()
        super.onDestroy()
    }

    companion object {
        private const val NOTIFICATION_ID = 1
        const val ACTION_CAMERA_ON = "camera_on"
        const val ACTION_CAMERA_OFF = "camera_off"
        const val ACTION_CAMERA_REBIND = "camera_rebind"
        const val ACTION_CAMERA_RESTART = "camera_restart"
        const val ACTION_MIC_ON = "mic_on"
        const val ACTION_MIC_OFF = "mic_off"
        const val ACTION_DISCONNECT = "disconnect"

        private fun intent(
            context: Context,
            action: String?,
        ) = Intent(context, LinkService::class.java).setAction(action)

        /** Starts (or pokes) the service. Call from the foreground only. */
        fun send(
            context: Context,
            action: String? = null,
        ) {
            ContextCompat.startForegroundService(context, intent(context, action))
        }
    }
}
