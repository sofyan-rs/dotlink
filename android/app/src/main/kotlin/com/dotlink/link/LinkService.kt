package com.dotlink.link

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.wifi.WifiManager
import android.provider.Settings
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import com.dotlink.DotlinkApp
import com.dotlink.R
import com.dotlink.ui.ClipboardActivity
import com.dotlink.ui.MainActivity
import com.dotlink.ui.StreamActivity
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch

/**
 * Keeps the link to the PC alive in the background (clipboard, incoming files), and hosts the
 * camera and microphone streams so they keep running with the screen off.
 */
class LinkService : LifecycleService() {
    private val link get() = DotlinkApp.link
    private val phones get() = DotlinkApp.phones
    private var camera: CameraStreamer? = null

    /** Start the camera again as soon as the current one has fully stopped (new settings). */
    private var restartCamera = false
    private var mic: MicStreamer? = null
    private var networkCallback: ConnectivityManager.NetworkCallback? = null

    /*
     * Wi-Fi power saving batches a phone's packets: pings to it took 90 ms on average, up to
     * 300 ms. Remote, Shell, the webcam and the mic all felt that. The low-latency lock only
     * applies while our app is on screen (no cost otherwise); the high-performance one keeps
     * streams smooth with the screen off.
     */
    private val wifi by lazy { applicationContext.getSystemService(WifiManager::class.java) }
    private val lowLatency by lazy { wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "dotlink:link").apply { setReferenceCounted(false) } }

    @Suppress("DEPRECATION") // Still honoured before Android 14; harmless after.
    private val highPerf by lazy { wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "dotlink:stream").apply { setReferenceCounted(false) } }

    private fun WifiManager.WifiLock.hold(on: Boolean) {
        if (on && !isHeld) acquire() else if (!on && isHeld) release()
    }

    override fun onCreate() {
        super.onCreate()
        promote()
        link.start()
        lifecycleScope.launch {
            combine(link.state, link.cameraOn, link.micOn, phones.list) { s, c, m, _ -> Triple(s, c, m) }.collect { (s, c, m) ->
                lowLatency.hold(s is LinkState.Online)
                highPerf.hold(s is LinkState.Online && (c || m))
                // Paired phones send to this one at any time: keep listening for them.
                if (phones.needed) phones.startServer() else phones.stopServer()
                if (s is LinkState.Unpaired && !phones.needed) stopSelf() else promote()
            }
        }
        // A code shown on screen expires after 5 minutes: stop listening then if nothing else needs it.
        lifecycleScope.launch {
            while (true) {
                delay(60_000)
                if (!phones.needed) {
                    phones.stopServer()
                    if (link.state.value is LinkState.Unpaired) stopSelf()
                }
            }
        }
        lifecycleScope.launch {
            link.streamRequests.collect { (kind, on) -> onStreamRequest(kind, on) }
        }
        lifecycleScope.launch { link.adbWifiRequests.collect { showAdbWifiPrompt() } }
        val transfers = TransferNotifier(this, link)
        lifecycleScope.launch { link.transfers.collect { transfers.update(it) } }
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
            ACTION_RING_STOP -> link.ringer.stop()
            ACTION_ANSWER -> {
                val id = intent.getStringExtra(EXTRA_ASK)
                if (id != null) link.answerIncoming(id, intent.getBooleanExtra(EXTRA_ACCEPT, false))
            }
            ACTION_DISCONNECT -> {
                camera?.stop()
                mic?.stop()
                link.disconnect()
                // Paired phones still send here: only the PC link stops then.
                if (phones.needed) promote() else stopSelf()
            }
            ACTION_SWITCH -> {
                // Streams go to the old PC; stop them before leaving it.
                camera?.stop()
                mic?.stop()
                link.stop()
                intent.getStringExtra(EXTRA_PC)?.let { link.switchTo(it) }
                link.start()
            }
            else -> {
                // Paired while this service already ran for paired phones: onCreate's start()
                // found nothing to connect to then. A no-op when the link is already running.
                link.start()
                link.kick()
            }
        }
        return START_STICKY
    }

    private fun startCamera() {
        if (camera != null) return
        link.cameraOn.value = true
        promote()
        cancelStreamPrompt("camera")
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
        cancelStreamPrompt("mic")
        mic =
            MicStreamer(link) { error ->
                mic = null
                link.micOn.value = false
                error?.let { link.notices.tryEmit("Microphone stopped: $it") }
            }.also { it.start() }
    }

    /**
     * The PC asked to turn [kind] on or off. Stopping always works. Starting works only while
     * one of our activities is on screen (Android 14+ keeps background apps off the camera and
     * mic); otherwise we post a notification whose Start button opens [StreamActivity].
     */
    private fun onStreamRequest(
        kind: String,
        on: Boolean,
    ) {
        val camera = kind == "camera"
        if (!on) {
            cancelStreamPrompt(kind)
            if (camera) this.camera?.stop() else mic?.stop()
            return
        }
        if ((camera && this.camera != null) || (!camera && mic != null)) return
        val permission = if (camera) Manifest.permission.CAMERA else Manifest.permission.RECORD_AUDIO
        val granted = ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED
        if (granted && DotlinkApp.inForeground) {
            try {
                if (camera) startCamera() else startMic()
                return
            } catch (e: SecurityException) {
                // Lost the foreground in between: fall back to asking.
                if (camera) link.cameraOn.value = false else link.micOn.value = false
                promote()
            }
        }
        showStreamPrompt(kind)
        link.sendStreamNeedsTap(kind)
    }

    private fun showStreamPrompt(kind: String) {
        val what = if (kind == "camera") "webcam" else "microphone"
        val start =
            PendingIntent.getActivity(
                this,
                if (kind == "camera") 10 else 11,
                StreamActivity.intent(this, kind),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val n =
            NotificationCompat
                .Builder(this, DotlinkApp.CHANNEL_REQUESTS)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle("${link.prefs.serverName} wants your $what")
                .setContentText("Tap Start to use this phone as the PC's $what.")
                .setContentIntent(start)
                .addAction(0, "Start", start)
                .setAutoCancel(true)
                .setTimeoutAfter(60_000)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
                .build()
        getSystemService(NotificationManager::class.java).notify(promptId(kind), n)
    }

    /**
     * The PC wants to cast this screen but can't reach it: Wireless debugging is off (or the
     * phone isn't plugged in). Opens Developer options at that switch, or About phone when
     * Developer options are off (tap Build number 7 times).
     */
    private fun showAdbWifiPrompt() {
        val devOptions = Settings.Global.getInt(contentResolver, Settings.Global.DEVELOPMENT_SETTINGS_ENABLED, 0) == 1
        val target =
            if (devOptions) {
                Intent(Settings.ACTION_APPLICATION_DEVELOPMENT_SETTINGS)
                    // Scrolls to and highlights the switch on AOSP-based Settings.
                    .putExtra(":settings:fragment_args_key", "toggle_adb_wireless")
            } else {
                Intent(Settings.ACTION_DEVICE_INFO_SETTINGS)
            }.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        val open = PendingIntent.getActivity(this, 12, target, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val text =
            if (devOptions) {
                "Turn on Wireless debugging to show this screen on the PC. Or plug the phone in over USB."
            } else {
                "Turn on Developer options first: tap Build number 7 times, then turn on Wireless debugging."
            }
        val n =
            NotificationCompat
                .Builder(this, DotlinkApp.CHANNEL_REQUESTS)
                .setSmallIcon(R.drawable.ic_notification)
                .setContentTitle("${link.prefs.serverName} wants to cast this screen")
                .setContentText(text)
                .setStyle(NotificationCompat.BigTextStyle().bigText(text))
                .setContentIntent(open)
                .addAction(0, if (devOptions) "Open settings" else "Open About phone", open)
                .setAutoCancel(true)
                .setTimeoutAfter(120_000)
                .setPriority(NotificationCompat.PRIORITY_HIGH)
                .build()
        getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ADB_WIFI_PROMPT, n)
    }

    private fun cancelStreamPrompt(kind: String) = getSystemService(NotificationManager::class.java).cancel(promptId(kind))

    private fun promptId(kind: String) = if (kind == "camera") NOTIFICATION_CAMERA_PROMPT else NOTIFICATION_MIC_PROMPT

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
                LinkState.Unpaired -> "Ready for your phones"
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
        phones.stopServer()
        networkCallback?.let { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) }
        camera?.stop()
        mic?.stop()
        lowLatency.hold(false)
        highPerf.hold(false)
        super.onDestroy()
    }

    companion object {
        private const val NOTIFICATION_ID = 1
        private const val NOTIFICATION_CAMERA_PROMPT = 2
        private const val NOTIFICATION_MIC_PROMPT = 3
        private const val NOTIFICATION_ADB_WIFI_PROMPT = 4
        const val ACTION_CAMERA_ON = "camera_on"
        const val ACTION_CAMERA_OFF = "camera_off"
        const val ACTION_CAMERA_REBIND = "camera_rebind"
        const val ACTION_CAMERA_RESTART = "camera_restart"
        const val ACTION_MIC_ON = "mic_on"
        const val ACTION_MIC_OFF = "mic_off"
        const val ACTION_DISCONNECT = "disconnect"

        /** Accept or Decline on an incoming file ([EXTRA_ASK], [EXTRA_ACCEPT]). */
        const val ACTION_ANSWER = "answer"
        const val EXTRA_ASK = "ask"
        const val EXTRA_ACCEPT = "accept"

        /** Stop on the "Ringing from …" notification. */
        const val ACTION_RING_STOP = "ring_stop"

        /** Reconnect to the PC in [EXTRA_PC], or to the active PC again (after pairing or unpairing one). */
        const val ACTION_SWITCH = "switch"
        const val EXTRA_PC = "pc"

        private fun intent(
            context: Context,
            action: String?,
        ) = Intent(context, LinkService::class.java).setAction(action)

        /** Starts (or pokes) the service. Call from the foreground only. */
        fun send(
            context: Context,
            action: String? = null,
            pc: String? = null,
        ) {
            ContextCompat.startForegroundService(context, intent(context, action).putExtra(EXTRA_PC, pc))
        }
    }
}
