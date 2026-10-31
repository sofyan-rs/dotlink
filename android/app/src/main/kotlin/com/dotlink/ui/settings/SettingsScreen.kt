/**
 * The SETTINGS tab: clipboard, where received files go, remote and shell options, the paired
 * PC, theme and about.
 */
package com.dotlink.ui.settings

import android.content.Intent
import android.net.Uri
import com.dotlink.link.CallWatcher
import com.dotlink.link.FileBrowser
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.compose.rememberLauncherForActivityResult
import android.os.Build
import android.Manifest
import com.dotlink.link.NotifListener
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.heightIn
import android.provider.Settings
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.drop
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.link.LinkState
import com.dotlink.ui.Hint
import com.dotlink.ui.Section
import com.dotlink.ui.Choice
import com.dotlink.ui.TabScreen
import com.dotlink.ui.dotline.DestructiveButton
import com.dotlink.ui.dotline.DotlineBottomSheet
import com.dotlink.ui.dotline.DotlineDialog
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.DotlinePasswordField
import com.dotlink.ui.dotline.KuSwitch
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.ListRowChevron
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.dotline.SegmentOption
import com.dotlink.ui.dotline.SegmentedControl
import com.dotlink.ui.dotline.SheetHeader
import com.dotlink.ui.dotline.DotlineTextField
import com.dotlink.ui.theme.Dotline
import com.dotlink.update.UpdateState
import com.dotlink.update.Updater
import kotlinx.coroutines.launch

@OptIn(kotlinx.coroutines.FlowPreview::class)
@Composable
fun SettingsScreen() {
    val link = DotlinkApp.link
    val prefs = link.prefs
    val context = LocalContext.current
    val state by link.state.collectAsStateWithLifecycle()
    // Prefs aren't observable: read the active PC again when the link state changes or a PC is
    // forgotten, so its sections go the moment it does.
    var forgotten by remember { mutableIntStateOf(0) }
    val pcId = remember(state, forgotten) { prefs.active?.id }
    // Remote, Shell, the PC clipboard and the paired PC only mean something with a PC.
    val pc = pcId != null

    TabScreen {
        ScreenTitle(title = "Settings")

        Section("This phone") {
            val name = rememberTextFieldState(prefs.phoneName)
            LaunchedEffect(name) {
                // Saved as you type; the PC hears about it once you stop.
                snapshotFlow { name.text.toString() }.drop(1).debounce(600).collect { link.renamePhone(it) }
            }
            DotlineTextField(
                state = name,
                label = "Name",
                placeholder = link.systemDeviceName(),
                helperText = "What your PCs and phones call this phone.",
            )
        }

        if (pc) {
            Section("Clipboard") {
                var auto by remember { mutableStateOf(prefs.autoClipboard) }
                ListGroup {
                    ListRow(
                        title = "Send when I open Dotlink",
                        subtitle = "Android only lets apps read the clipboard while open",
                        leadingIcon = DotlineIcons.Clipboard,
                        role = Role.Switch,
                        onClick = {
                            auto = !auto
                            prefs.autoClipboard = auto
                        },
                        trailing = { KuSwitch(checked = auto, onCheckedChange = null) },
                    )
                }
                Hint("PC → phone is automatic. Phone → PC: open Dotlink, use Share → Dotlink, or the notification button.")
            }

        }
        if (pc) {
            Section("Notifications") {
                // Notification access is granted in the system's settings: read again on return.
                var access by remember { mutableStateOf(NotifListener.enabled(context)) }
                LifecycleResumeEffect(Unit) {
                    access = NotifListener.enabled(context)
                    onPauseOrDispose {}
                }
                var on by remember { mutableStateOf(prefs.notificationsToPc) }
                var apps by remember { mutableStateOf(false) }
                ListGroup {
                    ListRow(
                        title = "Show notifications on PC",
                        subtitle = if (on && !access) "Needs Notification access: tap to allow Dotlink" else "Codes in them are copied on the PC",
                        leadingIcon = DotlineIcons.Monitor,
                        role = Role.Switch,
                        onClick = {
                            if (on && !access) {
                                context.startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
                                return@ListRow
                            }
                            on = !on
                            prefs.notificationsToPc = on
                            if (on && !access) context.startActivity(Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
                        },
                        trailing = { KuSwitch(checked = on && access, onCheckedChange = null) },
                    )
                    if (on) {
                        ListDivider()
                        ListRow(
                            title = "Apps",
                            subtitle = "Chat apps start off: they'd repeat every message",
                            leadingIcon = DotlineIcons.Sliders,
                            onClick = { apps = true },
                            trailing = { ListRowChevron() },
                        )
                    }
                }
                Hint("Dismissing one on the PC dismisses it here. Ongoing ones (music, downloads) stay on the phone.")
                if (apps) NotificationAppsSheet(onDismissRequest = { apps = false })
            }
        }
        if (pc) {
            Section("Calls") {
                var on by remember { mutableStateOf(prefs.pauseOnCalls && CallWatcher.allowed(context)) }
                val phonePermission =
                    rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
                        on = granted
                        prefs.pauseOnCalls = granted
                        CallWatcher.update(context)
                    }
                ListGroup {
                    ListRow(
                        title = "Pause PC media during calls",
                        subtitle = "Resumes when the call ends, if it was playing",
                        leadingIcon = DotlineIcons.Smartphone,
                        role = Role.Switch,
                        onClick = {
                            if (!on && !CallWatcher.allowed(context)) {
                                phonePermission.launch(Manifest.permission.READ_PHONE_STATE)
                                return@ListRow
                            }
                            on = !on
                            prefs.pauseOnCalls = on
                            CallWatcher.update(context)
                        },
                        trailing = { KuSwitch(checked = on, onCheckedChange = null) },
                    )
                }
                Hint("Android asks to let Dotlink see the call state; nothing about the call itself is sent.")
            }
        }
        if (pc) {
            Section("Browse from PC") {
                // All files access is granted in the system's settings: read again on return.
                var access by remember { mutableStateOf(FileBrowser.hasAccess(context)) }
                LifecycleResumeEffect(Unit) {
                    access = FileBrowser.hasAccess(context)
                    onPauseOrDispose {}
                }
                var on by remember { mutableStateOf(prefs.browseFromPc) }
                val storagePermission =
                    rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { access = FileBrowser.hasAccess(context) }
                fun askAccess() {
                    if (Build.VERSION.SDK_INT >= 30) {
                        context.startActivity(
                            Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:${context.packageName}")),
                        )
                    } else {
                        storagePermission.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
                    }
                }
                ListGroup {
                    ListRow(
                        title = "Let ${prefs.serverName} browse files",
                        subtitle = if (on && !access) "Needs All files access: tap to allow Dotlink" else "List, download, upload, rename and delete",
                        leadingIcon = DotlineIcons.Folder,
                        role = Role.Switch,
                        onClick = {
                            if (on && !access) {
                                askAccess()
                                return@ListRow
                            }
                            on = !on
                            prefs.browseFromPc = on
                            if (on && !access) askAccess()
                        },
                        trailing = { KuSwitch(checked = on && access, onCheckedChange = null) },
                    )
                }
                Hint("The whole internal storage, except Android/data and Android/obb. Only your paired PCs can ask, not linked phones.")
            }
        }
        Section("Received files") {
            var ask by remember { mutableStateOf(prefs.askBeforeReceiving) }
            ListGroup {
                ListRow(
                    title = "Saved to",
                    // Fixed: Link.download writes through MediaStore into Downloads/Dotlink.
                    subtitle = "Download/Dotlink",
                    subtitleIsData = true,
                    leadingIcon = DotlineIcons.Folder,
                )
                ListDivider()
                ListRow(
                    title = "Ask before receiving",
                    subtitle = "Each file waits for Accept in a notification; 2 minutes without an answer declines it",
                    leadingIcon = DotlineIcons.Download,
                    role = Role.Switch,
                    onClick = {
                        ask = !ask
                        prefs.askBeforeReceiving = ask
                    },
                    trailing = { KuSwitch(checked = ask, onCheckedChange = null) },
                )
            }
            Hint("Files from the PC and from phones always go here; open it in your Files app. This can't be changed yet.")
        }

        if (pc) {
            Section("Remote") {
                val speeds = listOf(1.0f, 1.4f, 2.0f)
                Choice(listOf("Slow", "Normal", "Fast"), speeds.indexOfFirst { it >= prefs.pointerSpeed }.coerceAtLeast(0)) {
                    prefs.pointerSpeed = speeds[it]
                }
            }

        }
        if (pc) {
            Section("Shell") {
                val sizes = listOf(11, 13, 15)
                Choice(listOf("Small", "Medium", "Large"), sizes.indexOf(prefs.terminalFontSize).coerceAtLeast(0)) {
                    prefs.terminalFontSize = sizes[it]
                }
            }

        }
        if (pc) {
            Section("Paired PC") {
                var confirm by remember { mutableStateOf(false) }
                ListGroup {
                    ListRow(
                        title = prefs.serverName,
                        subtitle = (prefs.fingerprint ?: "").chunked(4).take(8).joinToString(" "),
                        subtitleIsData = true,
                        leadingIcon = DotlineIcons.Monitor,
                    )
                    ListDivider()
                    ListRow(
                        title = "Addresses",
                        subtitle = (listOfNotNull(prefs.lastHost) + prefs.hosts).distinct().joinToString(", ") + " · port ${prefs.port}",
                        subtitleIsData = true,
                        leadingIcon = DotlineIcons.Link,
                    )
                }
                Spacer(Modifier.height(Dotline.space.s3))
                // Each PC has its own PIN: a fresh field (and save target) per PC, so switching PCs
                // never writes one PC's PIN into another.
                key(pcId) {
                    val pin = rememberTextFieldState(prefs.terminalPin)
                    var show by remember { mutableStateOf(false) }
                    LaunchedEffect(pin) {
                        snapshotFlow { pin.text.toString() }.drop(1).collect { if (pcId != null) prefs.setTerminalPin(pcId, it) }
                    }
                    DotlinePasswordField(
                        state = pin,
                        label = "Terminal PIN",
                        placeholder = "No PIN",
                        passwordVisible = show,
                        onPasswordVisibleChange = { show = it },
                        helperText = "Each PC has its own. Only needed if ${prefs.serverName}'s Dotlink sets a terminal PIN.",
                    )
                }
                Spacer(Modifier.height(Dotline.space.s4))
                Column(Modifier.padding(horizontal = Dotline.space.s1)) {
                    val disconnected = state is LinkState.Disconnected
                    SecondaryButton(
                        text = if (disconnected) "Reconnect" else "Disconnect for now",
                        onClick = { LinkService.send(context, if (disconnected) null else LinkService.ACTION_DISCONNECT) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Spacer(Modifier.height(Dotline.space.s2))
                    DestructiveButton(text = "Forget this PC", onClick = { confirm = true }, modifier = Modifier.fillMaxWidth())
                }
                if (confirm) {
                    DotlineDialog(
                        onDismissRequest = { confirm = false },
                        title = "Forget ${prefs.serverName}?",
                        body = "This phone stops connecting to it. To link again, pair from the PC's Dotlink app.",
                        confirmButton = {
                            DestructiveButton(
                                text = "Forget",
                                compact = true,
                                onClick = {
                                    confirm = false
                                    pcId?.let { link.forget(it) }
                                    forgotten++
                                },
                                modifier = Modifier.weight(1f),
                            )
                        },
                        dismissButton = {
                            SecondaryButton(text = "Cancel", compact = true, onClick = { confirm = false }, modifier = Modifier.weight(1f))
                        },
                    )
                }
            }

        }
        Section("Theme") {
            Choice(listOf("System", "Light", "Dark"), prefs.themeMode) {
                prefs.themeMode = it
                link.themeMode.value = it
            }
        }

        Section("About") {
            var about by remember { mutableStateOf(false) }
            val version = remember { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            val update by Updater.state.collectAsStateWithLifecycle()
            val scope = rememberCoroutineScope()
            ListGroup {
                ListRow(
                    title = "Check for updates",
                    subtitle =
                        when (val u = update) {
                            UpdateState.Idle -> null
                            UpdateState.Checking -> "Checking…"
                            UpdateState.UpToDate -> "Dotlink is up to date"
                            is UpdateState.Available -> "Dotlink ${u.release.version} is available"
                            is UpdateState.Downloading -> "Downloading ${u.release.version}: ${(u.progress * 100).toInt()}%"
                            is UpdateState.Failed -> u.message
                        },
                    leadingIcon = DotlineIcons.Refresh,
                    onClick = {
                        when (val u = update) {
                            is UpdateState.Available -> Updater.prompt.value = u.release
                            else -> scope.launch { Updater.check(context, prompt = true) }
                        }
                    },
                    trailing = { ListRowChevron() },
                )
                ListDivider()
                ListRow(
                    title = "Report a bug or suggestion",
                    leadingIcon = DotlineIcons.Bug,
                    onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/sofyan-rs/dotlink/issues"))) },
                    trailing = { ListRowChevron() },
                )
                ListDivider()
                ListRow(
                    title = "About Dotlink",
                    leadingIcon = DotlineIcons.Info,
                    onClick = { about = true },
                    trailing = { ListRowChevron() },
                )
            }
            Text(
                text = "DOTLINK $version",
                style = Dotline.type.label,
                color = Dotline.colors.inkFaint,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(top = Dotline.space.s3),
            )
            if (about) {
                DotlineBottomSheet(onDismissRequest = { about = false }) {
                    SheetHeader(icon = DotlineIcons.Info, title = "Dotlink", subtitle = "Version $version")
                    Text(
                        text = "Dotlink is made by Kurodev.",
                        style = Dotline.type.body,
                        color = Dotline.colors.inkMuted,
                        modifier = Modifier.padding(horizontal = Dotline.space.s1),
                    )
                    ListGroup(inSheet = true) {
                        ListRow(
                            title = "Kurodev website",
                            subtitle = "kurodev.org",
                            subtitleIsData = true,
                            leadingIcon = DotlineIcons.Globe,
                            inSheet = true,
                            onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://kurodev.org"))) },
                            trailing = { ListRowChevron() },
                        )
                    }
                }
            }
        }
    }
}

/** Which apps' notifications go to the PC: those that posted one since it was turned on. */
@Composable
private fun NotificationAppsSheet(onDismissRequest: () -> Unit) {
    val prefs = DotlinkApp.link.prefs
    val apps = remember { prefs.seenApps.entries.sortedBy { it.value.lowercase() } }
    DotlineBottomSheet(onDismissRequest = onDismissRequest) {
        SheetHeader(icon = DotlineIcons.Sliders, title = "Apps on the PC", subtitle = null)
        if (apps.isEmpty()) {
            Hint("Apps show here once they post a notification.")
            return@DotlineBottomSheet
        }
        Column(Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState())) {
            ListGroup(inSheet = true) {
                apps.forEachIndexed { i, (pkg, label) ->
                    var on by remember(pkg) { mutableStateOf(prefs.forwardsApp(pkg)) }
                    if (i > 0) ListDivider()
                    ListRow(
                        title = label,
                        subtitle = pkg,
                        subtitleIsData = true,
                        inSheet = true,
                        role = Role.Switch,
                        onClick = {
                            on = !on
                            prefs.setForwardsApp(pkg, on)
                        },
                        trailing = { KuSwitch(checked = on, onCheckedChange = null) },
                    )
                }
            }
        }
    }
}
