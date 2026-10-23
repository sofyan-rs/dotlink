/**
 * OpenNoteCard: a small card for one open note, the phone's replacement for desktop tabs
 * ("OPEN NOTES" on the Files tab).
 *
 * 148×88 dp, `surface`, radius 16: icon at the top (note or lock), a red dot at the top right
 * when it is the note being edited, then the name and a mono caption (EDITING / 1 H AGO / LOCKED).
 *
 * Also in this file: [OpenNotesRow], the horizontal scrolling row of cards.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Card size from the Main mockup. */
private val CardWidth = 148.dp
private val CardHeight = 88.dp

/** The card icon is 20 dp (Main mockup). */
private val CardIconSize = 20.dp

/**
 * One open-note card.
 *
 * @param name note name, one line.
 * @param caption mono caption under the name, e.g. "EDITING", "1 H AGO", "LOCKED" (uppercased for you).
 * @param onClick open the note.
 * @param icon [DotlineIcons.Note] by default; pass [DotlineIcons.Lock] for a locked note.
 * @param isCurrent the note being edited: shows the red dot.
 * @param onLongClick long press (later: close, pin).
 */
@Composable
fun OpenNoteCard(
    name: String,
    caption: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector = DotlineIcons.Note,
    isCurrent: Boolean = false,
    /** Pinned notes stay at the front of the row and aren't closed by "Close others". */
    isPinned: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    /** What a long press does, read by TalkBack ("Actions"). */
    onLongClickLabel: String? = null,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val pressed by interactionSource.collectIsPressedAsState()
    val background = if (pressed) Dotline.colors.surfaceRaised else Dotline.colors.surface
    Column(
        // Order: focus ring before clip, so the ring (drawn outside the card) isn't cut off.
        modifier =
            modifier
                .size(CardWidth, CardHeight)
                .focusRing(interactionSource, Dotline.radius.m)
                .clip(Dotline.radius.m)
                .background(background)
                .combinedClickable(
                    interactionSource = interactionSource,
                    indication = null, // No Material ripple: the pressed background is the feedback.
                    role = Role.Button,
                    onLongClick = onLongClick,
                    onLongClickLabel = onLongClickLabel,
                    onClick = onClick,
                ).semantics {
                    stateDescription =
                        listOfNotNull(if (isCurrent) "Current note" else null, if (isPinned) "Pinned" else null).joinToString(", ")
                }.padding(horizontal = Dotline.space.s4, vertical = Dotline.space.s3),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null, tint = Dotline.colors.ink, modifier = Modifier.size(CardIconSize))
            Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2), verticalAlignment = Alignment.CenterVertically) {
                if (isPinned) {
                    Icon(DotlineIcons.Pin, contentDescription = null, tint = Dotline.colors.inkMuted, modifier = Modifier.size(16.dp))
                }
                if (isCurrent) {
                    Box(Modifier.size(Dotline.size.dot).background(Dotline.colors.accent, CircleShape))
                }
            }
        }
        Column {
            Text(
                text = name,
                style = Dotline.type.body.copy(fontWeight = FontWeight.Medium),
                color = Dotline.colors.ink,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = caption.uppercase(),
                style = Dotline.type.label,
                color = Dotline.colors.inkFaint,
                maxLines = 1,
            )
        }
    }
}

/**
 * The horizontal "OPEN NOTES" row. It starts at the 16 dp screen margin and scrolls to the
 * screen edge, with 12 dp between cards.
 *
 * Generic over your note type: pass the list and draw each item with an [OpenNoteCard].
 *
 * @param key a stable id per item (e.g. the note path), so cards keep their state when the list changes.
 */
@Composable
fun <T> OpenNotesRow(
    notes: List<T>,
    modifier: Modifier = Modifier,
    key: ((T) -> Any)? = null,
    card: @Composable (T) -> Unit,
) {
    // LazyRow only composes the cards that are on screen.
    LazyRow(
        modifier = modifier.fillMaxWidth(),
        contentPadding = PaddingValues(horizontal = Dotline.space.s4),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3),
    ) {
        items(notes, key = key) { note -> card(note) }
    }
}

@Composable
private fun OpenNoteCardPreviewContent() {
    val notes = listOf("Example" to "Editing", "Welcome" to "1 h ago", "Server Keys" to "Locked")
    OpenNotesRow(notes = notes, key = { it.first }) { (name, caption) ->
        OpenNoteCard(
            name = name,
            caption = caption,
            icon = if (caption == "Locked") DotlineIcons.Lock else DotlineIcons.Note,
            isCurrent = name == "Example",
            onClick = {},
        )
    }
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun OpenNoteCardDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { OpenNoteCardPreviewContent() }
    }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun OpenNoteCardLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { OpenNoteCardPreviewContent() }
    }
}
