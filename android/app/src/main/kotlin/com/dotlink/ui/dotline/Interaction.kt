/**
 * Shared touch feedback for Dotline components:
 * - [pressScale]: pills shrink to 97% while pressed (120 ms, no bounce).
 * - [focusRing]: a 2 dp ring, 2 dp outside the control, while it has keyboard focus.
 *
 * Both read an `InteractionSource`, the object Compose uses to report press and focus events.
 * Create one with `remember { MutableInteractionSource() }` and pass the same one to `clickable`.
 */
package com.dotlink.ui.dotline

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.drawOutline
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline

/** How long press and toggle animations take (design system: 120–160 ms). */
const val DOTLINE_MOTION_MILLIS = 140

/** Shrinks the element to 97% while it is pressed. */
@Composable
fun Modifier.pressScale(interactionSource: InteractionSource): Modifier {
    val pressed by interactionSource.collectIsPressedAsState()
    val scale by animateFloatAsState(
        targetValue = if (pressed) 0.97f else 1f,
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "pressScale",
    )
    // graphicsLayer scales the drawing only; the layout size stays the same.
    return this.graphicsLayer {
        scaleX = scale
        scaleY = scale
    }
}

/**
 * Draws the focus ring around the element while it has keyboard focus. The ring is drawn
 * outside the element's bounds, so it never changes the layout.
 */
@Composable
fun Modifier.focusRing(
    interactionSource: InteractionSource,
    shape: Shape,
): Modifier {
    val focused by interactionSource.collectIsFocusedAsState()
    val color = Dotline.colors.focus
    return this.drawWithContent {
        drawContent()
        if (focused) {
            val ringWidth = 2.dp.toPx()
            // Ring centre line: 2 dp gap + half the ring width outside the element.
            val inset = 2.dp.toPx() + ringWidth / 2
            val outline = shape.createOutline(Size(size.width + 2 * inset, size.height + 2 * inset), layoutDirection, this)
            translate(-inset, -inset) { drawOutline(outline, color, style = Stroke(ringWidth)) }
        }
    }
}
