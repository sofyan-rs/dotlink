/**
 * TreeRow: one folder or note in the vault file tree (Files tab).
 *
 * It is a [ListRow] with tree extras:
 * - folders: a ▸ / ▾ chevron (collapsed / expanded), the folder icon, the name and a count;
 * - notes: the note icon, the name and a subtitle ("2 min ago · 645 chars");
 * - locked notes: the lock icon and "Locked · encrypted";
 * - 28 dp of extra start indent per depth level;
 * - the selected note gets the `surfaceRaised` background; ⋮ and long-press open the actions sheet.
 *
 * Put TreeRows in a [ListGroup] with a [ListDivider] between them.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Extra start padding per tree depth level (Main mockup: 12, 40, 68 dp). */
private val IndentPerDepth = 28.dp

/** The folder ▸ / ▾ chevron is 16 dp. */
private val TreeChevronSize = 16.dp

/** Subtitle shown for a locked note when the caller passes none. */
private const val LOCKED_SUBTITLE = "Locked · encrypted"

/**
 * A row of the file tree.
 *
 * @param name file or folder name, shown as the title.
 * @param depth 0 for items at the vault root, 1 inside a root folder, and so on.
 * @param isFolder folder (chevron + count) or note (subtitle).
 * @param onClick tap: toggle the folder, or open the note.
 * @param expanded folders only: ▾ when `true`, ▸ when `false`.
 * @param count folders only: number of items inside, shown at the end.
 * @param subtitle notes only: e.g. "2 min ago · 645 chars".
 * @param isLocked notes only: lock icon, and "Locked · encrypted" when [subtitle] is `null`.
 * @param selected the note open in the editor gets the `surfaceRaised` background.
 * @param onLongClick long press, usually opens the same sheet as ⋮.
 * @param onMoreClick shows the ⋮ button when not `null` (→ 2.2 Folder actions / 2.3 Note actions).
 */
@Composable
fun TreeRow(
    name: String,
    depth: Int,
    isFolder: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    expanded: Boolean = false,
    count: Int? = null,
    subtitle: String? = null,
    isLocked: Boolean = false,
    selected: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    onMoreClick: (() -> Unit)? = null,
) {
    ListRowFrame(
        modifier = modifier,
        selected = selected,
        minHeight = Dotline.size.rowMin,
        // 12 dp at the root, then 28 dp more per level. A note's icon sits where a folder's chevron would.
        startPadding = Dotline.space.s3 + IndentPerDepth * depth,
        contentGap = Dotline.space.s3,
        role = Role.Button,
        onClick = onClick,
        onLongClick = onLongClick,
        onMoreClick = onMoreClick,
        moreContentDescription = "Actions for $name",
        stateDescription =
            when {
                !isFolder -> null
                expanded -> "Expanded"
                else -> "Collapsed"
            },
    ) {
        if (isFolder) {
            Icon(
                imageVector = if (expanded) DotlineIcons.ChevronDown else DotlineIcons.ChevronRight,
                contentDescription = null, // Said by stateDescription ("Expanded" / "Collapsed").
                tint = Dotline.colors.inkFaint,
                modifier = Modifier.size(TreeChevronSize),
            )
            Icon(DotlineIcons.Folder, contentDescription = null, tint = Dotline.colors.ink)
            Text(
                text = name,
                style = Dotline.type.headline,
                color = Dotline.colors.ink,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            if (count != null) {
                ListRowValue(text = count.toString(), color = Dotline.colors.inkFaint)
            }
        } else {
            Icon(
                imageVector = if (isLocked) DotlineIcons.Lock else DotlineIcons.Note,
                contentDescription = if (isLocked) "Locked note" else null,
                tint = Dotline.colors.ink,
            )
            ListRowTexts(
                title = name,
                subtitle = subtitle ?: if (isLocked) LOCKED_SUBTITLE else null,
                titleColor = Dotline.colors.ink,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun TreeRowPreviewContent() {
    // Preview state: tap "Projects" in the interactive preview to toggle it.
    var projectsOpen by remember { mutableStateOf(true) }
    ListGroup {
        TreeRow(name = "Journal", depth = 0, isFolder = true, count = 3, onClick = {}, onMoreClick = {})
        ListDivider()
        TreeRow(
            name = "Projects",
            depth = 0,
            isFolder = true,
            expanded = projectsOpen,
            count = 4,
            onClick = { projectsOpen = !projectsOpen },
            onMoreClick = {},
        )
        if (projectsOpen) {
            ListDivider()
            TreeRow(name = "Archive", depth = 1, isFolder = true, count = 1, onClick = {}, onMoreClick = {})
            ListDivider()
            TreeRow(
                name = "Example",
                depth = 1,
                isFolder = false,
                subtitle = "2 min ago · 645 chars",
                selected = true,
                onClick = {},
                onMoreClick = {},
            )
            ListDivider()
            TreeRow(name = "Server Keys", depth = 1, isFolder = false, isLocked = true, onClick = {}, onMoreClick = {})
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun TreeRowDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { TreeRowPreviewContent() }
    }
}

@Preview(name = "Light")
@Composable
private fun TreeRowLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { TreeRowPreviewContent() }
    }
}
