/**
 * The SETTINGS tab: clipboard, where received files go, remote and shell options, the paired
 * PC, theme and about.
 */
package com.dotlink.ui.settings

import android.content.Intent
import android.net.Uri
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

@OptIn(kotlinx.coroutines.FlowPreview::class)
@Composable
fun SettingsScreen() {
    val link = DotlinkApp.link
    val prefs = link.prefs
    val context = LocalContext.current
    val state by link.state.collectAsStateWithLifecycle()

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
                helperText = "What your PCs call this phone.",
            )
        }

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

        Section("Received files") {
            ListGroup {
                ListRow(
                    title = "Saved to",
                    // Fixed: Link.download writes through MediaStore into Downloads/Dotlink.
                    subtitle = "Download/Dotlink",
                    subtitleIsData = true,
                    leadingIcon = DotlineIcons.Folder,
                )
            }
            Hint("Files from the PC always go here; open it in your Files app. This can't be changed yet.")
        }

        Section("Remote") {
            val speeds = listOf(1.0f, 1.4f, 2.0f)
            Choice(listOf("Slow", "Normal", "Fast"), speeds.indexOfFirst { it >= prefs.pointerSpeed }.coerceAtLeast(0)) {
                prefs.pointerSpeed = speeds[it]
            }
        }

        Section("Shell") {
            val sizes = listOf(11, 13, 15)
            Choice(listOf("Small", "Medium", "Large"), sizes.indexOf(prefs.terminalFontSize).coerceAtLeast(0)) {
                prefs.terminalFontSize = sizes[it]
            }
        }

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
            val pcId = prefs.active?.id
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
                                prefs.active?.let { link.forget(it.id) }
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

        Section("Theme") {
            Choice(listOf("System", "Light", "Dark"), prefs.themeMode) {
                prefs.themeMode = it
                link.themeMode.value = it
            }
        }

        Section("About") {
            var about by remember { mutableStateOf(false) }
            val version = remember { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            ListGroup {
                // TODO: link these once there is an update feed and an issue tracker.
                ListRow(
                    title = "Check for updates",
                    leadingIcon = DotlineIcons.Refresh,
                    onClick = {},
                    trailing = { ListRowChevron() },
                )
                ListDivider()
                ListRow(
                    title = "Report a bug or suggestion",
                    leadingIcon = DotlineIcons.Bug,
                    onClick = {},
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
