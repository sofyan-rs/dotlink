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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
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
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.theme.Dotline
import kotlinx.coroutines.launch

@Composable
fun PhonesSection(onPairPhone: () -> Unit) {
    val phones by DotlinkApp.phones.list.collectAsStateWithLifecycle()
    var open by remember { mutableStateOf<PhonePeer?>(null) }
    Section("Phones") {
        ListGroup {
            phones.forEachIndexed { i, p ->
                if (i > 0) ListDivider()
                ListRow(
                    title = p.name,
                    subtitle = "Send files or the clipboard",
                    leadingIcon = DotlineIcons.Share,
                    onClick = { open = p },
                    trailing = { Text("›", style = Dotline.type.body, color = Dotline.colors.inkMuted) },
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

/** Home when phones are paired but no PC is. */
@Composable
fun PhonesHomeScreen(
    onPairPhone: () -> Unit,
    onPairPc: () -> Unit,
) {
    TabScreen {
        ScreenTitle(title = "Phones", subline = "No PC paired")
        PhonesSection(onPairPhone = onPairPhone)
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
