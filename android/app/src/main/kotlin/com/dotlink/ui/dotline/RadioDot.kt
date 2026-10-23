/**
 * Dotline radio button: a round ring; when selected, a solid circle with a
 * small dot inside (Nothing OS 5.0 style).
 *
 * Used on: Move-To (destination folder list, 22 dp) and Vault-Switcher (vault list, 24 dp).
 *
 * 1.5 dp ring. Selected = `ink` filled circle + `onInk` centre dot (10 dp in a 24 dp circle,
 * 9 dp in a 22 dp circle). Unselected = `lineStrong` ring, empty. Disabled (the folder the note is
 * already in) = `line` ring, empty.
 */
package com.dotlink.ui.dotline

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Ring diameter: 24 dp (Vault-Switcher) or 22 dp when `small` (Move-To). */
private val RingSize = 24.dp
private val SmallRingSize = 22.dp

/** The centre dot is always 13–14 dp smaller than the circle: 10 dp in 24, 9 dp in 22. */
private val CentreInset = 14.dp
private val SmallCentreInset = 13.dp

/**
 * A radio button: an empty ring, or a filled circle with a centre dot when selected.
 *
 * @param selected whether this option is the chosen one.
 * @param onClick called when tapped. Pass `null` when the whole list row is the radio button
 *   (the row handles taps and semantics; the dot only draws). This is the usual case.
 * @param enabled when false the ring is dimmed and taps are ignored.
 * @param small true for the 22 dp version used in the Move-To folder list.
 */
@Composable
fun RadioDot(
    selected: Boolean,
    onClick: (() -> Unit)?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    small: Boolean = false,
) {
    val colors = Dotline.colors
    val interactionSource = remember { MutableInteractionSource() }
    val ringSize = if (small) SmallRingSize else RingSize

    val ringColor by animateColorAsState(
        targetValue =
            when {
                selected -> colors.ink
                enabled -> colors.lineStrong
                else -> colors.line
            },
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "ring",
    )
    val fillColor by animateColorAsState(
        targetValue = if (selected) colors.ink else Color.Transparent,
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "fill",
    )
    val centreColor by animateColorAsState(
        targetValue = if (selected) colors.onInk else Color.Transparent,
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "centre",
    )
    val centreInset = if (small) SmallCentreInset else CentreInset

    // Only tappable on its own when it has a callback; a parent row usually handles taps.
    val selectModifier =
        if (onClick != null) {
            Modifier
                .selectable(
                    selected = selected,
                    interactionSource = interactionSource,
                    indication = null,
                    enabled = enabled,
                    role = Role.RadioButton,
                    onClick = onClick,
                ).sizeIn(minWidth = Dotline.size.touchTarget, minHeight = Dotline.size.touchTarget)
        } else {
            Modifier
        }

    Box(modifier = modifier.then(selectModifier), contentAlignment = Alignment.Center) {
        Box(
            modifier =
                Modifier
                    .focusRing(interactionSource, CircleShape)
                    .size(ringSize)
                    .background(fillColor, CircleShape)
                    .border(Dotline.size.stroke, ringColor, CircleShape),
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier
                    .size(ringSize - centreInset)
                    .clip(CircleShape)
                    .background(centreColor),
            )
        }
    }
}

// ---- Previews -------------------------------------------------------------------------------

@Composable
private fun RadioPreviewContent() {
    var picked by remember { mutableIntStateOf(0) }
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            RadioDot(selected = picked == 0, onClick = { picked = 0 })
            RadioDot(selected = picked == 1, onClick = { picked = 1 })
            RadioDot(selected = picked == 2, onClick = { picked = 2 }, small = true)
            RadioDot(selected = false, onClick = {}, enabled = false, small = true)
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun RadioDarkPreview() {
    DotlinkTheme(dark = true) { RadioPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun RadioLightPreview() {
    DotlinkTheme(dark = false) { RadioPreviewContent() }
}
