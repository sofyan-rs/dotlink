/**
 * Pairing two phones: this one shows its code (a QR code, `dotlink://phone?…`) and the other
 * scans it, or the other way round. Either phone can do either.
 */
package com.dotlink.ui.phones

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.DotlineTextField
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.dotline.SectionLabel
import com.dotlink.ui.pair.QrScanner
import com.dotlink.ui.theme.Dotline
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun PhonePairScreen(onDone: () -> Unit) {
    val phones = DotlinkApp.phones
    val link = DotlinkApp.link
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var scanning by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    val pasted = rememberTextFieldState()
    var code by remember { mutableStateOf<String?>(null) }
    var shownAt by remember { mutableLongStateOf(0L) }
    var left by remember { mutableLongStateOf(300L) }
    val pairedByScan by phones.pairedByScan.collectAsStateWithLifecycle()

    fun newCode() {
        code = phones.invite()
        shownAt = System.currentTimeMillis()
        // The server has to be up while the code is shown, PC or not.
        LinkService.send(context)
    }
    LaunchedEffect(Unit) {
        phones.pairedByScan.value = null
        newCode()
    }
    LaunchedEffect(shownAt) {
        while (true) {
            left = (300 - (System.currentTimeMillis() - shownAt) / 1000).coerceAtLeast(0)
            delay(1000)
        }
    }
    // The other phone scanned this one's code.
    LaunchedEffect(pairedByScan) {
        val name = pairedByScan ?: return@LaunchedEffect
        phones.pairedByScan.value = null
        link.notices.tryEmit("Paired with $name")
        onDone()
    }

    fun pair(uri: String) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            phones
                .pair(uri)
                .onSuccess {
                    phones.cancelInvite()
                    link.notices.tryEmit("Paired with $it")
                    onDone()
                }.onFailure { error = it.message ?: "Pairing failed" }
            busy = false
        }
    }

    val cameraPermission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) scanning = true else error = "Camera permission is needed to scan. Paste the code instead."
        }
    BackHandler {
        if (scanning) {
            scanning = false
        } else {
            phones.cancelInvite()
            onDone()
        }
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
        Spacer(Modifier.height(Dotline.space.s6))
        if (scanning) {
            QrScanner(
                onCode = {
                    scanning = false
                    pair(it)
                },
                modifier = Modifier.fillMaxWidth().aspectRatio(1f),
                hint = "Point at the code on the other phone",
            )
            Spacer(Modifier.height(Dotline.space.s4))
            SecondaryButton(text = "Cancel", onClick = { scanning = false }, modifier = Modifier.fillMaxWidth())
            return@Column
        }
        Text("Pair a phone", style = Dotline.type.title, color = Dotline.colors.ink)
        Spacer(Modifier.height(Dotline.space.s2))
        Text(
            "Two phones send each other files and the clipboard, on the same Wi-Fi. On the other phone, open Dotlink → Pair a phone and scan this code, or scan the code it shows.",
            style = Dotline.type.body,
            color = Dotline.colors.inkMuted,
        )
        Spacer(Modifier.height(Dotline.space.s5))
        SectionLabel("This phone's code", Modifier.padding(bottom = Dotline.space.s2))
        code?.let { QrImage(it, Modifier.fillMaxWidth().aspectRatio(1f)) }
        Spacer(Modifier.height(Dotline.space.s2))
        Text(
            if (left > 0) "Works once, for ${left / 60}:${"%02d".format(left % 60)}" else "Expired",
            style = Dotline.type.bodyS,
            color = Dotline.colors.inkMuted,
        )
        Spacer(Modifier.height(Dotline.space.s3))
        SecondaryButton(text = "New code", icon = DotlineIcons.Refresh, onClick = ::newCode, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(Dotline.space.s5))
        PrimaryButton(
            text = if (busy) "Pairing…" else "Scan the other phone's code",
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
        Spacer(Modifier.height(Dotline.space.s4))
        DotlineTextField(state = pasted, label = "Or paste its code", placeholder = "dotlink://phone?…", errorText = error)
        Spacer(Modifier.height(Dotline.space.s3))
        SecondaryButton(
            text = "Pair",
            enabled = !busy && pasted.text.isNotBlank(),
            onClick = { pair(pasted.text.toString()) },
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(Dotline.space.s2))
        SecondaryButton(
            text = "Cancel",
            onClick = {
                phones.cancelInvite()
                onDone()
            },
            modifier = Modifier.fillMaxWidth(),
        )
        // The code as text too, for pasting (and for tests reading the screen).
        Spacer(Modifier.height(Dotline.space.s4))
        Text(code.orEmpty(), style = Dotline.type.bodyS, color = Dotline.colors.inkFaint)
        Spacer(Modifier.height(Dotline.space.s6))
    }
}

/** [text] as a QR code: black modules on white, with a quiet zone. */
@Composable
fun QrImage(
    text: String,
    modifier: Modifier = Modifier,
) {
    val matrix =
        remember(text) {
            QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, mapOf(EncodeHintType.MARGIN to 2))
        }
    Box(
        modifier
            .clip(Dotline.radius.l)
            .background(Color.White)
            .semantics { contentDescription = "Pairing code" },
    ) {
        Canvas(Modifier.fillMaxSize().padding(Dotline.space.s3)) {
            val cell = size.width / matrix.width
            for (y in 0 until matrix.height) {
                for (x in 0 until matrix.width) {
                    if (matrix[x, y]) drawRect(Color.Black, Offset(x * cell, y * cell), Size(cell + 0.5f, cell + 0.5f))
                }
            }
        }
    }
}
