/**
 * ScreenTitle: the big `dot-m` caps title at the top of a tab screen.
 *
 * Optional parts:
 * - a ▾ after the title that makes it a button (Main: "PERSONAL ▾" opens the vault switcher, 6.1);
 * - round 48 dp icon actions at the end ([ScreenTitleAction]: search, new folder, close);
 * - a `data` subline under it, e.g. "9 notes · ● Synced 2 min ago" with a `positive` dot.
 *
 * Seen in: Main (2.1), Quick-Switcher (4.1), Git-Sync (7.1), Vaults (6.2).
 *
 * The title includes the 16 dp screen margin left and right. The status-bar inset above it is
 * the screen's job (e.g. `Modifier.statusBarsPadding()`).
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** The ▾ after a clickable title is 20 dp (Main mockup). */
private val TitleChevronSize = 20.dp

/**
 * The screen title row.
 *
 * @param title shown in capitals ("PERSONAL"); the text is uppercased for you.
 * @param onTitleClick makes the title a button with a ▾ after it, when not `null`.
 * @param titleClickLabel what TalkBack says the title button does.
 * @param subline mono text under the title, e.g. "9 notes".
 * @param status positive status after the subline with a dot in front, e.g. "Synced 2 min ago".
 * @param actions trailing icon buttons, usually [ScreenTitleAction]s.
 */
@Composable
fun ScreenTitle(
    title: String,
    modifier: Modifier = Modifier,
    onTitleClick: (() -> Unit)? = null,
    titleClickLabel: String = "Switch vault",
    subline: String? = null,
    status: String? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Column(
        modifier = modifier.fillMaxWidth().padding(horizontal = Dotline.space.s4),
        verticalArrangement = Arrangement.spacedBy(Dotline.space.s1),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth().heightIn(min = Dotline.size.touchTarget),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            TitleText(
                title = title,
                onClick = onTitleClick,
                clickLabel = titleClickLabel,
                // weight(1f, fill = false): take the space the actions leave, but no more.
                modifier = Modifier.weight(1f, fill = false),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s1), content = actions)
        }
        if (subline != null || status != null) {
            Subline(subline = subline, status = status)
        }
    }
}

/**
 * A round 48 dp icon button for the [ScreenTitle] actions slot.
 *
 * @param contentDescription what TalkBack says, e.g. "Search notes".
 */
@Composable
fun ScreenTitleAction(
    icon: ImageVector,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val interactionSource = remember { MutableInteractionSource() }
    Box(
        modifier =
            modifier
                .size(Dotline.size.touchTarget)
                .pressScale(interactionSource)
                .focusRing(interactionSource, CircleShape)
                .clickable(
                    interactionSource = interactionSource,
                    indication = null, // No Material ripple: pressScale is the feedback.
                    role = Role.Button,
                    onClick = onClick,
                ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = contentDescription, tint = Dotline.colors.ink)
    }
}

@Composable
private fun TitleText(
    title: String,
    onClick: (() -> Unit)?,
    clickLabel: String,
    modifier: Modifier = Modifier,
) {
    val text: @Composable () -> Unit = {
        Text(
            text = title.uppercase(),
            style = Dotline.type.dotM,
            color = Dotline.colors.ink,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.semantics { heading() },
        )
    }
    if (onClick == null) {
        Box(modifier) { text() }
        return
    }
    val interactionSource = remember { MutableInteractionSource() }
    Row(
        modifier =
            modifier
                .heightIn(min = Dotline.size.touchTarget)
                .pressScale(interactionSource)
                .focusRing(interactionSource, Dotline.radius.s)
                .clickable(
                    interactionSource = interactionSource,
                    indication = null,
                    role = Role.Button,
                    onClickLabel = clickLabel,
                    onClick = onClick,
                ),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f, fill = false)) { text() }
        Icon(
            imageVector = DotlineIcons.ChevronDown,
            contentDescription = null, // onClickLabel already tells what the button does.
            tint = Dotline.colors.ink,
            modifier = Modifier.size(TitleChevronSize),
        )
    }
}

/** "9 notes · ● Synced 2 min ago" in `data` style. */
@Composable
private fun Subline(
    subline: String?,
    status: String?,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (subline != null) {
            Text(subline, style = Dotline.type.data, color = Dotline.colors.inkMuted, maxLines = 1)
        }
        if (subline != null && status != null) {
            Text("·", style = Dotline.type.data, color = Dotline.colors.inkMuted)
        }
        if (status != null) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Dotline.space.s1),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // The dot is never alone: the status word next to it carries the meaning.
                Box(Modifier.size(Dotline.size.dot).background(Dotline.colors.positive, CircleShape))
                Text(status, style = Dotline.type.data, color = Dotline.colors.positive, maxLines = 1)
            }
        }
    }
}

@Composable
private fun ScreenTitlePreviewContent() {
    Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s5)) {
        ScreenTitle(
            title = "Personal",
            onTitleClick = {},
            subline = "9 notes",
            status = "Synced 2 min ago",
            actions = {
                ScreenTitleAction(DotlineIcons.Search, contentDescription = "Search notes", onClick = {})
                ScreenTitleAction(DotlineIcons.NewFolder, contentDescription = "New folder", onClick = {})
            },
        )
        ScreenTitle(
            title = "Search",
            actions = { ScreenTitleAction(DotlineIcons.Close, contentDescription = "Close search", onClick = {}) },
        )
        ScreenTitle(title = "Vaults")
    }
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun ScreenTitleDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { ScreenTitlePreviewContent() }
    }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun ScreenTitleLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { ScreenTitlePreviewContent() }
    }
}
