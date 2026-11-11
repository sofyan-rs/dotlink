/**
 * Drag to choose in a row (or column) of equal steps: [BottomNav], [NavRail] and
 * [SegmentedControl]. The selection indicator follows the finger, ticks as it passes each
 * choice, and on release settles on the nearest one, which is then selected. Taps still go to
 * the choices themselves: a drag only starts past the touch slop.
 */
package com.dotlink.ui.dotline

import android.view.HapticFeedbackConstants
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.AnimationVector1D
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

private val SettleSpring = spring<Float>(dampingRatio = 0.8f, stiffness = Spring.StiffnessMediumLow)

/** Where the indicator is, in px along the row, and whether a finger is moving it. */
@Stable
internal class SlideSelection(
    initial: Float,
) {
    val offset: Animatable<Float, AnimationVector1D> = Animatable(initial)
    var dragging by mutableStateOf(false)
        internal set

    /** The choice under the indicator while dragging, else [selectedIndex]: for the choices' colours. */
    fun highlighted(
        selectedIndex: Int,
        stepPx: Float,
    ): Int = if (dragging && stepPx > 0f) (offset.value / stepPx).roundToInt() else selectedIndex
}

/** Keeps the indicator on [selectedIndex] (sliding there with a spring) whenever no finger holds it. */
@Composable
internal fun rememberSlideSelection(
    selectedIndex: Int,
    stepPx: Float,
): SlideSelection {
    val state = remember { SlideSelection(selectedIndex * stepPx) }
    LaunchedEffect(selectedIndex, stepPx) {
        if (!state.dragging) state.offset.animateTo(selectedIndex * stepPx, SettleSpring)
    }
    return state
}

/** Lets a drag along this element move [state]'s indicator over [count] choices [stepPx] apart. */
@Composable
internal fun Modifier.slideToSelect(
    state: SlideSelection,
    count: Int,
    stepPx: Float,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    vertical: Boolean = false,
): Modifier {
    val view = LocalView.current
    val select by rememberUpdatedState(onSelect)
    val selected by rememberUpdatedState(selectedIndex)
    return pointerInput(count, stepPx, vertical) {
        if (count < 2 || stepPx <= 0f) return@pointerInput
        val max = (count - 1) * stepPx
        coroutineScope {
            var pos = 0f
            var last = 0
            val start = { _: androidx.compose.ui.geometry.Offset ->
                state.dragging = true
                pos = state.offset.value
                last = (pos / stepPx).roundToInt()
            }
            val drag = { delta: Float ->
                pos = (pos + delta).coerceIn(0f, max)
                launch { state.offset.snapTo(pos) }
                val now = (pos / stepPx).roundToInt()
                if (now != last) {
                    last = now
                    // CLOCK_TICK, like Flutter's selectionClick: Compose's SegmentFrequentTick is
                    // Android 14+ only and so faint that many phones (ColorOS…) skip it.
                    view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                }
            }
            val end = {
                val target = (pos / stepPx).roundToInt().coerceIn(0, count - 1)
                state.dragging = false
                launch { state.offset.animateTo(target * stepPx, SettleSpring) }
                if (target != selected) select(target)
            }
            val cancel = {
                state.dragging = false
                launch { state.offset.animateTo(selected * stepPx, SettleSpring) }
                Unit
            }
            if (vertical) {
                detectVerticalDragGestures(onDragStart = start, onDragEnd = end, onDragCancel = cancel) { change, delta ->
                    change.consume()
                    drag(delta)
                }
            } else {
                detectHorizontalDragGestures(onDragStart = start, onDragEnd = end, onDragCancel = cancel) { change, delta ->
                    change.consume()
                    drag(delta)
                }
            }
        }
    }
}
