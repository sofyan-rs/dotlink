/**
 * The main Activity: pairing when no PC is paired, else the four tabs.
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
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
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
import com.dotlink.link.LinkState
import com.dotlink.ui.dotline.BottomNav
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.DotlineSnackbarHost
import com.dotlink.ui.dotline.MainTab
import com.dotlink.ui.dotline.bottomNavHeight
import com.dotlink.ui.dotline.showDotlineSnackbar
import com.dotlink.ui.camera.CameraScreen
import com.dotlink.ui.home.LinkScreen
import com.dotlink.ui.pair.PairScreen
import com.dotlink.ui.remote.RemoteScreen
import com.dotlink.ui.settings.SettingsScreen
import com.dotlink.ui.shell.ShellScreen
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

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
                DotlinkRoot(pairLink.value)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleLink(intent)
    }

    private fun handleLink(intent: Intent?) {
        val data = intent?.data?.takeIf { it.scheme == "dotlink" } ?: return
        if (link.prefs.paired) {
            link.notices.tryEmit("Already paired with ${link.prefs.serverName}. Unpair in Settings first.")
        } else {
            pairLink.value = data.toString()
        }
    }

    override fun onResume() {
        super.onResume()
        if (link.prefs.paired) LinkService.send(this)
    }

    // Android only lets the focused app read the clipboard, so we send it when we gain focus.
    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus && link.prefs.autoClipboard) link.sendPhoneClipboard(force = false)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DotlinkRoot(pairLink: String?) {
    val link = DotlinkApp.link
    val context = LocalContext.current
    val state by link.state.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(Unit) {
        link.notices.collect { snackbar.showDotlineSnackbar(it, actionLabel = null) }
    }
    Box(Modifier.fillMaxSize().background(Dotline.colors.bg)) {
        if (state is LinkState.Unpaired) {
            PairScreen(initialLink = pairLink, onPaired = { LinkService.send(context) })
        } else {
            var tab by rememberSaveable { mutableStateOf(MainTab.Link) }
            when (tab) {
                MainTab.Link -> LinkScreen(onOpenTab = { tab = it })
                MainTab.Remote -> RemoteScreen()
                MainTab.Shell -> ShellScreen()
                MainTab.Camera -> CameraScreen()
                MainTab.Settings -> SettingsScreen()
            }
            // The keyboard needs the room in Remote and Shell; the tabs come back when it closes.
            if (!WindowInsets.isImeVisible) {
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
                    .padding(bottom = if (state is LinkState.Unpaired) Dotline.space.s4 else bottomNavHeight()),
        )
    }
}
