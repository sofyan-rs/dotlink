/**
 * Sheet header: a 48 dp icon tile (`surfaceRaised`, radius 16) next to a title and a
 * mono subtitle, at the top of an action sheet.
 *
 * Used by Folder-Actions, Note-Actions and Vault-Actions.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Size of the icon tile (mockups: 48×48). */
private val TileSize = 48.dp

/**
 * Header of an action sheet, e.g. folder icon + "Projects" + "/Projects · 1 folder · 3 notes".
 *
 * @param icon what the sheet is about (folder, note, vault). Decorative: the title says it.
 * @param subtitle path or facts joined with ` · `, in the mono `data` style. Null hides it.
 */
@Composable
fun SheetHeader(
    icon: ImageVector,
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
) {
    Row(
        // Mockups: 4 dp inset on top and sides, 12 dp between tile and text.
        modifier = modifier.fillMaxWidth().padding(start = Dotline.space.s1, top = Dotline.space.s1, end = Dotline.space.s1),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3),
    ) {
        Box(
            modifier = Modifier.size(TileSize).background(Dotline.colors.surfaceRaised, Dotline.radius.m),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = null, tint = Dotline.colors.ink, modifier = Modifier.size(Dotline.size.icon))
        }
        // weight(1f) lets long names shrink and end in "…" instead of pushing out of the sheet.
        Column(Modifier.weight(1f)) {
            Text(
                text = title,
                style = Dotline.type.title,
                color = Dotline.colors.ink,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { heading() },
            )
            if (subtitle != null) {
                Text(
                    text = subtitle,
                    style = Dotline.type.data,
                    color = Dotline.colors.inkMuted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun SheetHeaderPreviewContent() {
    Column(
        Modifier.background(Dotline.colors.surface).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SheetHeader(icon = DotlineIcons.Folder, title = "Projects", subtitle = "/Projects · 1 folder · 3 notes")
        SheetHeader(icon = DotlineIcons.Note, title = "Example", subtitle = "Example.md · 645 chars")
        SheetHeader(icon = DotlineIcons.Vault, title = "Work", subtitle = "Documents/Work · 41 notes")
    }
}

@Preview(name = "Dark")
@Composable
private fun SheetHeaderDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { SheetHeaderPreviewContent() }
    }
}

@Preview(name = "Light")
@Composable
private fun SheetHeaderLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { SheetHeaderPreviewContent() }
    }
}
