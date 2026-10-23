/**
 * Dotline pill buttons: [PrimaryButton], [SecondaryButton] and [DestructiveButton].
 *
 * Used on: First-Launch ("Open a folder", "Clone from git"), Rename / New-Item ("Cancel", "Rename"),
 * Remove-Vault ("Remove"), Set-Password ("Set password and lock"), Vault-Switcher ("Open folder",
 * "Open Personal"), and most other screens.
 *
 * Sizes from the mockups:
 * - regular: 56 dp tall, 17 sp medium text (`headline`), 20 dp icon;
 * - `compact = true` (inside bottom sheets and dialogs): 48 dp tall, 15 sp medium text, 18 dp icon.
 * The three buttons share one private implementation, [DotlineButton]; only the colours differ.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Height of a regular button (screen bottoms). */
private val ButtonHeight = 56.dp

/** Height of a compact button (inside sheets and dialogs). Still a full 48 dp touch target. */
private val CompactButtonHeight = 48.dp

/** Icon size next to the text: 20 dp on regular buttons, 18 dp on compact ones (mockups). */
private val ButtonIconSize = 20.dp
private val CompactButtonIconSize = 18.dp

/**
 * The main action of a screen: `ink` pill with `onInk` text.
 *
 * @param text the button text, sentence case ("Open a folder").
 * @param onClick called when the button is tapped.
 * @param icon optional line icon shown before the text.
 * @param enabled when false the button is greyed out and ignores taps.
 * @param compact true for the 48 dp version used inside bottom sheets and dialogs.
 */
@Composable
fun PrimaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    compact: Boolean = false,
) {
    val colors = Dotline.colors
    DotlineButton(
        text = text,
        onClick = onClick,
        modifier = modifier,
        icon = icon,
        enabled = enabled,
        compact = compact,
        containerColor = if (enabled) colors.ink else colors.surfaceRaised,
        contentColor = if (enabled) colors.onInk else colors.inkFaint,
        border = null,
    )
}

/**
 * A second, less important action: transparent pill with a 1.5 dp `lineStrong` border and `ink` text.
 * Parameters as in [PrimaryButton].
 */
@Composable
fun SecondaryButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    compact: Boolean = false,
) {
    val colors = Dotline.colors
    DotlineButton(
        text = text,
        onClick = onClick,
        modifier = modifier,
        icon = icon,
        enabled = enabled,
        compact = compact,
        containerColor = Color.Transparent,
        contentColor = if (enabled) colors.ink else colors.inkFaint,
        border = BorderStroke(Dotline.size.stroke, if (enabled) colors.lineStrong else colors.line),
    )
}

/**
 * An action that can't be undone ("Remove"): `accent` pill with `onAccent` text.
 * Red is a signal, so use at most one per screen. Parameters as in [PrimaryButton].
 */
@Composable
fun DestructiveButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
    compact: Boolean = false,
) {
    val colors = Dotline.colors
    DotlineButton(
        text = text,
        onClick = onClick,
        modifier = modifier,
        icon = icon,
        enabled = enabled,
        compact = compact,
        containerColor = if (enabled) colors.accent else colors.surfaceRaised,
        contentColor = if (enabled) colors.onAccent else colors.inkFaint,
        border = null,
    )
}

/** The shared pill: the three public buttons only choose the colours. */
@Composable
private fun DotlineButton(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier,
    icon: ImageVector?,
    enabled: Boolean,
    compact: Boolean,
    containerColor: Color,
    contentColor: Color,
    border: BorderStroke?,
) {
    val shape = Dotline.radius.pill
    // One InteractionSource is shared by clickable (which reports presses and focus)
    // and by pressScale / focusRing (which react to them). `remember` keeps the same
    // object across recompositions.
    val interactionSource = remember { MutableInteractionSource() }
    // Text style: 17 sp medium (`headline`) on 56 dp buttons, 15 sp medium (`body`) on compact ones.
    val textStyle =
        if (compact) {
            Dotline.type.body.copy(fontWeight = FontWeight.Medium)
        } else {
            Dotline.type.headline
        }

    // Modifier order matters: each modifier wraps the ones after it.
    // pressScale and focusRing come first so they scale / outline the whole pill,
    // clip comes before background so the fill is rounded.
    Row(
        modifier =
            modifier
                .pressScale(interactionSource)
                .focusRing(interactionSource, shape)
                .height(if (compact) CompactButtonHeight else ButtonHeight)
                .clip(shape)
                .background(containerColor)
                .then(if (border != null) Modifier.border(border, shape) else Modifier)
                .clickable(
                    interactionSource = interactionSource,
                    // No Material ripple: pressScale is our press feedback.
                    indication = null,
                    enabled = enabled,
                    role = Role.Button,
                    onClick = onClick,
                ).padding(horizontal = Dotline.space.s5),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(
                imageVector = icon,
                // The text already says what the button does, so the icon is decorative.
                contentDescription = null,
                tint = contentColor,
                modifier = Modifier.size(if (compact) CompactButtonIconSize else ButtonIconSize),
            )
        }
        Text(
            text = text,
            style = textStyle,
            color = contentColor,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

// ---- Previews -------------------------------------------------------------------------------

@Composable
private fun ButtonsPreviewContent() {
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            PrimaryButton("Open a folder", onClick = {}, icon = DotlineIcons.Folder, modifier = Modifier.fillMaxWidth())
            SecondaryButton("Clone from git", onClick = {}, icon = DotlineIcons.GitBranch, modifier = Modifier.fillMaxWidth())
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryButton("Cancel", onClick = {}, compact = true, modifier = Modifier.weight(1f))
                DestructiveButton("Remove", onClick = {}, compact = true, modifier = Modifier.weight(1f))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                SecondaryButton("Cancel", onClick = {}, compact = true, enabled = false, modifier = Modifier.weight(1f))
                PrimaryButton("Rename", onClick = {}, compact = true, enabled = false, modifier = Modifier.weight(1f))
            }
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun ButtonsDarkPreview() {
    DotlinkTheme(dark = true) { ButtonsPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun ButtonsLightPreview() {
    DotlinkTheme(dark = false) { ButtonsPreviewContent() }
}
