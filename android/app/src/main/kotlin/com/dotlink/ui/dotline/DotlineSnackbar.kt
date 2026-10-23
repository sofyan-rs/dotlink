/**
 * DotlineSnackbar: the short message at the bottom after an action, with an optional UNDO
 * (after a move to trash: "“Example” moved to trash · UNDO").
 *
 * `surfaceRaised`, 1.5 dp `lineStrong` border, radius 16, min 56 dp: a muted 20 dp icon, the
 * message in `body`, and the action in bold `label` caps.
 *
 * Also in this file:
 * - [DotlineSnackbarHost]: shows the current message of a Material 3 `SnackbarHostState`
 *   with our look. Place it at the bottom of the screen, above the bottom nav.
 * - [showDotlineSnackbar]: shows a message for about 5 seconds and tells you if UNDO was tapped.
 *
 * Usage:
 * ```
 * val snackbars = remember { SnackbarHostState() }
 * val scope = rememberCoroutineScope()
 * scope.launch {
 *     if (snackbars.showDotlineSnackbar("“Example” moved to trash") == SnackbarResult.ActionPerformed) undo()
 * }
 * DotlineSnackbarHost(snackbars, icon = DotlineIcons.Trash)
 * ```
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme
import kotlinx.coroutines.withTimeoutOrNull

/** The snackbar icon is 20 dp (Trash-Undo mockup). */
private val SnackbarIconSize = 20.dp

/** How long a snackbar stays (about 5 s). */
private const val SNACKBAR_MILLIS = 5_000L

/**
 * The snackbar itself (no timing; see [DotlineSnackbarHost] for that).
 *
 * @param message the text, e.g. "“Example” moved to trash".
 * @param icon optional muted icon at the start.
 * @param actionLabel e.g. "UNDO" (uppercased for you); `null` hides the action.
 * @param onAction called when the action is tapped.
 */
@Composable
fun DotlineSnackbar(
    message: String,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    actionLabel: String? = null,
    onAction: () -> Unit = {},
) {
    Row(
        modifier =
            modifier
                .fillMaxWidth()
                .heightIn(min = Dotline.size.rowMin)
                .clip(Dotline.radius.m)
                .background(Dotline.colors.surfaceRaised)
                .border(Dotline.size.stroke, Dotline.colors.lineStrong, Dotline.radius.m)
                // A polite live region makes TalkBack read the message when it appears.
                .semantics { liveRegion = LiveRegionMode.Polite }
                .padding(start = Dotline.space.s4, end = Dotline.space.s1),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(icon, contentDescription = null, tint = Dotline.colors.inkMuted, modifier = Modifier.size(SnackbarIconSize))
        }
        Text(
            text = message,
            style = Dotline.type.body,
            color = Dotline.colors.ink,
            modifier = Modifier.weight(1f).padding(vertical = Dotline.space.s2),
        )
        if (actionLabel != null) {
            SnackbarAction(label = actionLabel, onClick = onAction)
        }
    }
}

/** The 48 dp tall text button at the end ("UNDO"). */
@Composable
private fun SnackbarAction(
    label: String,
    onClick: () -> Unit,
) {
    val interactionSource = remember { MutableInteractionSource() }
    Box(
        modifier =
            Modifier
                .height(Dotline.size.touchTarget)
                .pressScale(interactionSource)
                .insetFocusRing(interactionSource) // The snackbar clips its content, so the ring goes inside.
                .clickable(
                    interactionSource = interactionSource,
                    indication = null,
                    role = Role.Button,
                    onClick = onClick,
                ).padding(horizontal = Dotline.space.s4),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = label.uppercase(),
            style = Dotline.type.label.copy(fontWeight = FontWeight.Bold),
            color = Dotline.colors.ink,
        )
    }
}

/**
 * Shows the message currently in [hostState] as a [DotlineSnackbar], with 16 dp side and bottom
 * margins. Place it at the bottom of the screen, above the bottom nav
 * (e.g. `Box(Modifier.align(Alignment.BottomCenter))` or a Scaffold's `snackbarHost`).
 *
 * @param icon icon shown in every snackbar of this host, e.g. [DotlineIcons.Trash].
 */
@Composable
fun DotlineSnackbarHost(
    hostState: SnackbarHostState,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
) {
    // Material's SnackbarHost handles the queue and the fade; we only swap in our own snackbar.
    SnackbarHost(hostState = hostState, modifier = modifier) { data ->
        DotlineSnackbar(
            message = data.visuals.message,
            icon = icon,
            actionLabel = data.visuals.actionLabel,
            onAction = { data.performAction() },
            modifier = Modifier.padding(start = Dotline.space.s4, end = Dotline.space.s4, bottom = Dotline.space.s4),
        )
    }
}

/**
 * Shows [message] for about 5 seconds and waits until it is gone.
 *
 * Material only offers 4 s (Short) or 10 s (Long), so we show it without a time limit and
 * cancel after 5 s; cancelling `showSnackbar` hides the snackbar.
 *
 * @return [SnackbarResult.ActionPerformed] when [actionLabel] was tapped, else [SnackbarResult.Dismissed].
 */
suspend fun SnackbarHostState.showDotlineSnackbar(
    message: String,
    actionLabel: String? = "UNDO",
): SnackbarResult =
    withTimeoutOrNull(SNACKBAR_MILLIS) {
        showSnackbar(message = message, actionLabel = actionLabel, duration = SnackbarDuration.Indefinite)
    } ?: SnackbarResult.Dismissed

@Composable
private fun DotlineSnackbarPreviewContent() {
    DotlineSnackbar(
        message = "“Example” moved to trash",
        icon = DotlineIcons.Trash,
        actionLabel = "Undo",
        onAction = {},
    )
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun DotlineSnackbarDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { DotlineSnackbarPreviewContent() }
    }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun DotlineSnackbarLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { DotlineSnackbarPreviewContent() }
    }
}
