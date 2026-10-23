/**
 * "Change PC": the paired PCs (the active one ticked) and pairing another. The phone talks to
 * one PC at a time; picking another one leaves the current one.
 */
package com.dotlink.ui.home

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import com.dotlink.DotlinkApp
import com.dotlink.link.LinkService
import com.dotlink.link.LinkState
import com.dotlink.link.Pc
import com.dotlink.ui.dotline.DestructiveButton
import com.dotlink.ui.dotline.DotlineDialog
import com.dotlink.ui.dotline.SecondaryButton
import com.dotlink.ui.dotline.DotlineBottomSheet
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.ListDivider
import com.dotlink.ui.dotline.ListGroup
import com.dotlink.ui.dotline.ListRow
import com.dotlink.ui.dotline.PrimaryButton
import com.dotlink.ui.dotline.RadioDot
import com.dotlink.ui.theme.Dotline

@Composable
fun PcSheet(
    state: LinkState,
    onAddPc: () -> Unit,
    onDismissRequest: () -> Unit,
) {
    val link = DotlinkApp.link
    val context = LocalContext.current
    var pcs by remember { mutableStateOf(link.prefs.pcs) }
    val activeId = link.prefs.active?.id
    var forgetting by remember { mutableStateOf<Pc?>(null) }
    DotlineBottomSheet(onDismissRequest = onDismissRequest) {
        Text(
            "Change PC",
            style = Dotline.type.title,
            color = Dotline.colors.ink,
            modifier = Modifier.padding(start = Dotline.space.s1),
        )
        ListGroup(inSheet = true) {
            pcs.forEachIndexed { i, pc ->
                if (i > 0) ListDivider()
                val active = pc.id == activeId
                val where = "${pc.lastHost ?: pc.hosts.firstOrNull().orEmpty()} · port ${pc.port}"
                ListRow(
                    title = pc.name,
                    subtitle =
                        if (!active) {
                            where
                        } else {
                            when (state) {
                                is LinkState.Online -> "Connected · $where"
                                LinkState.Connecting -> "Connecting…"
                                LinkState.Disconnected -> "Disconnected"
                                else -> "Offline · $where"
                            }
                        },
                    subtitleIsData = true,
                    leadingIcon = DotlineIcons.Monitor,
                    inSheet = true,
                    role = Role.RadioButton,
                    onClick = {
                        onDismissRequest()
                        // Picking the active PC again reconnects it (after "Disconnect for now").
                        if (!active || state !is LinkState.Online) {
                            LinkService.send(context, LinkService.ACTION_SWITCH, pc = pc.id)
                        }
                    },
                    trailing = { RadioDot(selected = active, onClick = null) },
                    onMoreClick = { forgetting = pc },
                    moreContentDescription = "Forget ${pc.name}",
                    moreIcon = DotlineIcons.Trash,
                )
            }
        }
        PrimaryButton(
            text = "Pair a new PC",
            icon = DotlineIcons.Plus,
            onClick = {
                onDismissRequest()
                onAddPc()
            },
            modifier = Modifier.fillMaxWidth(),
        )
    }
    forgetting?.let { pc ->
        DotlineDialog(
            onDismissRequest = { forgetting = null },
            title = "Forget ${pc.name}?",
            body = "This phone stops connecting to it. To link again, pair from the PC's Dotlink app.",
            confirmButton = {
                DestructiveButton(
                    text = "Forget",
                    compact = true,
                    onClick = {
                        forgetting = null
                        link.forget(pc.id)
                        pcs = link.prefs.pcs
                        if (pcs.isEmpty() || pc.id == activeId) onDismissRequest()
                    },
                    modifier = Modifier.weight(1f),
                )
            },
            dismissButton = {
                SecondaryButton(text = "Cancel", compact = true, onClick = { forgetting = null }, modifier = Modifier.weight(1f))
            },
        )
    }
}
