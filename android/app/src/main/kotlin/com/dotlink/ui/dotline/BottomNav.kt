/**
 * BottomNav: the tab bar at the bottom of the main screens; [NavRail], the same tabs as a
 * vertical pill on the left in landscape and on wide screens ([LocalNavRail]).
 *
 * 5 tabs: LINK, REMOTE, SHELL, CAMERA, SETTINGS ([MainTab]). Styled after Nothing OS 5.0: a floating
 * pill (`navBar`) with icons only, floating over the screen. The active tab sits in a circle
 * (`navSelected`) that slides between tabs, and is `ink`; inactive tabs are `inkFaint`. The
 * tab names are read out by TalkBack, not shown.
 *
 * Seen in: Main (2.1), Empty-Vault (1.3), Vaults (6.2), Git-Sync (7.1).
 *
 * The bar works with any tab type that implements [BottomNavTab], so tests or other screens can
 * use their own tabs.
 */
package com.dotlink.ui.dotline

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.displayCutout
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.union
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Anything that can be a tab in [BottomNav]: a label (shown in caps) and an icon. */
interface BottomNavTab {
    val label: String
    val icon: ImageVector
}

/** The four main tabs of the app, in bar order. */
enum class MainTab(
    override val label: String,
    override val icon: ImageVector,
) : BottomNavTab {
    Link("LINK", DotlineIcons.Monitor),
    Remote("REMOTE", DotlineIcons.Mouse),
    Shell("SHELL", DotlineIcons.Terminal),
    Camera("CAMERA", DotlineIcons.Camera),
    Settings("SETTINGS", DotlineIcons.Settings),
}

/**
 * The bottom tab bar. It floats over the screen: there is no band behind it, only a soft fade
 * to `bg` so the pill stays readable over the content scrolling under it.
 *
 * @param tabs the tabs in order, e.g. `MainTab.entries`.
 * @param selectedTab the active tab.
 * @param onTabClick called with the tapped tab; navigate there.
 */
@Composable
fun <T : BottomNavTab> BottomNav(
    tabs: List<T>,
    selectedTab: T,
    onTabClick: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    val bg = Dotline.colors.bg
    Box(
        modifier =
            modifier
                .fillMaxWidth()
                // Transparent at the top, fading to `bg` around the pill and solid behind the
                // system navigation bar, so content never shows under the system buttons.
                .background(Brush.verticalGradient(0f to bg.copy(alpha = 0f), 0.55f to bg.copy(alpha = 0.9f), 0.62f to bg))
                // Adds space for the system navigation bar (gesture pill or buttons) below the tabs.
                .windowInsetsPadding(WindowInsets.navigationBars)
                .padding(vertical = Dotline.space.s3),
        contentAlignment = Alignment.Center,
    ) {
        // The active circle slides to the tapped tab (a spring, so it settles softly).
        val selectedIndex = tabs.indexOf(selectedTab).coerceAtLeast(0)
        val circleOffset by animateDpAsState(
            targetValue = (ItemSize + ItemGap) * selectedIndex,
            animationSpec = spring(dampingRatio = 0.8f, stiffness = Spring.StiffnessMediumLow),
            label = "tabCircleOffset",
        )
        Box(
            Modifier
                .height(Dotline.size.tabBarHeight)
                .background(Dotline.colors.navBar, Dotline.radius.pill)
                .padding(PillPadding),
        ) {
            Box(
                Modifier
                    .offset { IntOffset(circleOffset.roundToPx(), 0) }
                    .size(ItemSize)
                    .background(Dotline.colors.navSelected, CircleShape),
            )
            Row(
                // selectableGroup tells TalkBack these tabs belong together ("tab 1 of 4").
                modifier = Modifier.selectableGroup(),
                horizontalArrangement = Arrangement.spacedBy(ItemGap),
            ) {
                tabs.forEach { tab ->
                    BottomNavItem(
                        tab = tab,
                        selected = tab == selectedTab,
                        onClick = { onTabClick(tab) },
                    )
                }
            }
        }
    }
}

/**
 * The tabs are a [NavRail] on the left rather than a [BottomNav]: set by the main screen in
 * landscape and on wide screens (tablets, unfolded foldables), where a bottom bar would take
 * height that's short, or sit far from the hand.
 */
val LocalNavRail = staticCompositionLocalOf { false }

/** Landscape, or at least this wide: the tabs go to a rail on the left. */
val NavRailMinWidth = 600.dp

/**
 * How tall the bottom nav is from the screen's bottom edge: the pill, its margins and the
 * system navigation bar. Tab screens keep this much space free at the end of their lists.
 * With a [NavRail] there is no bar at the bottom; only the system navigation bar's space.
 */
@Composable
fun bottomNavHeight(): Dp {
    val system = WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()
    return if (LocalNavRail.current) system else Dotline.size.tabBarHeight + Dotline.space.s3 * 2 + system
}

/**
 * The tabs as a vertical pill, centred on the left edge: the same pill, circle and items as
 * [BottomNav], turned upright. Content sits to its right, so it needs no fade behind it.
 */
@Composable
fun <T : BottomNavTab> NavRail(
    tabs: List<T>,
    selectedTab: T,
    onTabClick: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier =
            modifier
                .fillMaxHeight()
                .background(Dotline.colors.bg)
                // Clear of the status bar, a cutout on the left, and the system buttons when
                // they sit on this side in landscape.
                .windowInsetsPadding(
                    WindowInsets.statusBars
                        .union(WindowInsets.navigationBars)
                        .union(WindowInsets.displayCutout)
                        .only(WindowInsetsSides.Start + WindowInsetsSides.Vertical),
                ).padding(horizontal = Dotline.space.s3),
        contentAlignment = Alignment.Center,
    ) {
        val selectedIndex = tabs.indexOf(selectedTab).coerceAtLeast(0)
        val circleOffset by animateDpAsState(
            targetValue = (ItemSize + ItemGap) * selectedIndex,
            animationSpec = spring(dampingRatio = 0.8f, stiffness = Spring.StiffnessMediumLow),
            label = "railCircleOffset",
        )
        Box(
            Modifier
                .width(Dotline.size.tabBarHeight)
                .background(Dotline.colors.navBar, Dotline.radius.pill)
                .padding(PillPadding),
        ) {
            Box(
                Modifier
                    .offset { IntOffset(0, circleOffset.roundToPx()) }
                    .size(ItemSize)
                    .background(Dotline.colors.navSelected, CircleShape),
            )
            Column(
                modifier = Modifier.selectableGroup(),
                verticalArrangement = Arrangement.spacedBy(ItemGap),
            ) {
                tabs.forEach { tab ->
                    BottomNavItem(
                        tab = tab,
                        selected = tab == selectedTab,
                        onClick = { onTabClick(tab) },
                    )
                }
            }
        }
    }
}

/** Space between the pill's edge and the circle of the active tab. */
private val PillPadding = 6.dp

/** Each tab is a circle as tall as the pill's inside (64 − 2 × 6 = 52 dp, above the 48 dp touch minimum). */
private val ItemSize = Dotline.size.tabBarHeight - PillPadding * 2

/** Space between two tabs. */
private val ItemGap = Dotline.space.s3

/** One tab: an icon over the sliding circle. */
@Composable
private fun BottomNavItem(
    tab: BottomNavTab,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val interactionSource = remember { MutableInteractionSource() }
    // The icon colour fades while the circle slides under it.
    val iconColor by animateColorAsState(
        targetValue = if (selected) Dotline.colors.ink else Dotline.colors.inkFaint,
        animationSpec = tween(DOTLINE_MOTION_MILLIS * 2),
        label = "tabIcon",
    )
    Box(
        modifier =
            Modifier
                .size(ItemSize)
                .pressScale(interactionSource)
                .focusRing(interactionSource, CircleShape)
                .selectable(
                    selected = selected,
                    interactionSource = interactionSource,
                    indication = null, // No Material ripple.
                    role = Role.Tab,
                    onClick = onClick,
                ).semantics { contentDescription = tab.label },
        contentAlignment = Alignment.Center,
    ) {
        Icon(tab.icon, contentDescription = null, tint = iconColor)
    }
}

@Composable
private fun BottomNavPreviewContent() {
    var selected by remember { mutableStateOf(MainTab.Link) }
    BottomNav(tabs = MainTab.entries, selectedTab = selected, onTabClick = { selected = it })
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun BottomNavDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { BottomNavPreviewContent() }
    }
}

@Preview(name = "Rail", widthDp = 120, heightDp = 420)
@Composable
private fun NavRailPreview() {
    DotlinkTheme(dark = true) {
        var selected by remember { mutableStateOf(MainTab.Remote) }
        NavRail(tabs = MainTab.entries, selectedTab = selected, onTabClick = { selected = it })
    }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun BottomNavLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { BottomNavPreviewContent() }
    }
}
