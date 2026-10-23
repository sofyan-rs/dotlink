/**
 * Dotline colours.
 *
 * Read them in composables with `Dotline.colors.ink`, never with hard-coded `Color(...)`.
 */
package com.dotlink.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.graphics.Color

@Immutable
data class DotlineColors(
    /** Screen background. */
    val bg: Color,
    /** List groups, cards, sheets. */
    val surface: Color,
    /** Inputs, pressed or selected rows, segmented-control well. */
    val surfaceRaised: Color,
    /**
     * Hairlines between rows. Semi-transparent, so the same token shows on `bg`, `surface` and
     * `surfaceRaised` alike (a solid grey vanishes on a grey sheet group).
     */
    val line: Color,
    /** Edges of controls: secondary buttons, chips, checkboxes. */
    val lineStrong: Color,
    /** Primary text and icons; fill of primary buttons. */
    val ink: Color,
    /** Secondary text. */
    val inkMuted: Color,
    /** Captions and inactive tab labels (only on bg or surface). */
    val inkFaint: Color,
    /** Text on an `ink` fill. */
    val onInk: Color,
    /** Signal only: caret, active dot, switch on, destructive fill. At most once or twice per screen. */
    val accent: Color,
    /** Text on an `accent` fill. */
    val onAccent: Color,
    /** Red text (destructive rows, errors). */
    val accentText: Color,
    /** Synced, match. Always paired with a word or glyph. */
    val positive: Color,
    /** Focus ring. */
    val focus: Color,
    /** Behind sheets and dialogs. */
    val scrim: Color,
    /** The floating bottom-nav pill. */
    val navBar: Color,
    /** The circle behind the active tab in the bottom nav. */
    val navSelected: Color,
)

val DotlineDark =
    DotlineColors(
        bg = Color(0xFF000000),
        surface = Color(0xFF111111),
        surfaceRaised = Color(0xFF1C1C1C),
        line = Color(0x1FFFFFFF), // white at 12%
        lineStrong = Color(0xFF4A4A4A),
        ink = Color(0xFFFFFFFF),
        inkMuted = Color(0xFFA6A6A6),
        inkFaint = Color(0xFF808080),
        onInk = Color(0xFF000000),
        accent = Color(0xFFE5211C),
        onAccent = Color(0xFFFFFFFF),
        accentText = Color(0xFFFF5A52),
        positive = Color(0xFF4AD991),
        focus = Color(0xFFFFFFFF),
        scrim = Color(0xB8000000), // rgba(0,0,0,0.72)
        navBar = Color(0xFF1C1C1C),
        navSelected = Color(0xFF3A3A3A),
    )

val DotlineLight =
    DotlineColors(
        bg = Color(0xFFF2F2F2),
        surface = Color(0xFFFFFFFF),
        surfaceRaised = Color(0xFFE8E8E8),
        line = Color(0x1A000000), // black at 10%
        lineStrong = Color(0xFFBDBDBD),
        ink = Color(0xFF000000),
        inkMuted = Color(0xFF4D4D4D),
        inkFaint = Color(0xFF6B6B6B),
        onInk = Color(0xFFFFFFFF),
        accent = Color(0xFFD70015),
        onAccent = Color(0xFFFFFFFF),
        accentText = Color(0xFFD70015),
        positive = Color(0xFF0B6B3C),
        focus = Color(0xFF000000),
        scrim = Color(0x66000000), // rgba(0,0,0,0.4)
        navBar = Color(0xFFE2E2E2),
        navSelected = Color(0xFFFFFFFF),
    )
