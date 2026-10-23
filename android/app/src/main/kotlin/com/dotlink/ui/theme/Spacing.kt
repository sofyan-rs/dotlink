/**
 * Dotline spacing (4 dp grid) and fixed sizes.
 */
package com.dotlink.ui.theme

import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

object DotlineSpace {
    /** Label ↔ value, dot-meter gaps. */
    val s1: Dp = 4.dp

    /** Chips, icon ↔ label. */
    val s2: Dp = 8.dp

    /** Between widgets, compact row padding. */
    val s3: Dp = 12.dp

    /** Screen side margin, group padding. */
    val s4: Dp = 16.dp

    /** Between sections. */
    val s5: Dp = 24.dp

    /** Under a dot-m screen title. */
    val s6: Dp = 32.dp

    /** Above a hero numeral. */
    val s7: Dp = 48.dp
}

object DotlineSize {
    /** Minimum size of anything tappable. */
    val touchTarget: Dp = 48.dp
    val dot: Dp = 4.dp
    val dotPitch: Dp = 8.dp
    val stroke: Dp = 1.5.dp

    /** Height of the floating bottom-nav pill. */
    val tabBarHeight: Dp = 64.dp
    val rowMin: Dp = 56.dp
    val icon: Dp = 24.dp
}
