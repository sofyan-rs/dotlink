/**
 * Dotline corner radii. Buttons, chips, switches and segmented controls are pills.
 */
package com.dotlink.ui.theme

import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.unit.dp

object DotlineRadius {
    /** Segments, thumbnails. */
    val s = RoundedCornerShape(8.dp)

    /** List groups, inputs, cards. */
    val m = RoundedCornerShape(16.dp)

    /** Widgets, dialogs. */
    val l = RoundedCornerShape(28.dp)

    /** Top corners of a bottom sheet. */
    val sheet = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp)

    /** Fully rounded ends. */
    val pill = RoundedCornerShape(percent = 50)
}
