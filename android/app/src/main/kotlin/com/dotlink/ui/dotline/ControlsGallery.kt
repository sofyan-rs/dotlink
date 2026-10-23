/**
 * Debug gallery section for the Dotline "Controls" group: buttons, segmented control, switch,
 * radio dot, checkbox and text fields, each with sample data and live state.
 *
 * Put [ControlsGallerySection] on a debug gallery screen to try every control in one place.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.clearText
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
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Every control from this group, stacked, with a `label` overline above each one. */
@Composable
fun ControlsGallerySection(modifier: Modifier = Modifier) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(Dotline.space.s5)) {
        ButtonsSample()
        SegmentedSample()
        SwitchAndRadioSample()
        CheckboxSample()
        TextFieldsSample()
    }
}

/** A small `label` overline plus the sample under it. */
@Composable
private fun GalleryItem(
    title: String,
    content: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        Text(title.uppercase(), style = Dotline.type.label, color = Dotline.colors.inkFaint)
        content()
    }
}

@Composable
private fun ButtonsSample() {
    var taps by remember { mutableIntStateOf(0) }
    GalleryItem("Buttons · tapped $taps") {
        Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
            PrimaryButton("Open a folder", onClick = { taps++ }, icon = DotlineIcons.Folder, modifier = Modifier.fillMaxWidth())
            SecondaryButton("Clone from git", onClick = { taps++ }, icon = DotlineIcons.GitBranch, modifier = Modifier.fillMaxWidth())
            Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
                SecondaryButton("Cancel", onClick = { taps++ }, compact = true, modifier = Modifier.weight(1f))
                DestructiveButton("Remove", onClick = { taps++ }, compact = true, modifier = Modifier.weight(1f))
            }
            PrimaryButton("Disabled", onClick = {}, enabled = false, compact = true, modifier = Modifier.fillMaxWidth())
        }
    }
}

@Composable
private fun SegmentedSample() {
    var mode by remember { mutableIntStateOf(0) }
    var kind by remember { mutableIntStateOf(0) }
    GalleryItem("Segmented control") {
        Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s3)) {
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

@Composable
private fun SwitchAndRadioSample() {
    var autoSync by remember { mutableStateOf(true) }
    var vault by remember { mutableIntStateOf(0) }
    Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s5)) {
        GalleryItem("Switch") {
            Row(verticalAlignment = Alignment.CenterVertically) {
                KuSwitch(checked = autoSync, onCheckedChange = { autoSync = it }, contentDescription = "Auto-sync")
                KuSwitch(checked = false, onCheckedChange = {}, enabled = false, contentDescription = "Disabled")
            }
        }
        GalleryItem("Radio dot") {
            Row(verticalAlignment = Alignment.CenterVertically) {
                RadioDot(selected = vault == 0, onClick = { vault = 0 })
                RadioDot(selected = vault == 1, onClick = { vault = 1 })
                RadioDot(selected = vault == 2, onClick = { vault = 2 }, small = true)
                RadioDot(selected = false, onClick = {}, enabled = false, small = true)
            }
        }
    }
}

@Composable
private fun CheckboxSample() {
    var fingerprint by remember { mutableStateOf(true) }
    var deleteFolder by remember { mutableStateOf(false) }
    GalleryItem("Checkbox") {
        Column {
            DotlineCheckboxRow("Unlock with fingerprint", checked = fingerprint, onCheckedChange = { fingerprint = it })
            DotlineCheckboxRow(
                text = "Also delete the folder",
                supportingText = "Moves Documents/Work to trash. Notes synced with git stay in the repository.",
                checked = deleteFolder,
                onCheckedChange = { deleteFolder = it },
                destructive = true,
            )
        }
    }
}

@Composable
private fun TextFieldsSample() {
    // rememberTextFieldState keeps the typed text across recompositions (and config changes).
    val name = rememberTextFieldState("Example")
    val search = rememberTextFieldState("")
    val password = rememberTextFieldState("")
    val confirm = rememberTextFieldState("")
    var visible by remember { mutableStateOf(false) }
    val matches = password.text.isNotEmpty() && password.text.toString() == confirm.text.toString()
    val mismatch = confirm.text.isNotEmpty() && !matches

    GalleryItem("Text fields") {
        Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s4)) {
            DotlineTextField(
                state = name,
                label = "Name",
                suffix = ".md",
                trailing = {
                    DotlineTextFieldIconButton(DotlineIcons.Close, "Clear name", onClick = { name.clearText() }, iconSize = 18.dp)
                },
                helperText = "The file is renamed on disk. Open tabs follow it.",
            )
            DotlineTextField(
                state = search,
                label = null,
                placeholder = "Find a note by name",
                leadingIcon = DotlineIcons.Search,
                trailing = { DotlineTextFieldStatus("${search.text.length}") },
            )
            DotlinePasswordField(
                state = password,
                label = "Password",
                passwordVisible = visible,
                onPasswordVisibleChange = { visible = it },
            )
            DotlinePasswordField(
                state = confirm,
                label = "Confirm password",
                passwordVisible = false,
                onPasswordVisibleChange = null,
                trailing = if (matches) ({ DotlineTextFieldStatus("Match", positive = true) }) else null,
                errorText = if (mismatch) "The passwords don't match." else null,
            )
        }
    }
}

@Composable
private fun GalleryPreviewContent() {
    Box(Modifier.background(Dotline.colors.bg).verticalScroll(rememberScrollState()).padding(16.dp)) {
        ControlsGallerySection()
    }
}

@Preview(name = "Dark", heightDp = 1400)
@Composable
private fun ControlsGalleryDarkPreview() {
    DotlinkTheme(dark = true) { GalleryPreviewContent() }
}

@Preview(name = "Light", heightDp = 1400)
@Composable
private fun ControlsGalleryLightPreview() {
    DotlinkTheme(dark = false) { GalleryPreviewContent() }
}
