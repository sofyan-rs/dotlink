/**
 * The LINK tab: connection status, quick actions, phone-as-webcam/mic and recent transfers.
 */
package com.dotlink.ui.home

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.CameraStreamer
import com.dotlink.link.LinkService
import com.dotlink.link.LinkState
import com.dotlink.link.Transfer
import com.dotlink.ui.Hint
import com.dotlink.ui.Section
import com.dotlink.ui.TabScreen
import com.dotlink.ui.dotline.DotMeter
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.KuSwitch
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.ListRowValue
import com.dotlink.ui.dotline.MainTab
import com.dotlink.ui.dotline.QuickAction
import com.dotlink.ui.dotline.QuickActionRow
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.dotline.ScreenTitleAction
import com.dotlink.ui.formatSize
import com.dotlink.ui.theme.Dotline
import kotlinx.coroutines.launch

@Composable
fun LinkScreen(
    onOpenTab: (MainTab) -> Unit,
    onAddPc: () -> Unit,
) {
    val link = DotlinkApp.link
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val state by link.state.collectAsStateWithLifecycle()
    // Files with the PC; the Phones tab lists those with phones.
    val transfers by link.transfers.collectAsStateWithLifecycle()
    val pcTransfers = transfers.filter { it.phone == null }
    val cameraOn by link.cameraOn.collectAsStateWithLifecycle()
    val micOn by link.micOn.collectAsStateWithLifecycle()
    val online = state is LinkState.Online
    var pcSheet by remember { mutableStateOf(false) }

    val pickFiles =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
            if (uris.isNotEmpty()) scope.launch { link.upload(uris) }
        }
    val cameraPermission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) LinkService.send(context, LinkService.ACTION_CAMERA_ON)
        }
    val micPermission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) LinkService.send(context, LinkService.ACTION_MIC_ON)
        }

    fun needOnline(action: () -> Unit) {
        if (online) action() else link.notices.tryEmit("${link.prefs.serverName} is not connected")
    }

    fun withPermission(
        permission: String,
        launcher: androidx.activity.result.ActivityResultLauncher<String>,
        action: String,
    ) {
        if (ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED) {
            LinkService.send(context, action)
        } else {
            launcher.launch(permission)
        }
    }

    TabScreen {
        ScreenTitle(
            title = link.prefs.serverName,
            onTitleClick = { pcSheet = true },
            titleClickLabel = "Change PC",
            subline =
                when (state) {
                    is LinkState.Online -> if (link.viaUsb) "USB cable" else "${link.host}:${link.prefs.port} · Wi-Fi"
                    LinkState.Connecting -> "Connecting…"
                    LinkState.Disconnected -> "Disconnected · tap ↻ to reconnect"
                    else -> "Offline · retrying"
                },
            status = if (online) "Connected" else null,
            actions = {
                ScreenTitleAction(DotlineIcons.Sync, contentDescription = "Reconnect", onClick = { LinkService.send(context) })
            },
        )
        QuickActionRow(
            actions =
                listOf(
                    QuickAction("Send", DotlineIcons.Upload) { needOnline { pickFiles.launch(arrayOf("*/*")) } },
                    QuickAction("Clipboard", DotlineIcons.Clipboard) {
                        needOnline {
                            val sent = link.sendPhoneClipboard()
                            link.notices.tryEmit(if (sent) "Clipboard sent" else "Clipboard is empty")
                        }
                    },
                    QuickAction("Remote", DotlineIcons.Mouse) { onOpenTab(MainTab.Remote) },
                    QuickAction("Shell", DotlineIcons.Terminal) { onOpenTab(MainTab.Shell) },
                ),
            modifier = Modifier.padding(start = Dotline.space.s4, end = Dotline.space.s4, top = Dotline.space.s5),
        )

        Section("Phone as") {
            val prefs = link.prefs
            ListGroup {
                ListRow(
                    title = "Webcam",
                    subtitle =
                        listOf(
                            if (prefs.fullHd) "1080p" else "720p",
                            if (prefs.frontCamera) "front" else "back",
                            // What is really sent: a camera that tops out at 30 does 30.
                            "${remember(prefs.frontCamera, prefs.fps) { CameraStreamer.effectiveFps(context, prefs.frontCamera, prefs.fps) }} fps",
                            "${prefs.rotation}°" + if (prefs.mirror) " mirrored" else "",
                        ).joinToString(" · "),
                    subtitleIsData = true,
                    leadingIcon = DotlineIcons.Camera,
                    role = Role.Switch,
                    onClick = {
                        if (cameraOn) {
                            LinkService.send(context, LinkService.ACTION_CAMERA_OFF)
                        } else {
                            needOnline { withPermission(Manifest.permission.CAMERA, cameraPermission, LinkService.ACTION_CAMERA_ON) }
                        }
                    },
                    trailing = { KuSwitch(checked = cameraOn, onCheckedChange = null) },
                )
                ListDivider()
                ListRow(
                    title = "Microphone",
                    subtitle = "Dotlink Mic on the PC",
                    subtitleIsData = true,
                    leadingIcon = DotlineIcons.Mic,
                    role = Role.Switch,
                    onClick = {
                        if (micOn) {
                            LinkService.send(context, LinkService.ACTION_MIC_OFF)
                        } else {
                            needOnline { withPermission(Manifest.permission.RECORD_AUDIO, micPermission, LinkService.ACTION_MIC_ON) }
                        }
                    },
                    trailing = { KuSwitch(checked = micOn, onCheckedChange = null) },
                )
            }
            Hint("Pick \"Dotlink Camera\" and \"Dotlink Mic\" in your call, browser or OBS.")
        }

        Section("Transfers") {
            if (pcTransfers.isEmpty()) {
                Hint("Nothing yet. Files from the PC (dotlink send FILE) land in Download/Dotlink.")
            } else {
                ListGroup {
                    pcTransfers.take(8).forEachIndexed { i, t ->
                        if (i > 0) ListDivider()
                        TransferRow(t)
                    }
                }
            }
        }
    }
    if (pcSheet) PcSheet(state = state, onAddPc = onAddPc, onDismissRequest = { pcSheet = false })
}

@Composable
internal fun TransferRow(t: Transfer) {
    val arrow = if (t.incoming) "↓" else "↑"
    val subtitle =
        when (t.status) {
            Transfer.Status.Running -> "$arrow ${formatSize(t.done)} of ${formatSize(t.size)}"
            Transfer.Status.Done ->
                when {
                    t.phone != null -> "$arrow ${if (t.incoming) "from" else "to"} ${t.phone} · ${formatSize(t.size)}"
                    t.incoming -> "$arrow Download/Dotlink · ${formatSize(t.size)}"
                    else -> "$arrow Sent · ${formatSize(t.size)}"
                }
            Transfer.Status.Failed -> "$arrow ${t.error ?: "Failed"}"
        }
    ListRow(
        title = t.name,
        subtitle = subtitle,
        subtitleIsData = true,
        leadingIcon = if (t.incoming) DotlineIcons.Download else DotlineIcons.Upload,
        destructive = t.status == Transfer.Status.Failed,
        trailing = {
            when (t.status) {
                Transfer.Status.Running -> {
                    val lit = if (t.size > 0) (t.done * 8 / t.size).toInt() else 0
                    DotMeter(count = 8, lit = lit, contentDescription = "Progress")
                }
                Transfer.Status.Done -> ListRowValue("Done", color = Dotline.colors.positive)
                Transfer.Status.Failed -> ListRowValue("Failed", color = Dotline.colors.accentText)
            }
        },
    )
}
