/**
 * Dotline on/off switch.
 *
 * Used on: Git-Sync ("Auto-sync" row).
 *
 * From the mockup: a 52×32 dp pill with a 1.5 dp border and a 23 dp round thumb, 3 dp inside
 * the border. On = `accent` track and border, white (`onAccent`) thumb on the right.
 * Off = `surfaceRaised` track, `lineStrong` border, `inkMuted` thumb on the left.
 * Colours and thumb position animate over [DOTLINE_MOTION_MILLIS].
 */
package com.dotlink.ui.dotline

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Track size (mockup: 52×32 dp). */
private val TrackWidth = 52.dp
private val TrackHeight = 32.dp

/** Thumb diameter: nearly as tall as the track (Nothing OS 5.0 style). */
private val ThumbSize = 26.dp

/** Thumb distance from the track's outer edge. */
private val ThumbInset = 3.dp

/** How far the thumb slides: track width − thumb − both insets = 20 dp. */
private val ThumbTravel = TrackWidth - ThumbSize - ThumbInset * 2

/** Opacity of a disabled switch. There is no "disabled" colour token, so we fade it. */
private const val DISABLED_ALPHA = 0.4f

/**
 * An on/off switch.
 *
 * @param checked whether the switch is on.
 * @param onCheckedChange called with the new value when tapped. Pass `null` when the whole
 *   list row is the toggle (the row handles taps and semantics; the switch only draws).
 * @param enabled when false the switch is faded and ignores taps.
 * @param contentDescription what the switch controls ("Auto-sync"), read by screen readers.
 *   Leave `null` if a visible label next to it is already part of the same toggleable row.
 */
@Composable
fun KuSwitch(
    checked: Boolean,
    onCheckedChange: ((Boolean) -> Unit)?,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    contentDescription: String? = null,
) {
    val colors = Dotline.colors
    val shape = Dotline.radius.pill
    val interactionSource = remember { MutableInteractionSource() }

    val animation = tween<Color>(DOTLINE_MOTION_MILLIS)
    // On: solid `ink` track with an `onInk` thumb. Off: `surfaceRaised` track with an `inkMuted` thumb.
    val trackColor by animateColorAsState(if (checked) colors.ink else colors.surfaceRaised, animation, label = "track")
    val borderColor by animateColorAsState(if (checked) colors.ink else colors.lineStrong, animation, label = "border")
    val thumbColor by animateColorAsState(if (checked) colors.onInk else colors.inkMuted, animation, label = "thumb")
    val thumbOffset by animateDpAsState(
        targetValue = if (checked) ThumbInset + ThumbTravel else ThumbInset,
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "thumbOffset",
    )

    // Only make it tappable when there is a callback; otherwise a parent row handles taps.
    val toggleModifier =
        if (onCheckedChange != null) {
            Modifier.toggleable(
                value = checked,
                interactionSource = interactionSource,
                indication = null,
                enabled = enabled,
                role = Role.Switch,
                onValueChange = onCheckedChange,
            )
        } else {
            Modifier
        }

    // Outer box: at least 48×48 dp so it is easy to hit; the 52×32 track sits in the middle.
    Box(
        modifier =
            modifier
                .then(toggleModifier)
                .then(if (contentDescription != null) Modifier.semantics { this.contentDescription = contentDescription } else Modifier)
                .sizeIn(minWidth = Dotline.size.touchTarget, minHeight = Dotline.size.touchTarget)
                .alpha(if (enabled) 1f else DISABLED_ALPHA),
        contentAlignment = Alignment.Center,
    ) {
        // Track. The thumb is placed with `offset` from the track's top-left corner.
        Box(
            modifier =
                Modifier
                    .focusRing(interactionSource, shape)
                    .size(TrackWidth, TrackHeight)
                    .clip(shape)
                    .background(trackColor)
                    .border(Dotline.size.stroke, borderColor, shape),
        ) {
            Box(
                Modifier
                    // The lambda form of offset reads the animated value while placing, not while
                    // composing, so the animation doesn't recompose the switch on every frame.
                    .offset { IntOffset(thumbOffset.roundToPx(), ThumbInset.roundToPx()) }
                    .size(ThumbSize)
                    .clip(CircleShape)
                    .background(thumbColor),
            )
        }
    }
}

// ---- Previews -------------------------------------------------------------------------------

@Composable
private fun SwitchPreviewContent() {
    var on by remember { mutableStateOf(true) }
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            KuSwitch(checked = on, onCheckedChange = { on = it }, contentDescription = "Auto-sync")
            KuSwitch(checked = !on, onCheckedChange = { on = !it }, contentDescription = "Auto-sync")
            KuSwitch(checked = true, onCheckedChange = {}, enabled = false)
            KuSwitch(checked = false, onCheckedChange = {}, enabled = false)
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun SwitchDarkPreview() {
    DotlinkTheme(dark = true) { SwitchPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun SwitchLightPreview() {
    DotlinkTheme(dark = false) { SwitchPreviewContent() }
}
