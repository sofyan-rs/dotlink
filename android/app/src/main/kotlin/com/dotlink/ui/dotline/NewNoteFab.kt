/**
 * "New note" floating button: a 56 dp `ink` pill with a plus icon, bottom-right on the file
 * list, 12 dp above the bottom navigation.
 *
 * Used on the Files tab. The caller positions it (e.g. `Modifier.align(BottomEnd)
 * .padding(end = 16.dp, bottom = 12.dp)` inside a Box above the BottomNav).
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Height of the pill (mockup: 56 dp, the big-button height). */
private val FabHeight = 56.dp

/** Plus icon size (mockup: 22 dp). */
private val FabIconSize = 22.dp

/** Space before the icon (mockup: `padding: 0 24px 0 20px`). */
private val FabStartPadding = 20.dp

/**
 * The "New note" pill. Pressing it shrinks it to 97%; it shows the focus ring when focused.
 *
 * @param text button label; "New note" in the mockup.
 */
@Composable
fun NewNoteFab(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    text: String = "New note",
) {
    // Shared by clickable, pressScale and focusRing so all three see the same touches.
    val interactionSource = remember { MutableInteractionSource() }
    val shape = Dotline.radius.pill
    Row(
        // Modifier order matters: scale and ring wrap the pill, then fill, then click, then padding.
        modifier =
            modifier
                .pressScale(interactionSource)
                .focusRing(interactionSource, shape)
                .height(FabHeight)
                .background(Dotline.colors.ink, shape)
                .clickable(interactionSource = interactionSource, indication = null, role = Role.Button, onClick = onClick)
                .padding(start = FabStartPadding, end = Dotline.space.s5),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
    ) {
        Icon(DotlineIcons.Plus, contentDescription = null, tint = Dotline.colors.onInk, modifier = Modifier.size(FabIconSize))
        Text(text = text, style = Dotline.type.body.copy(fontWeight = FontWeight.Medium), color = Dotline.colors.onInk)
    }
}

@Preview(name = "Dark")
@Composable
private fun NewNoteFabDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { NewNoteFab(onClick = {}) }
    }
}

@Preview(name = "Light")
@Composable
private fun NewNoteFabLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { NewNoteFab(onClick = {}) }
    }
}
