/**
 * "Share → Dotlink": files go to the PC's download folder (or a paired phone's Download/Dotlink),
 * text goes to its clipboard. With phones paired, the sheet first asks where.
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
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
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
import com.dotlink.ui.dotline.DotlineBottomSheet
import com.dotlink.ui.dotline.SheetHeader
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

class ShareActivity : ComponentActivity() {
    private val link get() = DotlinkApp.link
    private var status by mutableStateOf("Connecting…")

    /** Several places to send to (the PC, paired phones): the sheet asks which first. */
    private var choosing by mutableStateOf(false)

    /** Transfers created from here on belong to this share. */
    private var firstId = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val phones = DotlinkApp.phones.list.value
        if (!link.prefs.paired && phones.isEmpty()) {
            Toast.makeText(this, "Pair Dotlink with your PC or another phone first", Toast.LENGTH_LONG).show()
            finish()
            return
        }
        val uris = sharedUris(intent)
        val text = intent.getStringExtra(Intent.EXTRA_TEXT)
        firstId = (link.transfers.value.maxOfOrNull { it.id } ?: 0) + 1
        if (phones.isEmpty()) {
            sendToPc(uris, text)
            if (uris.isEmpty()) return // text: no sheet, just the toast
        } else {
            choosing = true
        }
        setContent {
            DotlinkTheme(dark = isAppInDarkTheme()) {
                val transfers by link.transfers.collectAsStateWithLifecycle()
                val mine = transfers.filter { !it.incoming && it.id >= firstId }.reversed()
                // The same sheet as everywhere else in the app: its handle, drag to dismiss, scrim.
                DotlineBottomSheet(onDismissRequest = { finish() }) {
                    val what = if (uris.isEmpty()) "the text" else if (uris.size == 1) "1 file" else "${uris.size} files"
                    if (choosing) {
                        SheetHeader(icon = DotlineIcons.Upload, title = "Send $what to…", subtitle = null)
                        ListGroup(inSheet = true) {
                            if (link.prefs.paired) {
                                ListRow(
                                    title = link.prefs.serverName,
                                    subtitle = if (link.state.value is LinkState.Online) "PC · Connected" else "PC",
                                    leadingIcon = DotlineIcons.Monitor,
                                    inSheet = true,
                                    onClick = {
                                        choosing = false
                                        sendToPc(uris, text)
                                    },
                                )
                            }
                            phones.forEachIndexed { i, phone ->
                                if (i > 0 || link.prefs.paired) ListDivider()
                                ListRow(
                                    title = phone.name,
                                    subtitle = "Phone",
                                    leadingIcon = DotlineIcons.Smartphone,
                                    inSheet = true,
                                    onClick = {
                                        choosing = false
                                        sendToPhone(phone.id, phone.name, uris, text)
                                    },
                                )
                            }
                        }
                    } else {
                        SheetHeader(
                            icon = DotlineIcons.Upload,
                            title = if (uris.isEmpty()) "Sending the text" else if (uris.size == 1) "Sending 1 file" else "Sending ${uris.size} files",
                            subtitle = status,
                        )
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
                        }
                    }
                    SecondaryButton(text = if (choosing) "Cancel" else "Hide", compact = true, onClick = { finish() }, modifier = Modifier.fillMaxWidth())
                }
            }
        }
    }

    private fun sendToPc(
        uris: List<Uri>,
        text: String?,
    ) {
        LinkService.send(this)
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
            toastAndFinish(if (ok == uris.size) "Sent to ${link.prefs.serverName}" else failed(uris.size - ok))
        }
    }

    private fun sendToPhone(
        id: String,
        name: String,
        uris: List<Uri>,
        text: String?,
    ) {
        val phones = DotlinkApp.phones
        status = "to $name"
        link.scope.launch {
            if (uris.isEmpty()) {
                if (text == null) return@launch toastAndFinish("Nothing to send")
                phones
                    .sendText(id, text)
                    .onSuccess { toastAndFinish("Copied to $name") }
                    .onFailure { toastAndFinish(it.message ?: "Couldn't reach $name") }
                return@launch
            }
            val ok = phones.sendFiles(id, uris)
            toastAndFinish(if (ok == uris.size) "Sent to $name" else failed(uris.size - ok))
        }
    }

    private fun failed(count: Int): String {
        val error = link.transfers.value.firstOrNull { it.id >= firstId && it.error != null }?.error
        return "$count file(s) failed: $error"
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
