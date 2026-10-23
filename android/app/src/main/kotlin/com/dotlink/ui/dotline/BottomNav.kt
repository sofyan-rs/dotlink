/**
 * BottomNav: the tab bar at the bottom of the main screens.
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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
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
 * How tall the bottom nav is from the screen's bottom edge: the pill, its margins and the
 * system navigation bar. Tab screens keep this much space free at the end of their lists.
 */
@Composable
fun bottomNavHeight(): Dp =
    Dotline.size.tabBarHeight + Dotline.space.s3 * 2 +
        WindowInsets.navigationBars.asPaddingValues().calculateBottomPadding()

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

@Preview(name = "Light", widthDp = 390)
@Composable
private fun BottomNavLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { BottomNavPreviewContent() }
    }
}
