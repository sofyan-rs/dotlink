/**
 * Dot art: a small picture drawn as a grid of dots from text rows, for empty states and the
 * lock screen. Each character is one dot:
 * - `X` lit (`ink`)
 * - `R` red signal (`accent`)
 * - `.` unlit (`lineStrong` at 50%)
 * - space: no dot
 *
 * Used by First-Launch, Empty-Vault, No-Results, Unlock.
 * The patterns from those mockups are in [DotArtPatterns].
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Opacity of unlit dots (design system: `lineStrong` at 50%). */
private const val UNLIT_ALPHA = 0.5f

/** Default dot size in empty-state art (mockups: 8 dp; No-Results uses 6 dp). */
private val DefaultDotSize = 8.dp

/**
 * Draws [pattern] as dots, one Canvas for the whole grid.
 *
 * @param pattern rows of `X`, `R`, `.` and spaces; shorter rows are fine.
 * @param contentDescription what the picture shows, e.g. "Locked". Null if it is decorative.
 * @param dotSize diameter of one dot. The gap between dots is always 4 dp.
 */
@Composable
fun DotArt(
    pattern: List<String>,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    dotSize: Dp = DefaultDotSize,
) {
    val lit = Dotline.colors.ink
    val red = Dotline.colors.accent
    val unlit = Dotline.colors.lineStrong.copy(alpha = UNLIT_ALPHA)
    val gap = Dotline.space.s1
    val rows = pattern.size
    val columns = pattern.maxOfOrNull { it.length } ?: 0

    // Only add image semantics when there is something to say; otherwise TalkBack skips it.
    val semanticsModifier =
        if (contentDescription != null) {
            Modifier.semantics {
                this.contentDescription = contentDescription
                role = Role.Image
            }
        } else {
            Modifier
        }

    Canvas(
        modifier
            .then(semanticsModifier)
            .size(width = gridSize(columns, dotSize, gap), height = gridSize(rows, dotSize, gap)),
    ) {
        val radius = dotSize.toPx() / 2
        val step = (dotSize + gap).toPx()
        pattern.forEachIndexed { y, row ->
            row.forEachIndexed { x, char ->
                val color =
                    when (char) {
                        'X' -> lit
                        'R' -> red
                        '.' -> unlit
                        else -> null // space (or anything else): leave a hole
                    }
                if (color != null) {
                    drawCircle(color, radius, center = Offset(radius + x * step, radius + y * step))
                }
            }
        }
    }
}

/** Width or height of [count] dots with [gap] between them. */
private fun gridSize(
    count: Int,
    dot: Dp,
    gap: Dp,
): Dp = if (count > 0) dot * count + gap * (count - 1) else 0.dp

/** The dot pictures from the mockups (copied from each screen's `rows` script data). */
object DotArtPatterns {
    /** A note page with a folded corner and a red caret. First-Launch (1.1). */
    val NotePage: List<String> =
        listOf(
            "XXXXXX...",
            "X....XX..",
            "X....X.X.",
            "X....XXXX",
            "X.......X",
            "X.XXXX..X",
            "X.......X",
            "X.XXXXX.X",
            "X.......X",
            "X.XXR...X",
            "XXXXXXXXX",
        )

    /** An empty folder. Empty-Vault (1.3). */
    val EmptyFolder: List<String> =
        listOf(
            "XXXX.......",
            "X...X......",
            "X....XXXXXX",
            "X.........X",
            "X.........X",
            "X.........X",
            "X.........X",
            "XXXXXXXXXXX",
        )

    /** A magnifier. No-Results (4.2); the mockup draws it with 6 dp dots (`dotSize = 6.dp`). */
    val Magnifier: List<String> =
        listOf(
            "..XXXX...",
            ".X....X..",
            "X......X.",
            "X......X.",
            "X......X.",
            ".X....X..",
            "..XXXXX..",
            "......XX.",
            ".......XX",
        )

    /** A padlock with a red keyhole. Unlock (5.2). */
    val Lock: List<String> =
        listOf(
            "...XXX...",
            "..X...X..",
            "..X...X..",
            ".XXXXXXX.",
            ".XXXXXXX.",
            ".XXXRXXX.",
            ".XXXRXXX.",
            ".XXXXXXX.",
            ".XXXXXXX.",
        )
}

@Composable
private fun DotArtPreviewContent() {
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(24.dp), verticalAlignment = Alignment.CenterVertically) {
            DotArt(DotArtPatterns.NotePage, contentDescription = "A note")
            DotArt(DotArtPatterns.EmptyFolder, contentDescription = "Empty folder")
            DotArt(DotArtPatterns.Magnifier, contentDescription = "Nothing found", dotSize = 6.dp)
            DotArt(DotArtPatterns.Lock, contentDescription = "Locked")
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun DotArtDarkPreview() {
    DotlinkTheme(dark = true) { DotArtPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun DotArtLightPreview() {
    DotlinkTheme(dark = false) { DotArtPreviewContent() }
}
