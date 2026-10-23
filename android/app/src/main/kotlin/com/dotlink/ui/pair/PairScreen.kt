/**
 * Shown while no PC is paired: scan the QR code from `dotlink pair`, or paste its link.
 */
package com.dotlink.ui.pair

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.OptIn
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.dotlink.DotlinkApp
import com.dotlink.ui.dotline.DotArt
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.DotlineTextField
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.dotline.SectionLabel
import com.dotlink.ui.theme.Dotline
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.concurrent.Executors

/** Two linked devices, drawn in dots. */
private val LinkArt =
    listOf(
        "XXXXX.........XXX",
        "X...X.........X.X",
        "X...X..RRRRR..X.X",
        "X...X.........X.X",
        "XXXXX.........XXX",
        "..X...........XXX",
        "XXXXX............",
    )

@Composable
fun PairScreen(
    initialLink: String? = null,
    onPaired: () -> Unit,
) {
    val link = DotlinkApp.link
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var scanning by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val pasted = rememberTextFieldState()
    LaunchedEffect(initialLink) { if (initialLink != null) pasted.setTextAndPlaceCursorAtEnd(initialLink) }

    fun pair(code: String) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            link
                .pair(code)
                .onSuccess {
                    link.notices.tryEmit("Paired with $it")
                    onPaired()
                }.onFailure { error = it.message ?: "Pairing failed" }
            busy = false
        }
    }

    val cameraPermission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) scanning = true else error = "Camera permission is needed to scan. Paste the link instead."
        }

    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.statusBars)
            .windowInsetsPadding(WindowInsets.navigationBars)
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = Dotline.space.s4),
    ) {
        Spacer(Modifier.height(Dotline.space.s7))
        if (scanning) {
            QrScanner(
                onCode = {
                    scanning = false
                    pair(it)
                },
                modifier = Modifier.fillMaxWidth().aspectRatio(1f),
            )
            Spacer(Modifier.height(Dotline.space.s4))
            SecondaryButton(text = "Cancel", onClick = { scanning = false }, modifier = Modifier.fillMaxWidth())
            return@Column
        }
        DotArt(pattern = LinkArt, contentDescription = null, dotSize = 6.dp)
        Spacer(Modifier.height(Dotline.space.s6))
        Text("DOTLINK", style = Dotline.type.dotL, color = Dotline.colors.ink)
        Spacer(Modifier.height(Dotline.space.s2))
        Text(
            "Files, clipboard, remote, shell, webcam and mic between this phone and your Hyprland PC.",
            style = Dotline.type.body,
            color = Dotline.colors.inkMuted,
        )
        Spacer(Modifier.height(Dotline.space.s5))
        SectionLabel("On the PC", Modifier.padding(start = Dotline.space.s4, bottom = Dotline.space.s2))
        ListGroup {
            ListRow(title = "dotlink daemon", subtitle = "Runs in the background (exec-once)", subtitleIsData = false, leadingIcon = DotlineIcons.Terminal)
            ListDivider()
            ListRow(title = "dotlink pair", subtitle = "Shows a QR code for 5 minutes", leadingIcon = DotlineIcons.Scan)
        }
        Spacer(Modifier.height(Dotline.space.s5))
        PrimaryButton(
            text = if (busy) "Pairing…" else "Scan QR code",
            icon = DotlineIcons.Scan,
            enabled = !busy,
            onClick = {
                if (ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
                    scanning = true
                } else {
                    cameraPermission.launch(Manifest.permission.CAMERA)
                }
            },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(Dotline.space.s5))
        DotlineTextField(
            state = pasted,
            label = "Or paste the link",
            placeholder = "dotlink://pair?…",
            errorText = error,
        )
        Spacer(Modifier.height(Dotline.space.s3))
        SecondaryButton(
            text = "Pair",
            enabled = !busy && pasted.text.isNotBlank(),
            onClick = { pair(pasted.text.toString()) },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(Dotline.space.s6))
    }
}

@OptIn(ExperimentalGetImage::class)
@Composable
private fun QrScanner(
    onCode: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val previewView = remember { PreviewView(context) }
    val executor = remember { Executors.newSingleThreadExecutor() }
    var done by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        val provider = withContext(Dispatchers.IO) { ProcessCameraProvider.getInstance(context).get() }
        val preview = Preview.Builder().build().also { it.surfaceProvider = previewView.surfaceProvider }
        val scanner =
            BarcodeScanning.getClient(BarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build())
        val analysis =
            ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build().also {
                it.setAnalyzer(executor) { proxy ->
                    val media = proxy.image
                    if (media == null || done) {
                        proxy.close()
                        return@setAnalyzer
                    }
                    scanner
                        .process(InputImage.fromMediaImage(media, proxy.imageInfo.rotationDegrees))
                        .addOnSuccessListener { codes ->
                            val code = codes.firstNotNullOfOrNull { c -> c.rawValue?.takeIf { v -> v.startsWith("dotlink://") } }
                            if (code != null && !done) {
                                done = true
                                onCode(code)
                            }
                        }.addOnCompleteListener { proxy.close() }
                }
            }
        provider.unbindAll()
        provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
    }
    DisposableEffect(Unit) {
        onDispose {
            ProcessCameraProvider.getInstance(context).get().unbindAll()
            executor.shutdown()
        }
    }
    Box(
        modifier
            .clip(Dotline.radius.l)
            .background(Dotline.colors.surface)
            .border(Dotline.size.stroke, Dotline.colors.lineStrong, Dotline.radius.l),
        contentAlignment = Alignment.Center,
    ) {
        AndroidView(factory = { previewView }, modifier = Modifier.fillMaxSize())
    }
    Spacer(Modifier.height(Dotline.space.s3))
    Text(
        "Point at the code from dotlink pair",
        style = Dotline.type.bodyS,
        color = Dotline.colors.inkMuted,
        modifier = Modifier.fillMaxWidth(),
    )
}
