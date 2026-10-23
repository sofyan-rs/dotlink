/**
 * SectionLabel: the small mono overline above a group ("OPEN NOTES", "VAULT", "MATCHES",
 * "ON THIS PHONE"). `label` style in `inkFaint`.
 *
 * Seen in: Main (2.1), Quick-Switcher (4.1), Vaults (6.2), Git-Sync (7.1).
 *
 * In the mockups the group below sits 8 dp under the label; the screen adds that gap
 * (e.g. `Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2))`).
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/**
 * A section overline. [text] is uppercased for you.
 *
 * It is marked as a heading, so TalkBack users can jump between sections.
 */
@Composable
fun SectionLabel(
    text: String,
    modifier: Modifier = Modifier,
) {
    Text(
        text = text.uppercase(),
        style = Dotline.type.label,
        color = Dotline.colors.inkFaint,
        modifier = modifier.semantics { heading() },
    )
}

@Composable
private fun SectionLabelPreviewContent() {
    Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        SectionLabel("Open notes")
        SectionLabel("Vault")
        SectionLabel("Matches")
    }
}

@Preview(name = "Dark")
@Composable
private fun SectionLabelDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { SectionLabelPreviewContent() }
    }
}

@Preview(name = "Light")
@Composable
private fun SectionLabelLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { SectionLabelPreviewContent() }
    }
}
