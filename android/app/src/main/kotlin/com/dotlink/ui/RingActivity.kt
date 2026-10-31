package com.dotlink.ui

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.lifecycleScope
import com.dotlink.DotlinkApp
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/** Full screen over the lock screen while the PC rings this phone: Stop, and who rang. */
class RingActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setShowWhenLocked(true)
        setTurnScreenOn(true)
        val ringer = DotlinkApp.link.ringer
        if (!ringer.ringing.value) {
            finish()
            return
        }
        // Stopped from the PC, the notification or by itself: nothing left to show.
        lifecycleScope.launch {
            ringer.ringing.first { !it }
            finish()
        }
        setContent {
            DotlinkTheme(dark = isAppInDarkTheme()) {
                Column(
                    modifier = Modifier.fillMaxSize().background(Dotline.colors.bg).padding(32.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    Text("RINGING", style = Dotline.type.dotL, color = Dotline.colors.ink)
                    Spacer(Modifier.height(8.dp))
                    Text("from ${ringer.from}", style = Dotline.type.body, color = Dotline.colors.inkMuted)
                    Spacer(Modifier.height(48.dp))
                    PrimaryButton(text = "Stop", onClick = { ringer.stop() }, modifier = Modifier.fillMaxWidth())
                }
            }
        }
    }
}
