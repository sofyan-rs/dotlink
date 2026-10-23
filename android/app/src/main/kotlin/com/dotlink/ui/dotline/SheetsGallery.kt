/**
 * Debug gallery section for the "Sheets" group: bottom sheet, sheet header, dialog, dot meter,
 * dot art, formatter bar and the "New note" button, with sample data and live state.
 *
 * Put [SheetsGallerySection] on the debug gallery screen. Not used by real screens.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.triStateToggleable
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Strength meter in the gallery has 12 dots, like Set-Password. */
private const val METER_DOTS = 12

/** Shows every component of the Sheets group with sample data. */
@Composable
fun SheetsGallerySection(modifier: Modifier = Modifier) {
    // `remember { mutableStateOf(...) }` keeps this demo state while the gallery is on screen.
    var actionSheetOpen by remember { mutableStateOf(false) }
    var renameSheetOpen by remember { mutableStateOf(false) }
    var dialogOpen by remember { mutableStateOf(false) }
    var meterLit by remember { mutableIntStateOf(9) }
    var activeFormats by remember { mutableStateOf(setOf(FormatAction.Heading)) }
    var lastAction by remember { mutableStateOf<FormatAction?>(null) }
    var newNotes by remember { mutableIntStateOf(0) }

    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(Dotline.space.s5)) {
        GalleryItem("Bottom sheet") {
            Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
                SecondaryButton("Open action sheet", onClick = { actionSheetOpen = true })
                SecondaryButton("Open rename sheet", onClick = { renameSheetOpen = true })
            }
        }

        GalleryItem("Sheet header") {
            Column(
                Modifier.background(Dotline.colors.surface, Dotline.radius.m).padding(Dotline.space.s4),
                verticalArrangement = Arrangement.spacedBy(Dotline.space.s4),
            ) {
                SheetHeader(icon = DotlineIcons.Folder, title = "Projects", subtitle = "/Projects · 1 folder · 3 notes")
                SheetHeader(icon = DotlineIcons.Note, title = "Example", subtitle = "Example.md · 645 chars")
            }
        }

        GalleryItem("Dialog") {
            SecondaryButton("Open remove-vault dialog", onClick = { dialogOpen = true })
        }

        GalleryItem("Dot meter · tap to change") {
            Row(
                modifier =
                    Modifier
                        .heightIn(min = Dotline.size.touchTarget)
                        .clickable(role = Role.Button, onClickLabel = "Change strength") {
                            meterLit = (meterLit + 1) % (METER_DOTS + 1)
                        },
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3),
            ) {
                DotMeter(count = METER_DOTS, lit = meterLit, contentDescription = "Password strength")
                Text("$meterLit / $METER_DOTS", style = Dotline.type.label, color = Dotline.colors.inkMuted)
            }
        }

        GalleryItem("Dot art") {
            // FlowRow wraps to the next line when the patterns don't fit side by side.
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Dotline.space.s5),
                verticalArrangement = Arrangement.spacedBy(Dotline.space.s5),
            ) {
                DotArt(DotArtPatterns.NotePage, contentDescription = "A note")
                DotArt(DotArtPatterns.EmptyFolder, contentDescription = "Empty folder")
                DotArt(DotArtPatterns.Magnifier, contentDescription = "Nothing found", dotSize = 6.dp)
                DotArt(DotArtPatterns.Lock, contentDescription = "Locked")
            }
        }

        GalleryItem("Formatter bar · tap to toggle") {
            Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
                FormatterBar(
                    actions = FormatAction.entries,
                    activeActions = activeFormats,
                    onAction = { action ->
                        lastAction = action
                        activeFormats = if (action in activeFormats) activeFormats - action else activeFormats + action
                    },
                    line = 1,
                    column = 8,
                    chars = 645,
                    lineEnding = "LF",
                )
                Text(
                    text = "LAST: ${lastAction?.label?.uppercase() ?: "NONE"}",
                    style = Dotline.type.label,
                    color = Dotline.colors.inkFaint,
                )
            }
        }

        GalleryItem("New note button") {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3)) {
                NewNoteFab(onClick = { newNotes++ })
                Text("TAPPED $newNotes×", style = Dotline.type.label, color = Dotline.colors.inkFaint)
            }
        }
    }

    if (actionSheetOpen) {
        DotlineBottomSheet(onDismissRequest = { actionSheetOpen = false }) {
            SheetHeader(icon = DotlineIcons.Folder, title = "Projects", subtitle = "/Projects · 1 folder · 3 notes")
            Text(
                "Quick actions and rows go here. Tap the scrim, swipe down or press back to close.",
                style = Dotline.type.body,
                color = Dotline.colors.inkMuted,
            )
            SecondaryButton("Close", onClick = { actionSheetOpen = false })
        }
    }

    if (renameSheetOpen) {
        RenameSheetDemo(onClose = { renameSheetOpen = false })
    }

    if (dialogOpen) {
        RemoveVaultDialogDemo(onClose = { dialogOpen = false })
    }
}

/** A sheet with a text field, to check that the sheet rides above the keyboard. */
@Composable
private fun RenameSheetDemo(onClose: () -> Unit) {
    val name = rememberTextFieldState("Example")
    DotlineBottomSheet(onDismissRequest = onClose, bottomPadding = Dotline.space.s4) {
        Column(Modifier.padding(horizontal = Dotline.space.s1), verticalArrangement = Arrangement.spacedBy(Dotline.space.s1)) {
            Text("Rename note", style = Dotline.type.title, color = Dotline.colors.ink)
            Text("In /Personal", style = Dotline.type.data, color = Dotline.colors.inkMuted)
        }
        DotlineTextField(state = name, label = "Name", suffix = ".md", helperText = "The file is renamed on disk. Open tabs follow it.")
        Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
            SecondaryButton("Cancel", onClick = onClose, modifier = Modifier.weight(1f), compact = true)
            PrimaryButton("Rename", onClick = onClose, modifier = Modifier.weight(1f), compact = true)
        }
    }
}

/** The Remove-Vault dialog with its "Also delete the folder" checkbox row. */
@Composable
private fun RemoveVaultDialogDemo(onClose: () -> Unit) {
    var deleteFolder by remember { mutableStateOf(false) }
    DotlineDialog(
        onDismissRequest = onClose,
        title = "Remove “Work”?",
        body = "Dotnote forgets this vault. The folder and its 41 notes stay on your phone, and you can open it again any time.",
        dismissButton = { SecondaryButton("Cancel", onClick = onClose, modifier = Modifier.weight(1f), compact = true) },
        confirmButton = { DestructiveButton("Remove", onClick = onClose, modifier = Modifier.weight(1f), compact = true) },
        extraContent = {
            DotlineCheckboxRow(
                text = "Also delete the folder",
                checked = deleteFolder,
                onCheckedChange = { deleteFolder = it },
                supportingText = "Moves Documents/Work to trash. Notes synced with git stay in the repository.",
                destructive = true,
            )
        },
    )
}

/** Overline label above each component. */
@Composable
private fun GalleryItem(
    label: String,
    content: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        Text(label.uppercase(), style = Dotline.type.label, color = Dotline.colors.inkMuted)
        content()
    }
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun SheetsGalleryDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).verticalScroll(rememberScrollState()).padding(16.dp)) { SheetsGallerySection() }
    }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun SheetsGalleryLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).verticalScroll(rememberScrollState()).padding(16.dp)) { SheetsGallerySection() }
    }
}
