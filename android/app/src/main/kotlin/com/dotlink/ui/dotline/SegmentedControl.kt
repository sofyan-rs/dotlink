/**
 * Dotline segmented control: a pill "well" with two or three segments, one selected.
 *
 * Used on: the editor (LIVE | SOURCE | PREVIEW), New-Item (NOTE | FOLDER) and the Vaults tab
 * (THEME: SYSTEM | LIGHT | DARK).
 *
 * From the mockups: well = `surfaceRaised` pill with 4 dp padding and 4 dp between segments;
 * each segment is 40 dp tall (so the whole control is a 48 dp touch target), `label` text with
 * an optional 16 dp icon 6 dp before it. Selected = `ink` pill + `onInk` text, others
 * `inkMuted`. The pill slides to a newly selected segment.
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
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Padding inside the well and gap between segments (mockups: 4 dp). */
private val WellPadding = 4.dp

/** Segment height: 40 dp + 2 × 4 dp well padding = a 48 dp touch target. */
private val SegmentHeight = 40.dp

/** Segment icon size and gap to the text (mockups: 16 dp, 6 dp). */
private val SegmentIconSize = 16.dp
private val SegmentIconGap = 6.dp

/**
 * One choice in a [SegmentedControl].
 *
 * @param label the text; it is shown in uppercase ("Live" → "LIVE").
 * @param icon optional line icon shown before the label.
 */
@Immutable
data class SegmentOption(
    val label: String,
    val icon: ImageVector? = null,
)

/**
 * A row of mutually exclusive choices. Stateless: the caller keeps the selected index and
 * updates it in [onSelect].
 *
 * @param options the segments, left to right (all get the same width).
 * @param selectedIndex index into [options] of the selected segment.
 * @param onSelect called with the index of the tapped segment.
 */
@Composable
fun SegmentedControl(
    options: List<SegmentOption>,
    selectedIndex: Int,
    onSelect: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    // BoxWithConstraints gives the well's inner width, so we know how wide one segment is.
    BoxWithConstraints(
        modifier =
            modifier
                .clip(Dotline.radius.pill)
                .background(Dotline.colors.surfaceRaised)
                .padding(WellPadding),
    ) {
        val count = options.size.coerceAtLeast(1)
        val segmentWidth = (maxWidth - WellPadding * (count - 1)) / count
        // One `ink` pill slides under the selected segment (a spring, so it settles softly).
        val pillOffset by animateDpAsState(
            targetValue = (segmentWidth + WellPadding) * selectedIndex,
            animationSpec = spring(dampingRatio = 0.8f, stiffness = Spring.StiffnessMediumLow),
            label = "segmentPill",
        )
        Box(
            Modifier
                .offset { IntOffset(pillOffset.roundToPx(), 0) }
                .width(segmentWidth)
                .height(SegmentHeight)
                .background(Dotline.colors.ink, Dotline.radius.pill),
        )
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    // Tells accessibility services that these segments belong together (one choice).
                    .selectableGroup(),
            horizontalArrangement = Arrangement.spacedBy(WellPadding),
        ) {
            options.forEachIndexed { index, option ->
                Segment(
                    option = option,
                    selected = index == selectedIndex,
                    onClick = { onSelect(index) },
                )
            }
        }
    }
}

/** One segment (its pill is drawn by [SegmentedControl]). An extension on RowScope so it can use `Modifier.weight`. */
@Composable
private fun RowScope.Segment(
    option: SegmentOption,
    selected: Boolean,
    onClick: () -> Unit,
) {
    val colors = Dotline.colors
    val shape = Dotline.radius.pill
    val interactionSource = remember { MutableInteractionSource() }
    // animate*AsState smoothly moves to the new colour while the pill slides under the segment.
    val content by animateColorAsState(
        targetValue = if (selected) colors.onInk else colors.inkMuted,
        animationSpec = tween(DOTLINE_MOTION_MILLIS * 2),
        label = "segmentContent",
    )

    Row(
        modifier =
            Modifier
                .weight(1f)
                .pressScale(interactionSource)
                .focusRing(interactionSource, shape)
                .height(SegmentHeight)
                .clip(shape)
                // selectable = clickable + "selected" semantics for screen readers.
                .selectable(
                    selected = selected,
                    interactionSource = interactionSource,
                    indication = null,
                    role = Role.Tab,
                    onClick = onClick,
                ).padding(horizontal = Dotline.space.s2),
        horizontalArrangement = Arrangement.spacedBy(SegmentIconGap, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (option.icon != null) {
            Icon(
                imageVector = option.icon,
                contentDescription = null,
                tint = content,
                modifier = Modifier.size(SegmentIconSize),
            )
        }
        Text(
            text = option.label.uppercase(),
            style = Dotline.type.label,
            color = content,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

// ---- Previews -------------------------------------------------------------------------------

@Composable
private fun SegmentedPreviewContent() {
    var mode by remember { mutableIntStateOf(0) }
    var kind by remember { mutableIntStateOf(1) }
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            SegmentedControl(
                options =
                    listOf(
                        SegmentOption("Live", DotlineIcons.Edit),
                        SegmentOption("Source", DotlineIcons.Code),
                        SegmentOption("Preview", DotlineIcons.Eye),
                    ),
                selectedIndex = mode,
                onSelect = { mode = it },
                modifier = Modifier.fillMaxWidth(),
            )
            SegmentedControl(
                options = listOf(SegmentOption("Note", DotlineIcons.Note), SegmentOption("Folder", DotlineIcons.Folder)),
                selectedIndex = kind,
                onSelect = { kind = it },
                modifier = Modifier.fillMaxWidth(),
            )
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun SegmentedDarkPreview() {
    DotlinkTheme(dark = true) { SegmentedPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun SegmentedLightPreview() {
    DotlinkTheme(dark = false) { SegmentedPreviewContent() }
}
