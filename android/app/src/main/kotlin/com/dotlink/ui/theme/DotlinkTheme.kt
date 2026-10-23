/**
 * The app theme: Dotline values for our own components, mapped onto Material 3 so stock
 * components (text selection, ripples, dialogs) pick up the right colours too.
 *
 * Usage: wrap the UI in `DotlinkTheme { ... }`, then read `Dotline.colors`, `Dotline.type`,
 * `Dotline.space`, `Dotline.radius`, `Dotline.size`.
 */
package com.dotlink.ui.theme

import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.staticCompositionLocalOf

// "CompositionLocals" pass a value down the UI tree without passing it as a parameter.
private val LocalDotlineColors = staticCompositionLocalOf { DotlineDark }
private val LocalDotlineType = staticCompositionLocalOf { DotlineType() }

/** Shortcut to the current Dotline values, e.g. `Dotline.colors.ink`. */
object Dotline {
    val colors: DotlineColors
        @Composable @ReadOnlyComposable
        get() = LocalDotlineColors.current

    val type: DotlineType
        @Composable @ReadOnlyComposable
        get() = LocalDotlineType.current

    val space = DotlineSpace
    val radius = DotlineRadius
    val size = DotlineSize
}

/** [dark] picks the Dotline dark or light colours; callers pass `isAppInDarkTheme()`. */
@Composable
fun DotlinkTheme(
    dark: Boolean = true,
    content: @Composable () -> Unit,
) {
    val colors = if (dark) DotlineDark else DotlineLight
    CompositionLocalProvider(
        LocalDotlineColors provides colors,
        LocalDotlineType provides DotlineType(),
    ) {
        MaterialTheme(
            colorScheme = materialColors(colors, dark),
            shapes =
                Shapes(
                    small = DotlineRadius.s,
                    medium = DotlineRadius.m,
                    large = DotlineRadius.l,
                    extraLarge = DotlineRadius.l,
                ),
            content = content,
        )
    }
}

/** Depth comes from surface steps, so every Material surface colour maps to a Dotline surface. */
private fun materialColors(
    c: DotlineColors,
    dark: Boolean,
): ColorScheme =
    if (dark) {
        darkColorScheme(
            background = c.bg,
            onBackground = c.ink,
            surface = c.bg,
            onSurface = c.ink,
            surfaceContainer = c.surface,
            surfaceContainerHigh = c.surfaceRaised,
            surfaceVariant = c.surfaceRaised,
            onSurfaceVariant = c.inkMuted,
            primary = c.ink,
            onPrimary = c.onInk,
            secondary = c.inkMuted,
            onSecondary = c.onInk,
            error = c.accent,
            onError = c.onAccent,
            outline = c.lineStrong,
            outlineVariant = c.line,
            scrim = c.scrim,
        )
    } else {
        lightColorScheme(
            background = c.bg,
            onBackground = c.ink,
            surface = c.bg,
            onSurface = c.ink,
            surfaceContainer = c.surface,
            surfaceContainerHigh = c.surfaceRaised,
            surfaceVariant = c.surfaceRaised,
            onSurfaceVariant = c.inkMuted,
            primary = c.ink,
            onPrimary = c.onInk,
            secondary = c.inkMuted,
            onSecondary = c.onInk,
            error = c.accent,
            onError = c.onAccent,
            outline = c.lineStrong,
            outlineVariant = c.line,
            scrim = c.scrim,
        )
    }
