/**
 * Dot meter: a row of 4 dp dots, 4 dp apart. Lit dots are `ink`, unlit dots `lineStrong` at
 * 50% opacity. Shows a level, e.g. password strength (9 of 12 lit = strong).
 *
 * Used by Set-Password, next to the "STRONG · 16 CHARS" label.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Opacity of unlit dots (design system: `lineStrong` at 50%). */
private const val UNLIT_ALPHA = 0.5f

/**
 * A row of [count] dots with the first [lit] ones lit.
 *
 * @param contentDescription what the meter measures, e.g. "Password strength". TalkBack reads
 *   it together with the level.
 */
@Composable
fun DotMeter(
    count: Int,
    lit: Int,
    contentDescription: String,
    modifier: Modifier = Modifier,
) {
    val litColor = Dotline.colors.ink
    val unlitColor = Dotline.colors.lineStrong.copy(alpha = UNLIT_ALPHA)
    val dot = Dotline.size.dot
    val gap = Dotline.space.s1
    val litCount = lit.coerceIn(0, count)
    // One Canvas for all dots is cheaper than one composable per dot.
    Canvas(
        modifier =
            modifier
                .size(width = if (count > 0) dot * count + gap * (count - 1) else 0.dp, height = dot)
                // Reported to accessibility as a progress bar ("9 of 12").
                .semantics {
                    this.contentDescription = contentDescription
                    progressBarRangeInfo = ProgressBarRangeInfo(litCount.toFloat(), 0f..count.toFloat().coerceAtLeast(1f))
                },
    ) {
        val radius = dot.toPx() / 2
        val step = (dot + gap).toPx()
        repeat(count) { i ->
            drawCircle(
                color = if (i < litCount) litColor else unlitColor,
                radius = radius,
                center = Offset(radius + i * step, radius),
            )
        }
    }
}

@Composable
private fun DotMeterPreviewContent() {
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            DotMeter(count = 12, lit = 3, contentDescription = "Password strength")
            DotMeter(count = 12, lit = 9, contentDescription = "Password strength")
            DotMeter(count = 12, lit = 12, contentDescription = "Password strength")
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun DotMeterDarkPreview() {
    DotlinkTheme(dark = true) { DotMeterPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun DotMeterLightPreview() {
    DotlinkTheme(dark = false) { DotMeterPreviewContent() }
}
