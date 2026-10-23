/**
 * ListGroup: the rounded block that holds a list of [ListRow]s, plus [ListDivider], the hairline
 * between two rows.
 *
 * Seen in: Main (2.1, VAULT tree), Folder-Actions (2.2), Note-Actions (2.3), Vaults (6.2),
 * Vault-Actions (6.3), Git-Sync (7.1, REPOSITORY).
 *
 * Usage:
 * ```
 * ListGroup {
 *     ListRow(title = "Rename", leadingIcon = DotlineIcons.Edit, onClick = { ... })
 *     ListDivider()
 *     ListRow(title = "Move to trash", leadingIcon = DotlineIcons.Trash, destructive = true, onClick = { ... })
 * }
 * ```
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Thickness of the hairline between rows (1 CSS px in the mockups). */
private val HairlineThickness = 1.dp

/**
 * A `surface` block with radius 16 that stacks rows vertically.
 *
 * The group clips its children to the rounded shape, so a pressed or selected row's
 * `surfaceRaised` background gets rounded corners at the top and bottom of the group.
 *
 * @param inSheet `true` inside a bottom sheet. The sheet itself is `surface`, so the group
 *   uses `bg` instead to stand out (Folder-Actions, Note-Actions, Vault-Actions).
 * @param content the rows. Put a [ListDivider] between two rows yourself.
 */
@Composable
fun ListGroup(
    modifier: Modifier = Modifier,
    inSheet: Boolean = false,
    content: @Composable ColumnScope.() -> Unit,
) {
    val containerColor = if (inSheet) Dotline.colors.bg else Dotline.colors.surface
    Column(
        // Modifier order matters: clip first, so the background (and every row drawn later)
        // is cut to the rounded shape.
        modifier =
            modifier
                .fillMaxWidth()
                .clip(Dotline.radius.m)
                .background(containerColor),
        content = content,
    )
}

/** The 1 dp `line` hairline between two rows of a [ListGroup]. It runs the full width, as in the mockups. */
@Composable
fun ListDivider(modifier: Modifier = Modifier) {
    Box(
        modifier =
            modifier
                .fillMaxWidth()
                .height(HairlineThickness)
                .background(Dotline.colors.line),
    )
}

@Composable
private fun ListGroupPreviewContent() {
    Column {
        ListGroup {
            ListRow(title = "Rename folder", leadingIcon = DotlineIcons.Edit, onClick = {})
            ListDivider()
            ListRow(title = "Move to…", leadingIcon = DotlineIcons.MoveTo, onClick = {})
        }
        Box(Modifier.padding(top = Dotline.space.s4).background(Dotline.colors.surface).padding(Dotline.space.s4)) {
            ListGroup(inSheet = true) {
                ListRow(title = "Open vault", leadingIcon = DotlineIcons.Folder, inSheet = true, onClick = {})
                ListDivider()
                ListRow(title = "Remove vault", leadingIcon = DotlineIcons.RemoveVault, inSheet = true, destructive = true, onClick = {})
            }
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun ListGroupDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { ListGroupPreviewContent() }
    }
}

@Preview(name = "Light")
@Composable
private fun ListGroupLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { ListGroupPreviewContent() }
    }
}
