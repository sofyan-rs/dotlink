package com.dotlink.link

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Matrix
import android.util.Size
import android.view.Surface
import android.util.Log
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.resolutionselector.AspectRatioStrategy
import androidx.camera.core.resolutionselector.ResolutionSelector
import androidx.camera.core.resolutionselector.ResolutionStrategy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleOwner
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
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
    private var lastPreview = 0L
    private var socket: WebSocket? = null

    @Volatile private var ready = false

    @Volatile private var stopped = false
    private var lastFrame = 0L
    private val jpeg = ByteArrayOutputStream(512 * 1024)

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

    private fun bind(
        p: ProcessCameraProvider,
        w: Int,
        h: Int,
    ) {
        val prefs = link.prefs
        val minInterval = 1000L / prefs.fps
        val selector =
            ResolutionSelector
                .Builder()
                .setAspectRatioStrategy(AspectRatioStrategy.RATIO_16_9_FALLBACK_AUTO_STRATEGY)
                .setResolutionStrategy(
                    ResolutionStrategy(Size(maxOf(w, h), minOf(w, h)), ResolutionStrategy.FALLBACK_RULE_CLOSEST_LOWER_THEN_HIGHER),
                ).build()
        val a =
            ImageAnalysis
                .Builder()
                .setResolutionSelector(selector)
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .setOutputImageFormat(ImageAnalysis.OUTPUT_IMAGE_FORMAT_RGBA_8888)
                .setOutputImageRotationEnabled(true)
                .setTargetRotation(surfaceRotation(link.cameraRotation.value))
                .build()
        a.setAnalyzer(executor) { image ->
            image.use {
                val ws = socket ?: return@use
                val now = System.currentTimeMillis()
                // Skip when too soon or when the network is behind (keeps latency low).
                if (!ready || now - lastFrame < minInterval || ws.queueSize() > 1_500_000) return@use
                lastFrame = now
                val raw = it.toBitmap()
                val bmp =
                    if (link.cameraMirror.value) {
                        val flip = Matrix().apply { preScale(-1f, 1f) }
                        Bitmap.createBitmap(raw, 0, 0, raw.width, raw.height, flip, false).also { raw.recycle() }
                    } else {
                        raw
                    }
                jpeg.reset()
                bmp.compress(Bitmap.CompressFormat.JPEG, 75, jpeg)
                ws.send(jpeg.toByteArray().toByteString())
                if (now - lastLog > 2000) {
                    lastLog = now
                    Log.d("Dotlink", "camera frame ${bmp.width}x${bmp.height} jpeg=${jpeg.size()}B")
                }
                // The preview shows exactly this frame (~12 fps is plenty for framing the shot).
                // Published bitmaps are left to the GC: the UI may still be drawing the last one.
                if (link.cameraPreviewActive.value && now - lastPreview > 80) {
                    lastPreview = now
                    link.cameraFrame.value = bmp
                } else {
                    bmp.recycle()
                }
            }
        }
        analysis = a
        val cam = if (prefs.frontCamera) CameraSelector.DEFAULT_FRONT_CAMERA else CameraSelector.DEFAULT_BACK_CAMERA
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
}

/** Picture rotation in degrees → the CameraX target rotation that produces it. */
private fun surfaceRotation(degrees: Int) =
    when (((degrees % 360) + 360) % 360) {
        90 -> Surface.ROTATION_90
        180 -> Surface.ROTATION_180
        270 -> Surface.ROTATION_270
        else -> Surface.ROTATION_0
    }
