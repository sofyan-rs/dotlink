/**
 * The SETTINGS tab: clipboard, remote and shell options, the paired PC, theme and about.
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
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.ui.Hint
import com.dotlink.ui.Section
import com.dotlink.ui.Choice
import com.dotlink.ui.TabScreen
import com.dotlink.ui.dotline.DestructiveButton
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
import com.dotlink.ui.theme.Dotline

@Composable
fun SettingsScreen() {
    val link = DotlinkApp.link
    val prefs = link.prefs
    val context = LocalContext.current

    TabScreen {
        ScreenTitle(title = "Settings")

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
            Spacer(Modifier.height(Dotline.space.s3))
            val pin = rememberTextFieldState(prefs.terminalPin)
            var show by remember { mutableStateOf(false) }
            LaunchedEffect(pin) { snapshotFlow { pin.text.toString() }.collect { prefs.terminalPin = it } }
            DotlinePasswordField(
                state = pin,
                label = "Terminal PIN",
                passwordVisible = show,
                onPasswordVisibleChange = { show = it },
                helperText = "Only if terminal_pin is set in ~/.config/dotlink/config.toml",
            )
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
            Spacer(Modifier.height(Dotline.space.s4))
            Column(Modifier.padding(horizontal = Dotline.space.s1)) {
                SecondaryButton(
                    text = "Disconnect for now",
                    onClick = { LinkService.send(context, LinkService.ACTION_DISCONNECT) },
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(Dotline.space.s2))
                DestructiveButton(text = "Unpair", onClick = { confirm = true }, modifier = Modifier.fillMaxWidth())
            }
            if (confirm) {
                DotlineDialog(
                    onDismissRequest = { confirm = false },
                    title = "Unpair ${prefs.serverName}?",
                    body = "This phone forgets the PC. To link again, run dotlink pair on the PC and scan the code.",
                    confirmButton = {
                        DestructiveButton(
                            text = "Unpair",
                            compact = true,
                            onClick = {
                                confirm = false
                                LinkService.send(context, LinkService.ACTION_DISCONNECT)
                                link.unpair()
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
            ListGroup {
                ListRow(
                    title = "Created by Kurodev",
                    subtitle = "kurodev.org",
                    subtitleIsData = true,
                    onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://kurodev.org"))) },
                    trailing = { ListRowChevron() },
                )
            }
            val version = remember { context.packageManager.getPackageInfo(context.packageName, 0).versionName }
            Text(
                text = "DOTLINK $version",
                style = Dotline.type.label,
                color = Dotline.colors.inkFaint,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(top = Dotline.space.s3),
            )
        }
    }
}
