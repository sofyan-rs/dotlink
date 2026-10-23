/**
 * Dotline alert dialog: a centred `surface` card (radius 28, no shadow) over the `scrim`, with
 * a title, a body, an optional extra slot (e.g. a checkbox row) and two action slots.
 *
 * Used by the "Remove vault" confirmation.
 *
 * Two parts:
 * - [DotlineDialogCard]: just the card, so it can be shown inline (previews, gallery).
 * - [DotlineDialog]: the card in its own window over the scrim; back and scrim tap dismiss it.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.SideEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/**
 * Shows a Dotline alert dialog. Put it in an `if (open) { ... }` and set `open = false` in
 * [onDismissRequest] (back press, scrim tap) and in your action lambdas.
 *
 * Actions are slots so the caller picks the button style: usually a `SecondaryButton`
 * ("Cancel") in [dismissButton] and a `PrimaryButton` or `DestructiveButton` in
 * [confirmButton]. Both slots get equal width (`Modifier.weight(1f)` is applied for you
 * through the [RowScope]; pass `Modifier.fillMaxWidth()` to your button).
 *
 * @param extraContent optional content under the body, e.g. "Also delete the folder" checkbox.
 */
@Composable
fun DotlineDialog(
    onDismissRequest: () -> Unit,
    title: String,
    body: String,
    confirmButton: @Composable RowScope.() -> Unit,
    dismissButton: @Composable RowScope.() -> Unit,
    modifier: Modifier = Modifier,
    extraContent: (@Composable ColumnScope.() -> Unit)? = null,
) {
    Dialog(
        onDismissRequest = onDismissRequest,
        // Full-screen window so we can draw our own scrim colour behind the card.
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        // The platform dims behind dialogs with its own grey; turn that off and use Dotline's scrim.
        val dialogWindow = (LocalView.current.parent as? DialogWindowProvider)?.window
        SideEffect { dialogWindow?.setDimAmount(0f) }

        Box(
            modifier =
                Modifier
                    .fillMaxSize()
                    .background(Dotline.colors.scrim)
                    // A tap on the scrim closes the dialog, like tapping outside a sheet.
                    .pointerInput(onDismissRequest) { detectTapGestures { onDismissRequest() } }
                    .safeDrawingPadding(),
            contentAlignment = Alignment.Center,
        ) {
            DotlineDialogCard(
                title = title,
                body = body,
                confirmButton = confirmButton,
                dismissButton = dismissButton,
                extraContent = extraContent,
                modifier =
                    modifier
                        // Mockup: 24 dp from the screen edges.
                        .padding(horizontal = Dotline.space.s5)
                        // An empty pointerInput "catches" taps on the card, so they don't
                        // fall through to the scrim behind and close the dialog.
                        .pointerInput(Unit) {},
            )
        }
    }
}

/**
 * The dialog card on its own (no window, no scrim). [DotlineDialog] uses it; you can also use
 * it inline. Parameters are the same as [DotlineDialog].
 */
@Composable
fun DotlineDialogCard(
    title: String,
    body: String,
    confirmButton: @Composable RowScope.() -> Unit,
    dismissButton: @Composable RowScope.() -> Unit,
    modifier: Modifier = Modifier,
    extraContent: (@Composable ColumnScope.() -> Unit)? = null,
) {
    Column(
        modifier =
            modifier
                .fillMaxWidth()
                .background(Dotline.colors.surface, Dotline.radius.l)
                // paneTitle makes TalkBack announce the dialog by its title when it opens.
                .semantics { paneTitle = title }
                .padding(Dotline.space.s5),
        verticalArrangement = Arrangement.spacedBy(Dotline.space.s4),
    ) {
        Text(
            text = title,
            style = Dotline.type.title,
            color = Dotline.colors.ink,
            modifier = Modifier.semantics { heading() },
        )
        Text(text = body, style = Dotline.type.body, color = Dotline.colors.inkMuted)
        extraContent?.invoke(this)
        // Mockup: 8 dp extra space above the buttons, 8 dp between them.
        Row(
            modifier = Modifier.fillMaxWidth().padding(top = Dotline.space.s2),
            horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
        ) {
            // Each slot gets its own weighted Row, so both halves are equally wide.
            Row(Modifier.weight(1f)) { dismissButton() }
            Row(Modifier.weight(1f)) { confirmButton() }
        }
    }
}

/** Stand-in pill for previews; the real buttons live in their own files. */
@Composable
private fun PreviewPill(
    text: String,
    fill: Boolean,
) {
    val colors = Dotline.colors
    Box(
        modifier =
            Modifier
                .fillMaxWidth()
                .height(48.dp)
                .background(if (fill) colors.accent else colors.surfaceRaised, Dotline.radius.pill),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            style = Dotline.type.body.copy(fontWeight = FontWeight.Medium, textAlign = TextAlign.Center),
            color = if (fill) colors.onAccent else colors.ink,
        )
    }
}

@Composable
private fun DialogPreviewContent() {
    Box(Modifier.background(Dotline.colors.scrim).padding(16.dp)) {
        DotlineDialogCard(
            title = "Remove “Work”?",
            body = "Dotnote forgets this vault. The folder and its 41 notes stay on your phone, and you can open it again any time.",
            dismissButton = { PreviewPill("Cancel", fill = false) },
            confirmButton = { PreviewPill("Remove", fill = true) },
            extraContent = {
                Text("Also delete the folder", style = Dotline.type.body, color = Dotline.colors.ink)
            },
        )
    }
}

@Preview(name = "Dark")
@Composable
private fun DotlineDialogDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { DialogPreviewContent() }
    }
}

@Preview(name = "Light")
@Composable
private fun DotlineDialogLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) { DialogPreviewContent() }
    }
}
