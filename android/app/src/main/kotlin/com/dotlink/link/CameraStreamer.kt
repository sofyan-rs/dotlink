package com.dotlink.link

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.hardware.camera2.CaptureRequest
import android.util.Range
import android.util.Size
import android.view.Surface
import android.util.Log
import androidx.annotation.OptIn
import androidx.camera.camera2.interop.Camera2CameraInfo
import androidx.camera.camera2.interop.Camera2Interop
import androidx.camera.camera2.interop.ExperimentalCamera2Interop
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.resolutionselector.AspectRatioStrategy
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.cancelChildren
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

/**
 * Streams the phone camera to the PC as MJPEG over a WebSocket. The PC feeds it into
 * v4l2loopback, where it shows up as "Dotlink Camera".
 */
class CameraStreamer(
    private val context: Context,
    private val owner: LifecycleOwner,
    private val link: Link,
    private val onStopped: (error: String?) -> Unit,
) {
    private val executor = Executors.newSingleThreadExecutor()
    private var provider: ProcessCameraProvider? = null
    private var analysis: ImageAnalysis? = null
    private var camera: Camera? = null
    private val controls = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private var lastLog = 0L
    /** Frames sent since [lastLog], for the log's fps. */
    private var sentSinceLog = 0
    private var lastPreview = 0L
    private var socket: WebSocket? = null

    @Volatile private var ready = false

    @Volatile private var stopped = false
    /** When the next frame is due (ms). */
    private var nextFrame = 0L
    private val jpeg = JpegBuffer(512 * 1024)

    // Reused for every frame (analyzer thread only): a 1080p frame is 8 MB of pixels, and a
    // new bitmap per frame kept the GC busy at 30 fps.
    private var pixels: Bitmap? = null
    private var mirrored: Bitmap? = null

    fun start() {
        val prefs = link.prefs
        val (long, short) = if (prefs.fullHd) 1920 to 1080 else 1280 to 720
        val (w, h) = if (prefs.rotation % 180 == 90) long to short else short to long
        socket =
            link.openSocket(
                "/ws/camera",
                object : WebSocketListener() {
                    override fun onOpen(
                        webSocket: WebSocket,
                        response: Response,
                    ) {
                        webSocket.send(JSONObject().put("w", w).put("h", h).toString())
                    }

                    override fun onMessage(
                        webSocket: WebSocket,
                        text: String,
                    ) {
                        val msg = JSONObject(text)
                        if (msg.optBoolean("ok")) ready = true
                        msg.optString("error").takeIf { it.isNotEmpty() }?.let { stop(it) }
                    }

                    override fun onFailure(
                        webSocket: WebSocket,
                        t: Throwable,
                        response: Response?,
                    ) = stop(t.message ?: "connection lost")

                    override fun onClosed(
                        webSocket: WebSocket,
                        code: Int,
                        reason: String,
                    ) = stop(null)
                },
            )
        if (socket == null) {
            stop("PC not connected")
            return
        }
        val future = ProcessCameraProvider.getInstance(context)
        future.addListener({
            if (stopped) return@addListener
            val p = future.get()
            provider = p
            bind(p, w, h)
        }, ContextCompat.getMainExecutor(context))
    }

    @OptIn(ExperimentalCamera2Interop::class)
    private fun bind(
        p: ProcessCameraProvider,
        w: Int,
        h: Int,
    ) {
        val prefs = link.prefs
        val cam = if (prefs.frontCamera) CameraSelector.DEFAULT_FRONT_CAMERA else CameraSelector.DEFAULT_BACK_CAMERA
        // CameraX runs analysis at the camera's default rate (about 30 fps); 60 has to be asked
        // for, as an auto-exposure range the camera lists. Without one, the best it has.
        val fpsRange = frameRate(p, cam, prefs.fps)
        val minInterval = 1000L / fpsRange.upper.coerceAtMost(prefs.fps)
        val selector =
            ResolutionSelector
                .Builder()
                .setAspectRatioStrategy(AspectRatioStrategy.RATIO_16_9_FALLBACK_AUTO_STRATEGY)
                .setResolutionStrategy(
                    ResolutionStrategy(Size(maxOf(w, h), minOf(w, h)), ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER),
                ).build()
        val builder =
            ImageAnalysis
                .Builder()
                .setResolutionSelector(selector)
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_RGBA_8888)
                .setOutputImageRotationEnabled(true)
                .setTargetRotation(surfaceRotation(link.cameraRotation.value))
        Camera2Interop.Extender(builder).setCaptureRequestOption(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, fpsRange)
        val a = builder.build()
        a.setAnalyzer(executor) { image ->
            image.use {
                val ws = socket ?: return@use
                val now = System.currentTimeMillis()
                // Skip when too soon, or while the last frame is still waiting to go out: frames
                // that queue up behind it would only reach the PC later and later. "Too soon" goes
                // by a schedule with some slack: with a plain minimum gap, a 30 fps camera's
                // frames that came a millisecond early were dropped, and 30 fps became ~20.
                if (!ready || nextFrame - now > minInterval / 4 || ws.queueSize() > 0) return@use
                nextFrame = maxOf(nextFrame, now - minInterval) + minInterval
                val raw = pixelsOf(it)
                val bmp = if (link.cameraMirror.value) mirror(raw) else raw
                jpeg.reset()
                bmp.compress(Bitmap.CompressFormat.JPEG, 75, jpeg)
                ws.send(jpeg.bytes())
                sentSinceLog++
                if (now - lastLog > 2000) {
                    val fps = sentSinceLog * 1000f / (now - lastLog)
                    lastLog = now
                    sentSinceLog = 0
                    Log.d("Dotlink", "camera frame ${bmp.width}x${bmp.height} jpeg=${jpeg.size()}B %.1f fps (camera $fpsRange)".format(fps))
                }
                // The preview shows exactly this frame (~12 fps is plenty for framing the shot), as
                // a small copy of its own: the reused bitmaps change under the UI otherwise.
                if (link.cameraPreviewActive.value && now - lastPreview > 80) {
                    lastPreview = now
                    link.cameraFrame.value = previewOf(bmp)
                }
                if (raw !== pixels) raw.recycle()
            }
        }
        analysis = a
        try {
            p.unbindAll()
            camera = p.bindToLifecycle(owner, cam, a)
        } catch (e: Exception) {
            stop(e.message ?: "camera unavailable")
            return
        }
        controls.coroutineContext.cancelChildren()
        // Rotation applies live: CameraX rotates the next frames, no rebind needed.
        controls.launch { link.cameraRotation.collect { a.targetRotation = surfaceRotation(it) } }
        controls.launch { link.cameraTorch.collect { camera?.cameraControl?.enableTorch(it) } }
        controls.launch {
            link.cameraZoom.collect { ratio ->
                val c = camera ?: return@collect
                val zoom = c.cameraInfo.zoomState.value
                c.cameraControl.setZoomRatio(ratio.coerceIn(zoom?.minZoomRatio ?: 1f, zoom?.maxZoomRatio ?: 1f))
            }
        }
    }

    /**
     * The camera's auto-exposure range for [fps]: fixed `[fps, fps]` when it has one (a steady
     * rate), else the range reaching highest without going over, else `[fps, fps]` as asked.
     */
    @OptIn(ExperimentalCamera2Interop::class)
    private fun frameRate(
        p: ProcessCameraProvider,
        selector: CameraSelector,
        fps: Int,
    ): Range<Int> {
        val ranges =
            runCatching {
                val info = selector.filter(p.availableCameraInfos).firstOrNull()
                info?.let { Camera2CameraInfo.from(it).getCameraCharacteristic(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES) }
            }.getOrNull().orEmpty().toList()
        return ranges.firstOrNull { it.lower == fps && it.upper == fps }
            ?: ranges.filter { it.upper <= fps }.maxWithOrNull(compareBy<Range<Int>> { it.upper }.thenBy { it.lower })
            ?: Range(fps, fps)
    }

    /** The frame's RGBA pixels, in a bitmap reused from frame to frame when the rows are unpadded. */
    private fun pixelsOf(image: ImageProxy): Bitmap {
        val plane = image.planes[0]
        val (w, h) = image.width to image.height
        if (plane.pixelStride != 4 || plane.rowStride != w * 4) return image.toBitmap()
        val bmp = pixels?.takeIf { it.width == w && it.height == h } ?: Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888).also { pixels = it }
        plane.buffer.rewind()
        bmp.copyPixelsFromBuffer(plane.buffer)
        return bmp
    }

    /** [src] flipped left-right, into a reused bitmap. */
    private fun mirror(src: Bitmap): Bitmap {
        val out =
            mirrored?.takeIf { it.width == src.width && it.height == src.height }
                ?: Bitmap.createBitmap(src.width, src.height, Bitmap.Config.ARGB_8888).also { mirrored = it }
        Canvas(out).apply {
            scale(-1f, 1f, src.width / 2f, 0f)
            drawBitmap(src, 0f, 0f, null)
        }
        return out
    }

    private fun previewOf(bmp: Bitmap): Bitmap {
        val scale = PREVIEW_SIZE / maxOf(bmp.width, bmp.height)
        if (scale >= 1f) return bmp.copy(Bitmap.Config.ARGB_8888, false)
        return Bitmap.createScaledBitmap(bmp, (bmp.width * scale).toInt(), (bmp.height * scale).toInt(), true)
    }

    /** Re-binds with the current settings (e.g. after switching front/back). */
    fun rebind() {
        val p = provider ?: return
        val (long, short) = if (link.prefs.fullHd) 1920 to 1080 else 1280 to 720
        val (w, h) = if (link.prefs.rotation % 180 == 90) long to short else short to long
        ContextCompat.getMainExecutor(context).execute { if (!stopped) bind(p, w, h) }
    }

    fun stop(error: String? = null) {
        if (stopped) return
        stopped = true
        ready = false
        socket?.close(1000, null)
        socket = null
        ContextCompat.getMainExecutor(context).execute {
            controls.cancel()
            link.cameraTorch.value = false
            link.cameraZoom.value = 1f
            link.cameraFrame.value = null
            analysis?.clearAnalyzer()
            provider?.unbindAll()
            executor.shutdown()
            onStopped(error)
        }
    }

    companion object {
        /** The frame rates the Camera tab can offer. */
        private val RATES = listOf(30, 60)

        /**
         * The highest frame rate the front or back camera lists (CONTROL_AE ranges; the same
         * camera CameraX's default selector picks). Front and back often differ. 30 when it
         * can't be read. High-speed modes don't count: Android only gives those to preview and
         * video-encoder outputs, not to the frames this app reads.
         */
        fun maxFps(
            context: Context,
            front: Boolean,
        ): Int =
            runCatching {
                val manager = context.getSystemService(CameraManager::class.java)
                val facing = if (front) CameraCharacteristics.LENS_FACING_FRONT else CameraCharacteristics.LENS_FACING_BACK
                val id = manager.cameraIdList.first { manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) == facing }
                val ranges = manager.getCameraCharacteristics(id).get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES).orEmpty()
                Log.d("Dotlink", "camera ${if (front) "front" else "back"} ($id) fps ranges: ${ranges.joinToString()}")
                ranges.maxOf { it.upper }
            }.getOrElse {
                Log.w("Dotlink", "camera ${if (front) "front" else "back"}: can't read its frame rates ($it), assuming 30")
                30
            }

        /** The rates of [RATES] this camera can do; at least the lowest, even if it does less. */
        fun supportedFps(
            context: Context,
            front: Boolean,
        ): List<Int> {
            val max = maxFps(context, front)
            return RATES.filter { it <= max }.ifEmpty { RATES.take(1) }
        }

        /** What the camera really sends for the chosen rate. */
        fun effectiveFps(
            context: Context,
            front: Boolean,
            chosen: Int,
        ): Int = minOf(chosen, maxFps(context, front))
    }
}

/** Longest side of the preview bitmaps; the Camera tab's box is smaller than that anyway. */
private const val PREVIEW_SIZE = 960f

/** A JPEG buffer that hands out its bytes with one copy (not two, as toByteArray would). */
private class JpegBuffer(
    size: Int,
) : ByteArrayOutputStream(size) {
    fun bytes(): ByteString = buf.toByteString(0, count)
}

/** Picture rotation in degrees → the CameraX target rotation that produces it. */
private fun surfaceRotation(degrees: Int) =
    when (((degrees % 360) + 360) % 360) {
        90 -> Surface.ROTATION_90
        180 -> Surface.ROTATION_180
        270 -> Surface.ROTATION_270
        else -> Surface.ROTATION_0
    }
