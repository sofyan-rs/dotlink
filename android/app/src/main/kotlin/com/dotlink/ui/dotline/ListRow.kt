/**
 * ListRow: one row inside a [ListGroup].
 *
 * A row has an optional leading icon, a title (`headline`), an optional subtitle (`body-s` muted,
 * or `data` mono for paths), and a trailing slot (count text, chevron, switch, radio …).
 * An optional ⋮ button sits flush at the right edge. Pressed and selected rows get a
 * `surfaceRaised` background; destructive rows draw title and icon in `accentText`.
 *
 * Seen in: Main (2.1), Folder-Actions (2.2), Note-Actions (2.3), Vaults (6.2),
 * Vault-Actions (6.3), Git-Sync (7.1).
 *
 * Also in this file: the small trailing pieces [ListRowValue] and [ListRowChevron], and
 * [ListRowFrame], the shared row shell that [TreeRow] builds on.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Icons in sheet rows are 22 dp with a 16 dp gap (Folder-/Note-/Vault-Actions mockups). */
private val SheetIconSize = 22.dp

/** The trailing chevron is 18 dp (Git-Sync mockup). */
private val TrailingChevronSize = 18.dp

/**
 * A list row.
 *
 * @param title row title in `headline`.
 * @param subtitle optional second line, `body-s` in `inkMuted`.
 * @param leadingIcon optional icon at the start (24 dp; 22 dp when [inSheet]).
 * @param subtitleIsData draw the subtitle in mono `data` style, one line with "…" (vault paths, git remote).
 * @param selected draws the `surfaceRaised` background and tells TalkBack the row is selected.
 * @param destructive draws title and icon in `accentText` ("Move to trash", "Remove vault").
 * @param inSheet use the sheet layout: 22 dp icon, 16 dp gap. Pair with `ListGroup(inSheet = true)`.
 * @param minHeight 56 dp by default; use 72 dp for two-line vault rows, 64 dp for Git-Sync and search rows.
 * @param role what the row does for accessibility: [Role.Button] (default), [Role.RadioButton], [Role.Switch].
 * @param onClick tap on the row. `null` makes the row not clickable (e.g. when only the trailing switch is).
 * @param onLongClick long press on the row (e.g. open the actions sheet).
 * @param onMoreClick shows a ⋮ button at the right edge when not `null`.
 * @param moreContentDescription TalkBack text of the ⋮ button.
 * @param moreIcon the button's icon, when it does one thing (e.g. trash) instead of opening a menu.
 * @param trailing slot at the end of the row: [ListRowValue], [ListRowChevron], a switch, a radio …
 */
@Composable
fun ListRow(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    leadingIcon: ImageVector? = null,
    subtitleIsData: Boolean = false,
    selected: Boolean = false,
    destructive: Boolean = false,
    inSheet: Boolean = false,
    minHeight: Dp = Dotline.size.rowMin,
    role: Role = Role.Button,
    onClick: (() -> Unit)? = null,
    onLongClick: (() -> Unit)? = null,
    onMoreClick: (() -> Unit)? = null,
    moreContentDescription: String = "Actions for $title",
    moreIcon: ImageVector = DotlineIcons.More,
    trailing: (@Composable () -> Unit)? = null,
) {
    val contentColor = if (destructive) Dotline.colors.accentText else Dotline.colors.ink
    ListRowFrame(
        modifier = modifier,
        selected = selected,
        minHeight = minHeight,
        startPadding = Dotline.space.s4,
        contentGap = if (inSheet) Dotline.space.s4 else Dotline.space.s3,
        role = role,
        onClick = onClick,
        onLongClick = onLongClick,
        onMoreClick = onMoreClick,
        moreContentDescription = moreContentDescription,
        moreIcon = moreIcon,
    ) {
        if (leadingIcon != null) {
            Icon(
                imageVector = leadingIcon,
                contentDescription = null, // The title already says what the row is.
                tint = contentColor,
                modifier = Modifier.size(if (inSheet) SheetIconSize else Dotline.size.icon),
            )
        }
        ListRowTexts(
            title = title,
            subtitle = subtitle,
            titleColor = contentColor,
            subtitleIsData = subtitleIsData,
            // weight(1f) takes the free width, which pushes the trailing slot to the end.
            modifier = Modifier.weight(1f),
        )
        if (trailing != null) trailing()
    }
}

/** Mono value at the end of a row: a count ("4"), a state ("On", "Set") or a path ("/"). */
@Composable
fun ListRowValue(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = Dotline.colors.inkMuted,
) {
    Text(text = text, style = Dotline.type.data, color = color, maxLines = 1, modifier = modifier)
}

/** The ▸ chevron at the end of a row that opens another screen (Git-Sync "Remote", "Sign-in"). */
@Composable
fun ListRowChevron(modifier: Modifier = Modifier) {
    Icon(
        imageVector = DotlineIcons.ChevronRight,
        contentDescription = null,
        tint = Dotline.colors.inkMuted,
        modifier = modifier.size(TrailingChevronSize),
    )
}

/** Title + optional subtitle, stacked. Shared by [ListRow] and [TreeRow]. */
@Composable
internal fun ListRowTexts(
    title: String,
    subtitle: String?,
    titleColor: Color,
    modifier: Modifier = Modifier,
    subtitleIsData: Boolean = false,
) {
    Column(modifier = modifier) {
        Text(
            text = title,
            style = Dotline.type.headline,
            color = titleColor,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        if (subtitle != null) {
            Text(
                text = subtitle,
                style = if (subtitleIsData) Dotline.type.data else Dotline.type.bodyS,
                color = Dotline.colors.inkMuted,
                maxLines = if (subtitleIsData) 1 else 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/**
 * The shell every row shares: background (pressed / selected), click and long-press handling,
 * focus ring, start padding, and the optional ⋮ button flush at the right edge.
 *
 * [content] is laid out in a Row with [contentGap] between the items.
 */
@Composable
internal fun ListRowFrame(
    modifier: Modifier,
    selected: Boolean,
    minHeight: Dp,
    startPadding: Dp,
    contentGap: Dp,
    role: Role,
    onClick: (() -> Unit)?,
    onLongClick: (() -> Unit)?,
    onMoreClick: (() -> Unit)?,
    moreContentDescription: String,
    moreIcon: ImageVector = DotlineIcons.More,
    stateDescription: String? = null,
    content: @Composable RowScope.() -> Unit,
) {
    // `remember` keeps the same object across recompositions. The InteractionSource reports
    // press and focus events of the clickable area, which we read for our own pressed look.
    val interactionSource = remember { MutableInteractionSource() }
    val pressed by interactionSource.collectIsPressedAsState()
    val background = if (selected || pressed) Dotline.colors.surfaceRaised else Color.Transparent
    val clickable = onClick != null || onLongClick != null

    Row(
        modifier = modifier.fillMaxWidth().background(background),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            modifier =
                Modifier
                    .weight(1f)
                    .heightIn(min = minHeight)
                    .then(
                        if (clickable) {
                            Modifier
                                .combinedClickable(
                                    interactionSource = interactionSource,
                                    indication = null, // No Material ripple: the background above is our feedback.
                                    role = role,
                                    onLongClick = onLongClick,
                                    onClick = onClick ?: {},
                                ).insetFocusRing(interactionSource)
                        } else {
                            Modifier
                        },
                    ).semantics {
                        if (selected) this.selected = true
                        if (stateDescription != null) this.stateDescription = stateDescription
                    }
                    // With a ⋮ button the content runs right up to it; otherwise keep the 16 dp margin.
                    .padding(start = startPadding, end = if (onMoreClick != null) 0.dp else Dotline.space.s4)
                    // Single-line rows are held at minHeight anyway; this keeps wrapped subtitles off the edges.
                    .padding(vertical = Dotline.space.s3),
            horizontalArrangement = Arrangement.spacedBy(contentGap),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
        if (onMoreClick != null) {
            MoreButton(onClick = onMoreClick, contentDescription = moreContentDescription, icon = moreIcon)
        }
    }
}

/**
 * Focus ring for rows. The shared [focusRing] draws outside the element, but rows sit inside a
 * clipped [ListGroup] (or menu), so that ring would be cut off. This one is drawn 2 dp inside
 * the row instead. It only draws, so the layout never changes.
 */
@Composable
internal fun Modifier.insetFocusRing(interactionSource: InteractionSource): Modifier {
    val focused by interactionSource.collectIsFocusedAsState()
    val color = Dotline.colors.focus
    return this.drawWithContent {
        drawContent()
        if (focused) {
            val ringWidth = 2.dp.toPx()
            val inset = 2.dp.toPx() + ringWidth / 2 // ring centre line: 2 dp gap + half the ring
            drawRoundRect(
                color = color,
                topLeft = Offset(inset, inset),
                size = Size(size.width - 2 * inset, size.height - 2 * inset),
                cornerRadius = CornerRadius(8.dp.toPx()),
                style = Stroke(ringWidth),
            )
        }
    }
}

@Composable
private fun ListRowPreviewContent() {
    ListGroup {
        ListRow(
            title = "Personal",
            subtitle = "Documents/Personal · 9 notes",
            subtitleIsData = true,
            leadingIcon = DotlineIcons.Vault,
            minHeight = 72.dp,
            onClick = {},
            trailing = { ListRowValue("OPEN", color = Dotline.colors.ink) },
        )
        ListDivider()
        ListRow(
            title = "Work",
            subtitle = "Synced from git · 41 notes",
            subtitleIsData = true,
            leadingIcon = DotlineIcons.Vault,
            minHeight = 72.dp,
            onClick = {},
            onMoreClick = {},
        )
        ListDivider()
        ListRow(title = "Sign-in", onClick = {}, trailing = {
            Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3), verticalAlignment = Alignment.CenterVertically) {
                ListRowValue("Access token")
                ListRowChevron()
            }
        })
        ListDivider()
        ListRow(title = "Example", subtitle = "2 min ago · 645 chars", leadingIcon = DotlineIcons.Note, selected = true, onClick = {})
        ListDivider()
        ListRow(
            title = "Move to trash",
            subtitle = "With everything inside: 4 items",
            leadingIcon = DotlineIcons.Trash,
            destructive = true,
            onClick = {},
        )
    }
}

@Preview(name = "Dark")
@Composable
private fun ListRowDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { ListRowPreviewContent() }
    }
}

@Preview(name = "Light")
@Composable
private fun ListRowLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { ListRowPreviewContent() }
    }
}
