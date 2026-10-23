/**
 * Dotline checkbox (checkboxes have a `lineStrong` edge) and [DotlineCheckboxRow],
 * a checkbox with its label that is tappable as a whole.
 *
 * Used on: Set-Password ("Unlock with fingerprint"), Unlock ("Unlock with fingerprint next time"),
 * Remove-Vault ("Also delete the folder", with a supporting line and the red variant).
 *
 * A 22 dp box with 6 dp corners (Nothing OS 5.0 style), 12 dp before the label, row at least
 * 48 dp tall, label in `body`. Checked = filled box with a check mark: `ink` fill normally, `accent` fill for a
 * destructive choice (Remove-Vault uses `accent-color: #e5211c`).
 */
package com.dotlink.ui.dotline

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Box size and its corner radius (Nothing OS 5.0: a soft rounded square). */
private val BoxSize = 22.dp
private val BoxShape = RoundedCornerShape(6.dp)

/** Check mark size inside the box. */
private val CheckSize = 16.dp

/** Opacity of a disabled checkbox. There is no "disabled" colour token, so we fade it. */
private const val DISABLED_ALPHA = 0.4f

/**
 * A square checkbox.
 *
 * @param checked whether it is ticked.
 * @param onCheckedChange called with the new value when tapped. Pass `null` when a parent
 *   (like [DotlineCheckboxRow]) handles taps; then the box only draws.
 * @param enabled when false it is faded and ignores taps.
 * @param destructive true to fill it with `accent` instead of `ink` (an option that deletes data).
 */
@Composable
fun DotlineCheckbox(
    checked: Boolean,
    onCheckedChange: ((Boolean) -> Unit)?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    destructive: Boolean = false,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val toggleModifier =
        if (onCheckedChange != null) {
            Modifier
                .toggleable(
                    value = checked,
                    interactionSource = interactionSource,
                    indication = null,
                    enabled = enabled,
                    role = Role.Checkbox,
                    onValueChange = onCheckedChange,
                ).sizeIn(minWidth = Dotline.size.touchTarget, minHeight = Dotline.size.touchTarget)
        } else {
            Modifier
        }
    Box(
        modifier = modifier.then(toggleModifier).alpha(if (enabled) 1f else DISABLED_ALPHA),
        contentAlignment = Alignment.Center,
    ) {
        CheckboxBox(
            checked = checked,
            destructive = destructive,
            modifier = Modifier.focusRing(interactionSource, BoxShape),
        )
    }
}

/**
 * A checkbox with a label (and an optional second line) where the whole row is one tap target.
 *
 * @param text the label, sentence case ("Unlock with fingerprint").
 * @param checked whether it is ticked.
 * @param onCheckedChange called with the new value when the row is tapped.
 * @param supportingText optional smaller grey line under the label (Remove-Vault).
 * @param enabled when false the row is faded and ignores taps.
 * @param destructive true for the red box (see [DotlineCheckbox]).
 */
@Composable
fun DotlineCheckboxRow(
    text: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    supportingText: String? = null,
    enabled: Boolean = true,
    destructive: Boolean = false,
) {
    val interactionSource = remember { MutableInteractionSource() }
    // With a second line the box lines up with the first line (top), otherwise it is centred.
    val twoLines = supportingText != null

    Row(
        modifier =
            modifier
                .focusRing(interactionSource, Dotline.radius.s)
                // toggleable on the Row: tapping the label ticks the box too, and screen readers
                // announce "checkbox, ticked, Unlock with fingerprint" as one item.
                .toggleable(
                    value = checked,
                    interactionSource = interactionSource,
                    indication = null,
                    enabled = enabled,
                    role = Role.Checkbox,
                    onValueChange = onCheckedChange,
                ).heightIn(min = Dotline.size.touchTarget)
                .alpha(if (enabled) 1f else DISABLED_ALPHA)
                .padding(top = if (twoLines) Dotline.space.s1 else 0.dp),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3),
        verticalAlignment = if (twoLines) Alignment.Top else Alignment.CenterVertically,
    ) {
        // 1 dp down so the box sits on the 22 sp first line (mockup: margin-top 1px).
        CheckboxBox(
            checked = checked,
            destructive = destructive,
            modifier = Modifier.padding(top = if (twoLines) 1.dp else 0.dp),
        )
        Column {
            Text(text = text, style = Dotline.type.body, color = Dotline.colors.ink)
            if (supportingText != null) {
                Text(text = supportingText, style = Dotline.type.bodyS, color = Dotline.colors.inkMuted)
            }
        }
    }
}

/** The drawn 22 dp box, shared by both composables above. */
@Composable
private fun CheckboxBox(
    checked: Boolean,
    destructive: Boolean,
    modifier: Modifier = Modifier,
) {
    val colors = Dotline.colors
    val onColor = if (destructive) colors.accent else colors.ink
    val fill by animateColorAsState(
        targetValue = if (checked) onColor else Color.Transparent,
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "checkboxFill",
    )
    val edge by animateColorAsState(
        targetValue = if (checked) onColor else colors.lineStrong,
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "checkboxEdge",
    )
    Box(
        modifier =
            modifier
                .size(BoxSize)
                .clip(BoxShape)
                .background(fill)
                .border(Dotline.size.stroke, edge, BoxShape),
        contentAlignment = Alignment.Center,
    ) {
        if (checked) {
            Icon(
                imageVector = DotlineIcons.Check,
                // The checked state is announced by the toggleable semantics, not by this icon.
                contentDescription = null,
                tint = if (destructive) colors.onAccent else colors.onInk,
                modifier = Modifier.size(CheckSize),
            )
        }
    }
}

// ---- Previews -------------------------------------------------------------------------------

@Composable
private fun CheckboxPreviewContent() {
    var fingerprint by remember { mutableStateOf(true) }
    var deleteFolder by remember { mutableStateOf(false) }
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Column {
            Row(verticalAlignment = Alignment.CenterVertically) {
                DotlineCheckbox(checked = fingerprint, onCheckedChange = { fingerprint = it })
                DotlineCheckbox(checked = true, onCheckedChange = {}, destructive = true)
                DotlineCheckbox(checked = false, onCheckedChange = {}, enabled = false)
            }
            DotlineCheckboxRow("Unlock with fingerprint", checked = fingerprint, onCheckedChange = { fingerprint = it })
            DotlineCheckboxRow(
                text = "Also delete the folder",
                supportingText = "Moves Documents/Work to trash. Notes synced with git stay in the repository.",
                checked = deleteFolder,
                onCheckedChange = { deleteFolder = it },
                destructive = true,
            )
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun CheckboxDarkPreview() {
    DotlinkTheme(dark = true) { CheckboxPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun CheckboxLightPreview() {
    DotlinkTheme(dark = false) { CheckboxPreviewContent() }
}
