/**
 * The main Activity: pairing when no PC is paired (or another PC is being added), else the tabs.
 */
package com.dotlink.ui

import android.Manifest
import android.content.Intent
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.ui.phones.PhonePairScreen
import com.dotlink.ui.phones.PhonesHomeScreen
import com.dotlink.link.LinkState
import com.dotlink.ui.dotline.BottomNav
import com.dotlink.ui.dotline.LocalNavRail
import com.dotlink.ui.dotline.NavRail
import com.dotlink.ui.dotline.NavRailMinWidth
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.DotlineSnackbarHost
import com.dotlink.ui.dotline.MainTab
import com.dotlink.ui.dotline.bottomNavHeight
import com.dotlink.ui.dotline.showDotlineSnackbar
import com.dotlink.ui.camera.CameraScreen
import com.dotlink.ui.home.LinkScreen
import com.dotlink.ui.pair.ChoosePcScreen
import com.dotlink.ui.pair.PairScreen
import com.dotlink.ui.remote.RemoteScreen
import com.dotlink.ui.settings.SettingsScreen
import com.dotlink.ui.shell.ShellScreen
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme
import com.dotlink.update.UpdateDialog
import com.dotlink.update.Updater

class MainActivity : ComponentActivity() {
    private val link get() = DotlinkApp.link

    private val notificationPermission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    /** A `dotlink://pair?...` link opened from a QR scanner; the pair screen pre-fills it. */
    private val pairLink = mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT),
        )
        if (Build.VERSION.SDK_INT >= 33) notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
        handleLink(intent)
        setContent {
            val dark = isAppInDarkTheme()
            // SystemBarStyle.auto follows the system, so set the bar icons from our own choice.
            LaunchedEffect(dark) {
                val style =
                    if (dark) SystemBarStyle.dark(Color.TRANSPARENT) else SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
                enableEdgeToEdge(statusBarStyle = style, navigationBarStyle = style)
            }
            DotlinkTheme(dark = dark) {
                DotlinkRoot(pairLink.value, onPairLinkUsed = { pairLink.value = null })
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleLink(intent)
    }

    private fun handleLink(intent: Intent?) {
        val data = intent?.data?.takeIf { it.scheme == "dotlink" } ?: return
        // Already paired: this adds another PC.
        pairLink.value = data.toString()
    }

    override fun onResume() {
        super.onResume()
        // After "Disconnect for now" only Reconnect brings the link back.
        // Nor before a PC is picked (several paired).
        if (link.prefs.paired && link.state.value !is LinkState.Disconnected && !link.choosePc.value) {
            LinkService.send(this)
        } else if (DotlinkApp.phones.needed) {
            // No PC (or not now), but paired phones: keep receiving from them.
            LinkService.send(this)
        }
    }

    // Android only lets the focused app read the clipboard, so we send it when we gain focus.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && link.prefs.autoClipboard) link.sendPhoneClipboard(force = false)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DotlinkRoot(
    pairLink: String?,
    onPairLinkUsed: () -> Unit,
) {
    val link = DotlinkApp.link
    val context = LocalContext.current
    val state by link.state.collectAsStateWithLifecycle()
    val choosePc by link.choosePc.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(Unit) {
        link.notices.collect { snackbar.showDotlineSnackbar(it, actionLabel = null) }
    }
    LaunchedEffect(Unit) { Updater.checkOnStart(context) }
    UpdateDialog()
    val phones by DotlinkApp.phones.list.collectAsStateWithLifecycle()
    // Pairing another PC while one is paired already.
    var addingPc by rememberSaveable { mutableStateOf(false) }
    var pairingPhone by rememberSaveable { mutableStateOf(false) }
    // Settings from the Phones screen (no PC, so no tabs).
    var phonesSettings by rememberSaveable { mutableStateOf(false) }
    // No PC, but phones: their own home screen instead of the PC pairing screen.
    val phonesHome = state is LinkState.Unpaired && phones.isNotEmpty() && !addingPc && pairLink == null
    // Pairing or choosing a PC: no tabs (and no tab bar under the snackbar).
    val fullScreen = state is LinkState.Unpaired || addingPc || pairLink != null || choosePc || pairingPhone
    BoxWithConstraints(Modifier.fillMaxSize().background(Dotline.colors.bg)) {
        // Landscape or a wide screen: the tabs on the left instead of along the bottom.
        val rail = !fullScreen && (maxWidth > maxHeight || maxWidth >= NavRailMinWidth)
        CompositionLocalProvider(LocalNavRail provides rail) {
            if (pairingPhone) {
                PhonePairScreen(onDone = { pairingPhone = false })
            } else if (phonesHome && phonesSettings) {
                SettingsScreen(onBack = { phonesSettings = false })
            } else if (phonesHome) {
                PhonesHomeScreen(onPairPhone = { pairingPhone = true }, onPairPc = { addingPc = true }, onSettings = { phonesSettings = true })
            } else if (state is LinkState.Unpaired) {
                PairScreen(
                    initialLink = pairLink,
                    onPaired = {
                        addingPc = false
                        onPairLinkUsed()
                        LinkService.send(context)
                    },
                    onCancel =
                        if (phones.isNotEmpty()) {
                            {
                                addingPc = false
                                onPairLinkUsed()
                            }
                        } else {
                            null
                        },
                    onPairPhone = { pairingPhone = true },
                )
            } else if (addingPc || pairLink != null) {
                fun close() {
                    addingPc = false
                    onPairLinkUsed()
                }
                PairScreen(
                    initialLink = pairLink,
                    onPaired = {
                        close()
                        // The new PC is the active one now: leave the old one.
                        LinkService.send(context, LinkService.ACTION_SWITCH)
                    },
                    onCancel = ::close,
                )
            } else if (choosePc) {
                ChoosePcScreen(
                    onChoose = { id ->
                        // Active first, so the service connects straight to it.
                        link.switchTo(id)
                        LinkService.send(context)
                    },
                    onAddPc = { addingPc = true },
                )
            } else {
                var tab by rememberSaveable { mutableStateOf(MainTab.Link) }
                val screen: @Composable () -> Unit = {
                    when (tab) {
                        MainTab.Link -> LinkScreen(onOpenTab = { tab = it }, onAddPc = { addingPc = true }, onPairPhone = { pairingPhone = true })
                        MainTab.Remote -> RemoteScreen()
                        MainTab.Shell -> ShellScreen()
                        MainTab.Camera -> CameraScreen()
                        MainTab.Settings -> SettingsScreen()
                    }
                }
                if (rail) {
                    // The rail stays with the keyboard open: it takes width, not height.
                    Row(Modifier.fillMaxSize()) {
                        NavRail(tabs = MainTab.entries, selectedTab = tab, onTabClick = { tab = it })
                        Box(Modifier.weight(1f).fillMaxHeight()) { screen() }
                    }
                } else {
                    screen()
                }
                // The keyboard needs the room in Remote and Shell; the tabs come back when it closes.
                if (!rail && !WindowInsets.isImeVisible) {
                    BottomNav(
                        tabs = MainTab.entries,
                        selectedTab = tab,
                        onTabClick = { tab = it },
                        modifier = Modifier.align(Alignment.BottomCenter),
                    )
                }
            }
            DotlineSnackbarHost(
                hostState = snackbar,
                icon = DotlineIcons.Monitor,
                modifier =
                    Modifier
                        .align(Alignment.BottomCenter)
                        .padding(bottom = if (fullScreen) Dotline.space.s4 else bottomNavHeight()),
            )
        }
    }
}
