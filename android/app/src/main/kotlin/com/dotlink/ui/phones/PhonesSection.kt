/**
 * The phones paired with this one: send files or the clipboard to one, or forget it. On the
 * Link tab, and on its own screen when no PC is paired.
 */
package com.dotlink.ui.phones

import android.content.ClipboardManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import com.dotlink.DotlinkApp
import com.dotlink.link.PhonePeer
import com.dotlink.ui.Hint
import com.dotlink.ui.Section
import com.dotlink.ui.TabScreen
import com.dotlink.ui.dotline.DestructiveButton
import com.dotlink.ui.dotline.DotlineBottomSheet
import com.dotlink.ui.dotline.DotlineDialog
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.ListRowValue
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.home.TransferRow
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.theme.Dotline
import kotlinx.coroutines.launch

@Composable
fun PhonesSection(onPairPhone: () -> Unit) {
    val phones by DotlinkApp.phones.list.collectAsStateWithLifecycle()
    val online by DotlinkApp.phones.online.collectAsStateWithLifecycle()
    var open by remember { mutableStateOf<PhonePeer?>(null) }
    // Looking for them only while the list is on screen (and the app in front).
    val owner = LocalLifecycleOwner.current
    var looking by remember { mutableStateOf(true) }
    LaunchedEffect(owner, phones.isEmpty()) {
        if (phones.isEmpty()) return@LaunchedEffect
        owner.repeatOnLifecycle(Lifecycle.State.STARTED) { DotlinkApp.phones.watch() }
    }
    LaunchedEffect(Unit) {
        delay(5000)
        looking = false
    }
    Section("Paired") {
        ListGroup {
            phones.forEachIndexed { i, p ->
                if (i > 0) ListDivider()
                val here = p.id in online
                ListRow(
                    title = p.name,
                    subtitle =
                        when {
                            here -> "Send files or the clipboard"
                            looking -> "Looking…"
                            else -> "Not on this Wi-Fi · open Dotlink on it"
                        },

                    leadingIcon = DotlineIcons.Smartphone,
                    onClick = { open = p },
                    trailing = {
                        if (here) ListRowValue("Online", color = Dotline.colors.positive) else Text("›", style = Dotline.type.body, color = Dotline.colors.inkMuted)
                    },
                )
            }
            if (phones.isNotEmpty()) ListDivider()
            ListRow(title = "Pair a phone", leadingIcon = DotlineIcons.Plus, onClick = onPairPhone)
        }
        if (phones.isEmpty()) Hint("Another phone with Dotlink, on the same Wi-Fi: files and the clipboard both ways.")
    }
    open?.let { PhoneSheet(it, onDismissRequest = { open = null }) }
}

/** What to do with one paired phone. */
@Composable
private fun PhoneSheet(
    phone: PhonePeer,
    onDismissRequest: () -> Unit,
) {
    val phones = DotlinkApp.phones
    val link = DotlinkApp.link
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var forgetting by remember { mutableStateOf(false) }
    val pickFiles =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
            if (uris.isNotEmpty()) {
                onDismissRequest()
                scope.launch {
                    val ok = phones.sendFiles(phone.id, uris)
                    if (ok < uris.size) link.notices.tryEmit("${uris.size - ok} file(s) didn't reach ${phone.name}")
                }
            }
        }
    DotlineBottomSheet(onDismissRequest = onDismissRequest) {
        Text(phone.name, style = Dotline.type.title, color = Dotline.colors.ink, modifier = Modifier.padding(start = Dotline.space.s1))
        PrimaryButton(
            text = "Send files",
            icon = DotlineIcons.Upload,
            onClick = { pickFiles.launch(arrayOf("*/*")) },
            modifier = Modifier.fillMaxWidth(),
        )
        SecondaryButton(
            text = "Send clipboard",
            icon = DotlineIcons.Clipboard,
            onClick = {
                // Only the app on screen may read the clipboard: read it now, send it after.
                val text =
                    context
                        .getSystemService(ClipboardManager::class.java)
                        .primaryClip
                        ?.getItemAt(0)
                        ?.coerceToText(context)
                        ?.toString()
                onDismissRequest()
                if (text.isNullOrEmpty()) {
                    link.notices.tryEmit("Clipboard is empty")
                    return@SecondaryButton
                }
                scope.launch {
                    phones
                        .sendText(phone.id, text)
                        .onSuccess { link.notices.tryEmit("Clipboard sent to ${phone.name}") }
                        .onFailure { link.notices.tryEmit(it.message ?: "Couldn't send the clipboard") }
                }
            },
            modifier = Modifier.fillMaxWidth(),
        )
        SecondaryButton(text = "Forget", icon = DotlineIcons.Trash, onClick = { forgetting = true }, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(Dotline.space.s2))
    }
    if (forgetting) {
        DotlineDialog(
            onDismissRequest = { forgetting = false },
            title = "Forget ${phone.name}?",
            body = "The two phones stop sending to each other. Pair again to link them.",
            confirmButton = {
                DestructiveButton(
                    text = "Forget",
                    compact = true,
                    onClick = {
                        forgetting = false
                        onDismissRequest()
                        scope.launch { phones.forget(phone.id) }
                    },
                    modifier = Modifier.weight(1f),
                )
            },
            dismissButton = {
                SecondaryButton(text = "Cancel", compact = true, onClick = { forgetting = false }, modifier = Modifier.weight(1f))
            },
        )
    }
}

/**
 * The Phones tab: paired phones (with whether they're on this network), files sent between
 * phones, and pairing. Without a PC it is the app's home, with a way to pair one.
 */
@Composable
fun PhonesScreen(
    onPairPhone: () -> Unit,
    /** Set when no PC is paired. */
    onPairPc: (() -> Unit)? = null,
) {
    val transfers by DotlinkApp.link.transfers.collectAsStateWithLifecycle()
    val phoneTransfers = transfers.filter { it.phone != null }
    val phones by DotlinkApp.phones.list.collectAsStateWithLifecycle()
    val online by DotlinkApp.phones.online.collectAsStateWithLifecycle()
    TabScreen {
        ScreenTitle(
            title = "Phones",
            subline =
                when {
                    phones.isEmpty() -> "None paired yet"
                    else -> "${phones.size} paired · ${online.size} on this Wi-Fi"
                },
        )
        PhonesSection(onPairPhone = onPairPhone)
        Section("Transfers") {
            if (phoneTransfers.isEmpty()) {
                Hint("Nothing yet. Files from phones land in Download/Dotlink.")
            } else {
                ListGroup {
                    phoneTransfers.take(8).forEachIndexed { i, t ->
                        if (i > 0) ListDivider()
                        TransferRow(t)
                    }
                }
            }
        }
        if (onPairPc != null) {
            Section("PC") {
                ListGroup {
                    ListRow(
                        title = "Pair a PC",
                        subtitle = "Remote, Shell, webcam, mic and more with Dotlink on a PC",
                        leadingIcon = DotlineIcons.Monitor,
                        onClick = onPairPc,
                    )
                }
            }
        }
    }
}
