/**
 * Dotline bottom sheet: a `surface` panel with 28 dp top corners and a five-dot handle,
 * sliding up over the `scrim`. No elevation, no tonal tint.
 *
 * Used by Open-Folder, Folder-Actions, Note-Actions,
 * Vault-Switcher (6.1), Rename and New-Item (sheets that sit above the keyboard).
 *
 * Built on Material 3 `ModalBottomSheet`, which handles the dialog window, drag-to-dismiss,
 * back press and the slide animation. We only restyle it.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.union
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Number of dots in the handle (mockups: five 4 dp dots, 4 dp apart). */
private const val HANDLE_DOTS = 5

/** Space above the handle (mockups: `padding-top: 12px`). */
private val HandleTopPadding = 12.dp

/**
 * A modal bottom sheet in Dotline style. Show it with `if (open) DotlineBottomSheet(...)`;
 * call [onDismissRequest] to set `open = false` (it fires on scrim tap, swipe down and back).
 *
 * The content is a column with 16 dp side padding; children are [spacing] apart. The sheet
 * pads itself above the navigation bar and the keyboard, so text fields stay visible.
 *
 * @param bottomPadding space under the last child: 32 dp for action sheets, 16 dp for sheets
 *   that sit above the keyboard (Rename, New-Item).
 * @param spacing gap between children (16 dp in most sheets, 20 dp in Open-Folder).
 *
 * Swipe-down and scrim tap animate the sheet away before [onDismissRequest] is called. If your
 * own code closes it (e.g. after an action), it disappears at once, which is fine for sheets
 * that hand over to another screen or sheet.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DotlineBottomSheet(
    onDismissRequest: () -> Unit,
    modifier: Modifier = Modifier,
    bottomPadding: Dp = Dotline.space.s6,
    spacing: Dp = Dotline.space.s4,
    content: @Composable ColumnScope.() -> Unit,
) {
    // skipPartiallyExpanded: our sheets are short, so they open fully instead of half-way.
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismissRequest,
        modifier = modifier,
        sheetState = sheetState,
        shape = Dotline.radius.sheet,
        containerColor = Dotline.colors.surface,
        contentColor = Dotline.colors.ink,
        // Depth comes from surface steps: no tonal tint, no shadow.
        tonalElevation = 0.dp,
        scrimColor = Dotline.colors.scrim,
        // The handle slot: Material places it above the content and makes it draggable.
        dragHandle = { SheetDotHandle(Modifier.padding(top = HandleTopPadding, bottom = spacing)) },
        // Keep the sheet above the navigation bar AND the keyboard (the "imePadding" of a sheet).
        contentWindowInsets = { WindowInsets.navigationBars.union(WindowInsets.ime) },
    ) {
        Column(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .padding(start = Dotline.space.s4, end = Dotline.space.s4, bottom = bottomPadding),
            verticalArrangement = Arrangement.spacedBy(spacing),
            content = content,
        )
    }
}

/** The sheet handle: five 4 dp `lineStrong` dots, 4 dp apart, drawn in one Canvas. */
@Composable
fun SheetDotHandle(modifier: Modifier = Modifier) {
    val color = Dotline.colors.lineStrong
    val dot = Dotline.size.dot
    val gap = Dotline.space.s1
    // Width = 5 dots + 4 gaps.
    Canvas(modifier.size(width = dot * HANDLE_DOTS + gap * (HANDLE_DOTS - 1), height = dot)) {
        val radius = dot.toPx() / 2
        val step = (dot + gap).toPx()
        repeat(HANDLE_DOTS) { i ->
            drawCircle(color, radius, center = Offset(radius + i * step, radius))
        }
    }
}

/**
 * Previews can't show a real modal sheet (it lives in its own window), so this draws the
 * same panel inline.
 */
@Composable
private fun SheetPreviewPanel(content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier =
            Modifier
                .fillMaxWidth()
                .background(Dotline.colors.surface, Dotline.radius.sheet)
                .padding(start = Dotline.space.s4, end = Dotline.space.s4, bottom = Dotline.space.s6),
        verticalArrangement = Arrangement.spacedBy(Dotline.space.s4),
    ) {
        SheetDotHandle(
            Modifier
                .align(Alignment.CenterHorizontally)
                .padding(top = HandleTopPadding),
        )
        content()
    }
}

@Composable
private fun SheetPreviewContent() {
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        SheetPreviewPanel {
            SheetHeader(icon = DotlineIcons.Folder, title = "Projects", subtitle = "/Projects · 1 folder · 3 notes")
            Text("Sheet content goes here.", style = Dotline.type.body, color = Dotline.colors.inkMuted)
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun DotlineBottomSheetDarkPreview() {
    DotlinkTheme(dark = true) { SheetPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun DotlineBottomSheetLightPreview() {
    DotlinkTheme(dark = false) { SheetPreviewContent() }
}
