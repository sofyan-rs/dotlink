package com.dotlink.link

import android.content.ComponentName
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.media.MediaMetadata
import android.media.session.MediaController
import android.media.session.MediaSessionManager
import android.media.session.PlaybackState
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Base64
import android.util.Log
import com.dotlink.DotlinkApp
import org.json.JSONObject
import java.io.ByteArrayOutputStream

/** What plays on the PC, from its `media_state`, for the Media card. */
data class PcMedia(
    val player: String,
    val title: String,
    val artist: String,
    val playing: Boolean,
    val positionMs: Long,
    val durationMs: Long,
    /** 0–100; null when the PC can't read it. */
    val volume: Int?,
    val art: Bitmap?,
    /** When [positionMs] was true (elapsedRealtime), to move the bar on while it plays. */
    val at: Long,
)

/**
 * Media control both ways. This phone's player (the active media sessions, which Android only
 * shows to an app with Notification access, see [NotifListener]) goes to the PC as
 * `media_state`; the PC's `media_cmd` reaches it through its transport controls. The PC's own
 * player arrives as `media_state` too ([fromPc]) and its card sends `media_cmd` back.
 */
object MediaBridge {
    private const val TAG = "Dotlink"
    private val main = Handler(Looper.getMainLooper())
    private var manager: MediaSessionManager? = null
    private var controller: MediaController? = null

    /** The track whose artwork the PC has: title, artist, album. */
    private var artSentFor: String? = null
    private var lastSent: String? = null

    private val callback =
        object : MediaController.Callback() {
            override fun onMetadataChanged(metadata: MediaMetadata?) = schedule()

            override fun onPlaybackStateChanged(state: PlaybackState?) = schedule()

            override fun onSessionDestroyed() {
                main.post { pick() }
            }
        }

    private val sessionsChanged = MediaSessionManager.OnActiveSessionsChangedListener { main.post { pick() } }

    /** A new connection to the PC: it gets the player (and its artwork) again. */
    fun connected(context: Context) =
        main.post {
            artSentFor = null
            lastSent = null
            if (manager == null) start(context) else pick()
        }

    /** Starts following the phone's media sessions; needs Notification access. */
    private fun start(context: Context) {
        if (!NotifListener.enabled(context)) return
        val m = context.getSystemService(MediaSessionManager::class.java) ?: return
        val listener = ComponentName(context, NotifListener::class.java)
        try {
            m.addOnActiveSessionsChangedListener(sessionsChanged, listener, main)
        } catch (e: SecurityException) {
            Log.w(TAG, "media sessions: $e")
            return
        }
        manager = m
        pick()
    }

    /** Follows the playing session, else the first one. */
    private fun pick() {
        val context = DotlinkApp.instanceContext
        val sessions =
            try {
                manager?.getActiveSessions(ComponentName(context, NotifListener::class.java)).orEmpty()
            } catch (_: SecurityException) {
                emptyList()
            }
        val next = sessions.firstOrNull { it.playbackState?.state == PlaybackState.STATE_PLAYING } ?: sessions.firstOrNull()
        if (next?.sessionToken != controller?.sessionToken) {
            controller?.unregisterCallback(callback)
            controller = next
            next?.registerCallback(callback, main)
        }
        schedule()
    }

    private val send = Runnable { sendState() }

    /** Several callbacks come together for one change: send once, a moment later. */
    private fun schedule() {
        main.removeCallbacks(send)
        main.postDelayed(send, 300)
    }

    private fun sendState() {
        val link = DotlinkApp.link
        val c = controller
        val meta = c?.metadata
        val title = meta?.getString(MediaMetadata.METADATA_KEY_TITLE).orEmpty()
        if (c == null || meta == null || title.isEmpty()) {
            if (lastSent != "none" && link.sendControl(JSONObject().put("t", "media_state").put("player", JSONObject.NULL))) lastSent = "none"
            return
        }
        val state = c.playbackState
        val playing = state?.state == PlaybackState.STATE_PLAYING
        var position = state?.position ?: 0
        if (playing && state != null && state.lastPositionUpdateTime > 0) {
            position += ((SystemClock.elapsedRealtime() - state.lastPositionUpdateTime) * state.playbackSpeed).toLong()
        }
        val artist = meta.getString(MediaMetadata.METADATA_KEY_ARTIST) ?: meta.getString(MediaMetadata.METADATA_KEY_ALBUM_ARTIST).orEmpty()
        val album = meta.getString(MediaMetadata.METADATA_KEY_ALBUM).orEmpty()
        val player =
            runCatching {
                val pm = DotlinkApp.instanceContext.packageManager
                pm.getApplicationLabel(pm.getApplicationInfo(c.packageName, 0)).toString()
            }.getOrDefault(c.packageName)
        val msg =
            JSONObject()
                .put("t", "media_state")
                .put("player", player)
                .put("title", title)
                .put("artist", artist)
                .put("album", album)
                .put("playing", playing)
                .put("position", position.coerceAtLeast(0))
                .put("duration", meta.getLong(MediaMetadata.METADATA_KEY_DURATION).coerceAtLeast(0))
        val track = "$title\n$artist\n$album"
        if (track != artSentFor) {
            art(meta)?.let { msg.put("art", it) }
        }
        val key = msg.toString()
        if (key == lastSent) return
        if (link.sendControl(msg)) {
            lastSent = key
            artSentFor = track
        }
    }

    /** The artwork as a JPEG of at most 300 px, base64. */
    private fun art(meta: MediaMetadata): String? {
        val bitmap =
            meta.getBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART)
                ?: meta.getBitmap(MediaMetadata.METADATA_KEY_ART)
                ?: meta.getBitmap(MediaMetadata.METADATA_KEY_DISPLAY_ICON)
                ?: return null
        return runCatching {
            val scale = 300f / maxOf(bitmap.width, bitmap.height)
            val small = if (scale < 1) Bitmap.createScaledBitmap(bitmap, (bitmap.width * scale).toInt(), (bitmap.height * scale).toInt(), true) else bitmap
            val out = ByteArrayOutputStream()
            small.compress(Bitmap.CompressFormat.JPEG, 85, out)
            Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP)
        }.getOrNull()
    }

    /** `media_cmd` from the PC, for this phone's player. */
    fun command(msg: JSONObject) =
        main.post {
            val c = controller ?: return@post
            val t = c.transportControls
            when (msg.optString("cmd")) {
                "play" -> t.play()
                "pause" -> t.pause()
                "toggle" -> if (c.playbackState?.state == PlaybackState.STATE_PLAYING) t.pause() else t.play()
                "next" -> t.skipToNext()
                "prev" -> t.skipToPrevious()
                "seek" -> t.seekTo(msg.optLong("ms"))
                "volume" -> c.playbackInfo?.let { c.setVolumeTo(msg.optInt("level") * it.maxVolume / 100, 0) }
            }
        }

    // ---------- the PC's player ----------

    /** The PC's `media_state`: the artwork comes only when the track changes, so it's kept. */
    fun fromPc(msg: JSONObject) {
        val link = DotlinkApp.link
        if (msg.isNull("player")) {
            link.pcMedia.value = null
            return
        }
        val art =
            msg.optString("art").takeIf { it.isNotEmpty() }?.let {
                runCatching { Base64.decode(it, Base64.DEFAULT).let { b -> BitmapFactory.decodeByteArray(b, 0, b.size) } }.getOrNull()
            }
        val old = link.pcMedia.value
        val title = msg.optString("title")
        val artist = msg.optString("artist")
        val sameTrack = old != null && old.title == title && old.artist == artist
        link.pcMedia.value =
            PcMedia(
                player = msg.optString("player"),
                title = title,
                artist = artist,
                playing = msg.optBoolean("playing"),
                positionMs = msg.optLong("position"),
                durationMs = msg.optLong("duration"),
                volume = if (msg.has("volume")) msg.optInt("volume") else null,
                art = art ?: old?.art?.takeIf { sameTrack },
                at = SystemClock.elapsedRealtime(),
            )
    }

    /** A button on the Media card: `cmd` for the PC's player. */
    fun toPc(
        cmd: String,
        level: Int? = null,
    ) {
        val msg = JSONObject().put("t", "media_cmd").put("cmd", cmd)
        level?.let { msg.put("level", it.coerceIn(0, 100)) }
        DotlinkApp.link.sendControl(msg)
    }
}
