/**
 * Shown when the app opens with several PCs paired: pick the one to connect to, or pair another.
 */
package com.dotlink.ui.pair

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.dotlink.DotlinkApp
import com.dotlink.ui.dotline.DotArt
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.ListRowChevron
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.dotline.SectionLabel
import com.dotlink.ui.theme.Dotline

@Composable
fun ChoosePcScreen(
    onChoose: (String) -> Unit,
    onAddPc: () -> Unit,
) {
    val prefs = DotlinkApp.link.prefs
    val lastId = prefs.active?.id
    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.statusBars)
            .windowInsetsPadding(WindowInsets.navigationBars)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = Dotline.space.s4),
    ) {
        Spacer(Modifier.height(Dotline.space.s7))
        DotArt(pattern = LinkArt, contentDescription = null, dotSize = 6.dp)
        Spacer(Modifier.height(Dotline.space.s6))
        Text("DOTLINK", style = Dotline.type.dotL, color = Dotline.colors.ink)
        Spacer(Modifier.height(Dotline.space.s2))
        Text("Which PC do you want to connect to?", style = Dotline.type.body, color = Dotline.colors.inkMuted)
        Spacer(Modifier.height(Dotline.space.s5))
        SectionLabel("Paired PCs", Modifier.padding(bottom = Dotline.space.s2))
        ListGroup {
            prefs.pcs.forEachIndexed { i, pc ->
                if (i > 0) ListDivider()
                val where = "${pc.lastHost ?: pc.hosts.firstOrNull().orEmpty()} · port ${pc.port}"
                ListRow(
                    title = pc.name,
                    subtitle = if (pc.id == lastId) "Last used · $where" else where,
                    subtitleIsData = true,
                    leadingIcon = DotlineIcons.Monitor,
                    onClick = { onChoose(pc.id) },
                    trailing = { ListRowChevron() },
                )
            }
        }
        Spacer(Modifier.height(Dotline.space.s5))
        SecondaryButton(
            text = "Pair a new PC",
            icon = DotlineIcons.Plus,
            onClick = onAddPc,
            modifier = Modifier.fillMaxWidth(),
        )
        Spacer(Modifier.height(Dotline.space.s6))
    }
}
