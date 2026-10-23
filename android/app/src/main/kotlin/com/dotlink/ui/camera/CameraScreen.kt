/**
 * The CAMERA tab: the phone as the PC's webcam ("Dotlink Camera"), with a live preview,
 * quick controls (flip, torch, zoom) and the video settings.
 */
package com.dotlink.ui.camera

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.link.LinkState
import com.dotlink.ui.Choice
import com.dotlink.ui.Hint
import com.dotlink.ui.Section
import com.dotlink.ui.TabScreen
import com.dotlink.ui.dotline.DestructiveButton
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.QuickAction
import com.dotlink.ui.dotline.QuickActionRow
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.theme.Dotline

/** Zoom steps offered by the ZOOM tile; it cycles through them. */
private val ZOOM_STEPS = listOf(1f, 2f, 4f)

@Composable
fun CameraScreen() {
    val link = DotlinkApp.link
    val prefs = link.prefs
    val context = LocalContext.current
    val state by link.state.collectAsStateWithLifecycle()
    val cameraOn by link.cameraOn.collectAsStateWithLifecycle()
    val torch by link.cameraTorch.collectAsStateWithLifecycle()
    val zoom by link.cameraZoom.collectAsStateWithLifecycle()
    var front by remember { mutableStateOf(prefs.frontCamera) }
    var summary by remember { mutableStateOf(summary(prefs.fullHd, prefs.fps, prefs.landscape)) }

    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) LinkService.send(context, LinkService.ACTION_CAMERA_ON)
        }

    fun start() {
        when {
            state !is LinkState.Online -> link.notices.tryEmit("${prefs.serverName} is not connected")
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED ->
                LinkService.send(context, LinkService.ACTION_CAMERA_ON)
            else -> permission.launch(Manifest.permission.CAMERA)
        }
    }

    /** A running stream restarts so the PC gets the new camera or size. */
    fun changed() {
        summary = summary(prefs.fullHd, prefs.fps, prefs.landscape)
        if (link.cameraOn.value) LinkService.send(context, LinkService.ACTION_CAMERA_RESTART)
    }

    // The streamer (in LinkService) draws its preview here while this tab is on screen.
    val previewView =
        remember {
            PreviewView(context).apply {
                implementationMode = PreviewView.ImplementationMode.COMPATIBLE
                scaleType = PreviewView.ScaleType.FIT_CENTER
            }
        }
    DisposableEffect(Unit) {
        link.cameraPreview.value = previewView.surfaceProvider
        onDispose { link.cameraPreview.value = null }
    }

    TabScreen {
        ScreenTitle(
            title = "Camera",
            subline = if (cameraOn) "Dotlink Camera · $summary" else "Off · $summary",
            status = if (cameraOn) "Live" else null,
        )
        Spacer(Modifier.height(Dotline.space.s4))
        Box(
            Modifier
                .padding(horizontal = Dotline.space.s4)
                .fillMaxWidth()
                .height(420.dp)
                .clip(Dotline.radius.l)
                .background(Dotline.colors.surface),
            contentAlignment = Alignment.Center,
        ) {
            if (cameraOn) {
                AndroidView(factory = { previewView }, modifier = Modifier.fillMaxSize())
            } else {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
                    Icon(DotlineIcons.Camera, contentDescription = null, tint = Dotline.colors.inkFaint, modifier = Modifier.size(32.dp))
                    Text("WEBCAM IS OFF", style = Dotline.type.label, color = Dotline.colors.inkFaint)
                }
            }
        }
        Spacer(Modifier.height(Dotline.space.s4))
        Column(Modifier.padding(horizontal = Dotline.space.s4)) {
            if (cameraOn) {
                DestructiveButton(
                    text = "Stop webcam",
                    onClick = { LinkService.send(context, LinkService.ACTION_CAMERA_OFF) },
                    modifier = Modifier.fillMaxWidth(),
                )
            } else {
                PrimaryButton(text = "Start webcam", icon = DotlineIcons.Camera, onClick = ::start, modifier = Modifier.fillMaxWidth())
            }
        }
        QuickActionRow(
            actions =
                listOf(
                    QuickAction(if (front) "Front" else "Back", DotlineIcons.SwitchCamera) {
                        front = !front
                        prefs.frontCamera = front
                        changed()
                    },
                    QuickAction(if (torch) "Torch on" else "Torch", DotlineIcons.Flashlight) {
                        if (!cameraOn) {
                            link.notices.tryEmit("Start the webcam first")
                        } else if (front) {
                            link.notices.tryEmit("The front camera has no torch")
                        } else {
                            link.cameraTorch.value = !torch
                        }
                    },
                    QuickAction("Zoom ${formatZoom(zoom)}", DotlineIcons.ZoomIn) {
                        val next = ZOOM_STEPS[(ZOOM_STEPS.indexOf(zoom).coerceAtLeast(0) + 1) % ZOOM_STEPS.size]
                        link.cameraZoom.value = next
                    },
                ),
            modifier = Modifier.padding(start = Dotline.space.s4, end = Dotline.space.s4, top = Dotline.space.s3),
        )

        Section("Video") {
            Choice(listOf("720p", "1080p"), if (prefs.fullHd) 1 else 0) {
                prefs.fullHd = it == 1
                changed()
            }
            Spacer(Modifier.height(Dotline.space.s2))
            Choice(listOf("15 fps", "30 fps"), if (prefs.fps == 15) 0 else 1) {
                prefs.fps = if (it == 0) 15 else 30
                changed()
            }
            Spacer(Modifier.height(Dotline.space.s2))
            Choice(listOf("Landscape", "Portrait"), if (prefs.landscape) 0 else 1) {
                prefs.landscape = it == 0
                changed()
            }
            Hint("Landscape: hold the phone sideways, top to the left. Pick \"Dotlink Camera\" in your call, browser or OBS.")
        }
    }
}

private fun summary(
    fullHd: Boolean,
    fps: Int,
    landscape: Boolean,
): String {
    val (long, short) = if (fullHd) 1920 to 1080 else 1280 to 720
    val (w, h) = if (landscape) long to short else short to long
    return "${w}×$h · $fps fps"
}

private fun formatZoom(z: Float) = if (z % 1f == 0f) "${z.toInt()}×" else "%.1f×".format(z)
