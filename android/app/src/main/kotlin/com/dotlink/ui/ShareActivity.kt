/**
 * "Share → Dotlink": files go to the PC's download folder, text goes to the PC clipboard.
 * It stays open as a small sheet until the upload is done, because Android only lends us
 * read access to shared files while this activity lives.
 */
package com.dotlink.ui

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.core.content.IntentCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.link.LinkState
import com.dotlink.link.Transfer
import com.dotlink.ui.dotline.DotMeter
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.ListRowValue
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.dotline.SheetDotHandle
import com.dotlink.ui.dotline.SheetHeader
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

class ShareActivity : ComponentActivity() {
    private val link get() = DotlinkApp.link
    private var status by mutableStateOf("Connecting…")

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (!link.prefs.paired) {
            Toast.makeText(this, "Pair Dotlink with your PC first", Toast.LENGTH_LONG).show()
            finish()
            return
        }
        val uris = sharedUris(intent)
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
        LinkService.send(this)
        // Transfers created from here on belong to this share.
        val firstId = (link.transfers.value.maxOfOrNull { it.id } ?: 0) + 1
        link.scope.launch {
            val online = withTimeoutOrNull(8_000) { link.state.first { it is LinkState.Online } } != null
            if (!online) {
                toastAndFinish("${link.prefs.serverName} is not reachable")
                return@launch
            }
            if (uris.isEmpty()) {
                val sent = text != null && link.sendClip(text)
                toastAndFinish(if (sent) "Copied to ${link.prefs.serverName}" else "Nothing to send")
                return@launch
            }
            status = "to ${link.prefs.serverName}"
            val ok = link.upload(uris)
            toastAndFinish(if (ok == uris.size) "Sent to ${link.prefs.serverName}" else {
                val error = link.transfers.value.firstOrNull { it.id >= firstId && it.error != null }?.error
                "${uris.size - ok} file(s) failed: $error"
            })
        }
        if (uris.isEmpty()) return // text: no sheet, just the toast
        setContent {
            DotlinkTheme(dark = isAppInDarkTheme()) {
                val transfers by link.transfers.collectAsStateWithLifecycle()
                val mine = transfers.filter { !it.incoming && it.id >= firstId }.reversed()
                Box(
                    Modifier
                        .fillMaxSize()
                        .background(Dotline.colors.scrim)
                        .clickable(MutableInteractionSource(), indication = null) { finish() },
                    contentAlignment = Alignment.BottomCenter,
                ) {
                    Column(
                        Modifier
                            .fillMaxWidth()
                            .clip(Dotline.radius.sheet)
                            .background(Dotline.colors.surface)
                            .clickable(MutableInteractionSource(), indication = null) {}
                            .windowInsetsPadding(WindowInsets.navigationBars)
                            .padding(horizontal = Dotline.space.s4),
                    ) {
                        SheetDotHandle(Modifier.align(Alignment.CenterHorizontally))
                        Spacer(Modifier.height(Dotline.space.s3))
                        SheetHeader(
                            icon = DotlineIcons.Upload,
                            title = if (uris.size == 1) "Sending 1 file" else "Sending ${uris.size} files",
                            subtitle = status,
                        )
                        Spacer(Modifier.height(Dotline.space.s4))
                        if (mine.isNotEmpty()) {
                            ListGroup(inSheet = true) {
                                mine.forEachIndexed { i, t ->
                                    if (i > 0) ListDivider()
                                    ListRow(
                                        title = t.name,
                                        subtitle = formatSize(t.size),
                                        subtitleIsData = true,
                                        inSheet = true,
                                        trailing = {
                                            when (t.status) {
                                                Transfer.Status.Running ->
                                                    DotMeter(8, if (t.size > 0) (t.done * 8 / t.size).toInt() else 0, "Progress")
                                                Transfer.Status.Done -> ListRowValue("Done", color = Dotline.colors.positive)
                                                Transfer.Status.Failed -> ListRowValue("Failed", color = Dotline.colors.accentText)
                                            }
                                        },
                                    )
                                }
                            }
                            Spacer(Modifier.height(Dotline.space.s4))
                        }
                        SecondaryButton(text = "Hide", compact = true, onClick = { finish() }, modifier = Modifier.fillMaxWidth())
                        Spacer(Modifier.height(Dotline.space.s4))
                    }
                }
            }
        }
    }

    private fun toastAndFinish(message: String) {
        runOnUiThread {
            Toast.makeText(this, message, Toast.LENGTH_SHORT).show()
            finish()
        }
    }

    private fun sharedUris(intent: Intent): List<Uri> =
        when (intent.action) {
            Intent.ACTION_SEND ->
                listOfNotNull(IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java))
            Intent.ACTION_SEND_MULTIPLE ->
                IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java).orEmpty()
            else -> emptyList()
        }
}
