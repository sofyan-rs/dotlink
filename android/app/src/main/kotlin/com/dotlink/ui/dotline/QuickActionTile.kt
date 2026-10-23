/**
 * QuickActionTile: a square-ish action button at the top of an actions sheet.
 * 72 dp tall, `surfaceRaised`, radius 16, a 22 dp icon over a
 * `label` caption (NEW NOTE, COPY PATH, PIN, SHARE …).
 *
 * Seen in: Folder-Actions (2.2), Note-Actions (2.3).
 *
 * Also in this file: [QuickAction] (the data for one tile) and [QuickActionRow], which lays
 * out the tiles in equal columns with 8 dp gaps (3 in the mockups).
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Tile height from the mockups. */
private val TileHeight = 72.dp

/** Tile icon size and the gap between icon and label (mockups: 22 dp, 6 dp). */
private val TileIconSize = 22.dp
private val TileIconLabelGap = 6.dp

/** The data for one tile in a [QuickActionRow]. [label] is shown in capitals. */
data class QuickAction(
    val label: String,
    val icon: ImageVector,
    val onClick: () -> Unit,
)

/**
 * One tile. It shrinks to 97% while pressed, like the other Dotline buttons.
 *
 * @param label shown in capitals (uppercased for you).
 */
@Composable
fun QuickActionTile(
    label: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val interactionSource = remember { MutableInteractionSource() }
    Column(
        // Order matters: pressScale and the focus ring come before clip, so the ring isn't cut off.
        modifier =
            modifier
                .height(TileHeight)
                .pressScale(interactionSource)
                .focusRing(interactionSource, Dotline.radius.m)
                .clip(Dotline.radius.m)
                .background(Dotline.colors.surfaceRaised)
                .clickable(
                    interactionSource = interactionSource,
                    indication = null, // No Material ripple: pressScale is the feedback.
                    role = Role.Button,
                    onClick = onClick,
                ).padding(horizontal = Dotline.space.s1),
        verticalArrangement = Arrangement.spacedBy(TileIconLabelGap, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, contentDescription = null, tint = Dotline.colors.ink, modifier = Modifier.size(TileIconSize))
        Text(
            text = label.uppercase(),
            style = Dotline.type.label,
            color = Dotline.colors.ink,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** Lays out [actions] as equal-width tiles in one row, 8 dp apart. */
@Composable
fun QuickActionRow(
    actions: List<QuickAction>,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
    ) {
        actions.forEach { action ->
            QuickActionTile(
                label = action.label,
                icon = action.icon,
                onClick = action.onClick,
                // weight(1f) gives every tile the same share of the row width.
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun QuickActionPreviewContent() {
    QuickActionRow(
        actions =
            listOf(
                QuickAction("New note", DotlineIcons.NewNote, onClick = {}),
                QuickAction("New folder", DotlineIcons.NewFolder, onClick = {}),
                QuickAction("Copy path", DotlineIcons.Copy, onClick = {}),
            ),
    )
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun QuickActionTileDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { QuickActionPreviewContent() }
    }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun QuickActionTileLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { QuickActionPreviewContent() }
    }
}
