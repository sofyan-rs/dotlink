/**
 * "Dotlink X is available": the release notes, and Update to download and install it.
 */
package com.dotlink.update

import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.ui.dotline.DotlineDialog
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.theme.Dotline
import kotlinx.coroutines.launch

@Composable
fun UpdateDialog() {
    val release by Updater.prompt.collectAsStateWithLifecycle()
    val state by Updater.state.collectAsStateWithLifecycle()
    val shown = release ?: return
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val downloading = state as? UpdateState.Downloading
    DotlineDialog(
        onDismissRequest = { Updater.prompt.value = null },
        title = "Dotlink ${shown.version} is available",
        body =
            buildString {
                append("You have ${Updater.installedVersion(context)}.")
                if (shown.prerelease) append(" This is a pre-release.")
                (state as? UpdateState.Failed)?.let { append("\n\n${it.message}") }
            },
        extraContent =
            shown.notes.takeIf { it.isNotEmpty() }?.let { notes ->
                {
                    ReleaseNotes(notes, Modifier.heightIn(max = 280.dp).verticalScroll(rememberScrollState()))
                }
            },
        confirmButton = {
            PrimaryButton(
                text = downloading?.let { "${(it.progress * 100).toInt()}%" } ?: "Update",
                enabled = downloading == null,
                compact = true,
                onClick = { scope.launch { Updater.update(context, shown) } },
                modifier = Modifier.weight(1f),
            )
        },
        dismissButton = {
            SecondaryButton(text = "Later", compact = true, onClick = { Updater.prompt.value = null }, modifier = Modifier.weight(1f))
        },
    )
}
