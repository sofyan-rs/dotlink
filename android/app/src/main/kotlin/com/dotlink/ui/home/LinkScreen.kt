/**
 * The LINK tab: connection status, quick actions, phone-as-webcam/mic and recent transfers.
 */
package com.dotlink.ui.home

import android.Manifest
import android.os.SystemClock
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.dotlink.link.MediaBridge
import com.dotlink.link.PcMedia
import kotlinx.coroutines.delay
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
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
import com.dotlink.ui.dotline.SecondaryButton
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
    // Only while connected, and only a PC with a battery sends it.
    val pcBattery by link.pcBattery.collectAsStateWithLifecycle()
    val pcMedia by link.pcMedia.collectAsStateWithLifecycle()
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
        pcBattery?.let { (level, charging) ->
            val color = if (charging) Dotline.colors.positive else if (level <= 15) Dotline.colors.accentText else Dotline.colors.inkMuted
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
                modifier = Modifier.padding(horizontal = Dotline.space.s4),
            ) {
                DotMeter(count = 10, lit = (level + 9) / 10, contentDescription = "PC battery", litColor = color)
                Text(
                    "$level%" + if (charging) " · CHARGING" else "",
                    style = Dotline.type.data,
                    color = color,
                )
            }
        }
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

        pcMedia?.let { m ->
            Section("Playing on ${link.prefs.serverName}") { PcMediaCard(m) }
        }

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
                ClearTransfersButton(pcTransfers, onClick = { link.clearTransfers(phones = false) })
            }
        }
    }
    if (pcSheet) PcSheet(state = state, onAddPc = onAddPc, onDismissRequest = { pcSheet = false })
}

/** Under a Transfers list: empties it of what has finished. Hidden while only running ones are left. */
@Composable
internal fun ClearTransfersButton(
    transfers: List<Transfer>,
    onClick: () -> Unit,
) {
    if (transfers.all { it.status == Transfer.Status.Running }) return
    SecondaryButton(
        text = "Clear history",
        icon = DotlineIcons.Trash,
        compact = true,
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().padding(top = Dotline.space.s3),
    )
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
        // A received file: tap opens it, long-press asks which app.
        onClick = t.uri?.let { { DotlinkApp.link.openReceived(t, choose = false) } },
        onLongClick = t.uri?.let { { DotlinkApp.link.openReceived(t, choose = true) } },
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

/** The PC's player: artwork, title, the position moving on while it plays, and its controls. */
@Composable
private fun PcMediaCard(m: PcMedia) {
    val colors = Dotline.colors
    // Ticks once a second while it plays, to move the position on.
    var now by remember { mutableLongStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(m) {
        while (m.playing) {
            delay(1_000)
            now = SystemClock.elapsedRealtime()
        }
    }
    val position = (m.positionMs + if (m.playing) now - m.at else 0).coerceIn(0, if (m.durationMs > 0) m.durationMs else Long.MAX_VALUE)
    fun time(ms: Long) = "%d:%02d".format(ms / 60_000, ms / 1000 % 60)
    ListGroup {
        Row(
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3),
            modifier = Modifier.fillMaxWidth().padding(Dotline.space.s3),
        ) {
            val art = m.art
            if (art != null) {
                Image(
                    bitmap = art.asImageBitmap(),
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.size(56.dp).clip(Dotline.radius.s),
                )
            } else {
                Box(Modifier.size(56.dp).clip(Dotline.radius.s).background(colors.surfaceRaised))
            }
            Column(Modifier.weight(1f)) {
                Text(m.title, style = Dotline.type.headline, color = colors.ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (m.artist.isNotEmpty()) Text(m.artist, style = Dotline.type.bodyS, color = colors.inkMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    m.player.uppercase() + if (m.durationMs > 0) " · ${time(position)} / ${time(m.durationMs)}" else "",
                    style = Dotline.type.data,
                    color = colors.inkFaint,
                    maxLines = 1,
                )
            }
        }
        if (m.durationMs > 0) {
            val lit = (position * 20 / m.durationMs).toInt()
            DotMeter(count = 20, lit = lit, contentDescription = "Position", modifier = Modifier.padding(horizontal = Dotline.space.s3))
        }
        Row(
            horizontalArrangement = Arrangement.SpaceEvenly,
            verticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.fillMaxWidth().padding(vertical = Dotline.space.s2),
        ) {
            m.volume?.let { v -> MediaButton("VOL−") { MediaBridge.toPc("volume", v - 10) } }
            MediaButton("⏮") { MediaBridge.toPc("prev") }
            MediaButton(if (m.playing) "⏸" else "▶") { MediaBridge.toPc("toggle") }
            MediaButton("⏭") { MediaBridge.toPc("next") }
            m.volume?.let { v -> MediaButton("VOL+") { MediaBridge.toPc("volume", v + 10) } }
        }
    }
}

@Composable
private fun MediaButton(
    label: String,
    onClick: () -> Unit,
) {
    val haptic = LocalHapticFeedback.current
    Box(
        modifier =
            Modifier
                .size(48.dp)
                .clip(CircleShape)
                .clickable {
                    haptic.performHapticFeedback(HapticFeedbackType.VirtualKey)
                    onClick()
                },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Dotline.type.label, color = Dotline.colors.ink)
    }
}
