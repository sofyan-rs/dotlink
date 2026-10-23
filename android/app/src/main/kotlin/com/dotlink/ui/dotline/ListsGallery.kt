/**
 * Debug gallery section for the "Lists" group of Dotline components: ScreenTitle, SectionLabel,
 * OpenNoteCard / OpenNotesRow, ListGroup / ListRow, TreeRow, QuickActionTile / QuickActionRow,
 * DotlineMenu, DotlineSnackbar and BottomNav, all with sample data and live state.
 *
 * Put [ListsGallerySection] in a scrolling debug screen; it does not scroll by itself.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme
import kotlinx.coroutines.launch

/** Every "Lists" component with sample data. Taps change the sample state, so you can try them. */
@Composable
fun ListsGallerySection(modifier: Modifier = Modifier) {
    Column(
        modifier = modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(Dotline.space.s5),
    ) {
        GalleryItem("ScreenTitle", padded = false) { ScreenTitleDemo() }
        GalleryItem("SectionLabel") { SectionLabel("Open notes") }
        GalleryItem("OpenNoteCard · OpenNotesRow", padded = false) { OpenNotesDemo() }
        GalleryItem("ListGroup · ListRow") { ListRowsDemo() }
        GalleryItem("ListRow in a sheet") { SheetRowsDemo() }
        GalleryItem("TreeRow") { TreeDemo() }
        GalleryItem("QuickActionTile · QuickActionRow") { QuickActionsDemo() }
        GalleryItem("DotlineMenu · MoreButton") { MenuDemo() }
        GalleryItem("DotlineSnackbar") { SnackbarDemo() }
        GalleryItem("BottomNav", padded = false) { BottomNavDemo() }
    }
}

/** A small `label` overline with the component name, then the demo. */
@Composable
private fun GalleryItem(
    name: String,
    padded: Boolean = true,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        Text(
            name.uppercase(),
            style = Dotline.type.label,
            color = Dotline.colors.inkFaint,
            modifier = Modifier.padding(horizontal = Dotline.space.s4),
        )
        // Some components bring their own 16 dp screen margin; the others get it here.
        Column(
            modifier = if (padded) Modifier.padding(horizontal = Dotline.space.s4) else Modifier,
            content = content,
        )
    }
}

@Composable
private fun ScreenTitleDemo() {
    var vault by remember { mutableStateOf("Personal") }
    ScreenTitle(
        title = vault,
        // Tapping the title stands in for the vault switcher: it flips between two vaults.
        onTitleClick = { vault = if (vault == "Personal") "Work" else "Personal" },
        subline = "9 notes",
        status = "Synced 2 min ago",
        actions = {
            ScreenTitleAction(DotlineIcons.Search, contentDescription = "Search notes", onClick = {})
            ScreenTitleAction(DotlineIcons.NewFolder, contentDescription = "New folder", onClick = {})
        },
    )
}

@Composable
private fun OpenNotesDemo() {
    var current by remember { mutableStateOf("Example") }
    val notes = listOf("Example", "Welcome", "Server Keys")
    OpenNotesRow(notes = notes, key = { it }) { name ->
        val locked = name == "Server Keys"
        OpenNoteCard(
            name = name,
            caption =
                when {
                    locked -> "Locked"
                    name == current -> "Editing"
                    else -> "1 h ago"
                },
            icon = if (locked) DotlineIcons.Lock else DotlineIcons.Note,
            isCurrent = name == current,
            onClick = { if (!locked) current = name },
        )
    }
}

@Composable
private fun ListRowsDemo() {
    var openVault by remember { mutableStateOf("Personal") }
    val vaults = listOf("Personal" to "Documents/Personal · 9 notes", "Work" to "Synced from git · 41 notes")
    ListGroup {
        vaults.forEachIndexed { index, (name, path) ->
            if (index > 0) ListDivider()
            ListRow(
                title = name,
                subtitle = path,
                subtitleIsData = true,
                leadingIcon = DotlineIcons.Vault,
                minHeight = 72.dp, // two-line vault rows
                onClick = { openVault = name },
                onMoreClick = if (name == openVault) null else ({}),
                trailing =
                    if (name == openVault) {
                        { ListRowValue("OPEN", color = Dotline.colors.ink) }
                    } else {
                        null
                    },
            )
        }
        ListDivider()
        ListRow(
            title = "Remote",
            subtitle = "github.com/sofyan-rs/notes.git",
            subtitleIsData = true,
            minHeight = 64.dp,
            onClick = {},
            trailing = { ListRowChevron() },
        )
        ListDivider()
        ListRow(
            title = "Sign-in",
            onClick = {},
            trailing = {
                Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s3), verticalAlignment = Alignment.CenterVertically) {
                    ListRowValue("Access token")
                    ListRowChevron()
                }
            },
        )
    }
}

@Composable
private fun SheetRowsDemo() {
    // The sheet background, so the `bg` group stands out as it does in a real sheet.
    Box(Modifier.background(Dotline.colors.surface, Dotline.radius.l).padding(Dotline.space.s4)) {
        ListGroup(inSheet = true) {
            ListRow(title = "Rename", leadingIcon = DotlineIcons.Edit, inSheet = true, onClick = {})
            ListDivider()
            ListRow(
                title = "Move to…",
                leadingIcon = DotlineIcons.MoveTo,
                inSheet = true,
                onClick = {},
                trailing = { ListRowValue("/") },
            )
            ListDivider()
            ListRow(
                title = "Lock note",
                subtitle = "Encrypts it on disk with the vault password",
                leadingIcon = DotlineIcons.Lock,
                inSheet = true,
                onClick = {},
            )
            ListDivider()
            ListRow(title = "Move to trash", leadingIcon = DotlineIcons.Trash, inSheet = true, destructive = true, onClick = {})
        }
    }
}

@Composable
private fun TreeDemo() {
    var journalOpen by remember { mutableStateOf(false) }
    var projectsOpen by remember { mutableStateOf(true) }
    var selected by remember { mutableStateOf("Example") }
    ListGroup {
        TreeRow(name = "Journal", depth = 0, isFolder = true, expanded = journalOpen, count = 1, onClick = { journalOpen = !journalOpen })
        if (journalOpen) {
            ListDivider()
            TreeRow(
                name = "2026-10-09",
                depth = 1,
                isFolder = false,
                subtitle = "Yesterday",
                selected = selected == "2026-10-09",
                onClick = { selected = "2026-10-09" },
                onMoreClick = {},
            )
        }
        ListDivider()
        TreeRow(
            name = "Projects",
            depth = 0,
            isFolder = true,
            expanded = projectsOpen,
            count = 2,
            onClick = { projectsOpen = !projectsOpen },
            onMoreClick = {},
        )
        if (projectsOpen) {
            ListDivider()
            TreeRow(
                name = "Example",
                depth = 1,
                isFolder = false,
                subtitle = "2 min ago · 645 chars",
                selected = selected == "Example",
                onClick = { selected = "Example" },
                onLongClick = {},
                onMoreClick = {},
            )
            ListDivider()
            TreeRow(name = "Server Keys", depth = 1, isFolder = false, isLocked = true, onClick = {}, onMoreClick = {})
        }
    }
}

@Composable
private fun QuickActionsDemo() {
    var pinned by remember { mutableStateOf(false) }
    QuickActionRow(
        actions =
            listOf(
                QuickAction(if (pinned) "Unpin" else "Pin", DotlineIcons.Pin, onClick = { pinned = !pinned }),
                QuickAction("Copy path", DotlineIcons.Copy, onClick = {}),
                QuickAction("Share", DotlineIcons.Share, onClick = {}),
            ),
    )
}

@Composable
private fun MenuDemo() {
    var open by remember { mutableStateOf(false) }
    var lastAction by remember { mutableStateOf("Tap ⋮") }
    // Every item closes the menu and records what was tapped.
    val pick: (String) -> Unit = {
        lastAction = it
        open = false
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Text(lastAction, style = Dotline.type.data, color = Dotline.colors.inkMuted, modifier = Modifier.weight(1f))
        // The menu opens below the Box that holds the ⋮ anchor.
        Box {
            MoreButton(onClick = { open = true }, contentDescription = "Note actions", open = open, tint = Dotline.colors.ink)
            DotlineMenu(expanded = open, onDismissRequest = { open = false }) {
                DotlineMenuItem("Rename", DotlineIcons.Edit, onClick = { pick("Rename") })
                DotlineMenuDivider()
                DotlineMenuItem("Move to…", DotlineIcons.MoveTo, onClick = { pick("Move to…") })
                DotlineMenuDivider()
                DotlineMenuItem("Pin note", DotlineIcons.Pin, onClick = { pick("Pin note") })
                DotlineMenuDivider()
                DotlineMenuItem("Copy path", DotlineIcons.Copy, onClick = { pick("Copy path") })
                DotlineMenuDivider()
                DotlineMenuItem("Share", DotlineIcons.Share, onClick = { pick("Share") })
                DotlineMenuDivider()
                DotlineMenuItem("Lock note", DotlineIcons.Lock, onClick = { pick("Lock note") })
                DotlineMenuDivider()
                DotlineMenuItem("Move to trash", DotlineIcons.Trash, destructive = true, onClick = { pick("Move to trash") })
            }
        }
    }
}

@Composable
private fun SnackbarDemo() {
    val hostState = remember { SnackbarHostState() }
    // A coroutine scope tied to this composable: showing a snackbar suspends until it is gone.
    val scope = rememberCoroutineScope()
    var trashed by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        DotlineSnackbar(message = "“Example” moved to trash", icon = DotlineIcons.Trash, actionLabel = "Undo")
        ListGroup {
            ListRow(
                title = if (trashed) "Example is in the trash" else "Move Example to trash",
                leadingIcon = DotlineIcons.Trash,
                destructive = !trashed,
                onClick = {
                    if (!trashed) {
                        trashed = true
                        scope.launch {
                            val result = hostState.showDotlineSnackbar("“Example” moved to trash")
                            if (result == SnackbarResult.ActionPerformed) trashed = false
                        }
                    }
                },
            )
        }
        // Tap the row above: the live snackbar shows here for about 5 s. In a real screen the
        // host sits at the bottom, above the bottom nav.
        DotlineSnackbarHost(hostState = hostState, icon = DotlineIcons.Trash)
    }
}

@Composable
private fun BottomNavDemo() {
    var tab by remember { mutableStateOf(MainTab.Link) }
    BottomNav(tabs = MainTab.entries, selectedTab = tab, onTabClick = { tab = it })
}

@Preview(name = "Dark", heightDp = 2000)
@Composable
private fun ListsGalleryDarkPreview() {
    DotlinkTheme(dark = true) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
            ListsGallerySection(Modifier.verticalScroll(rememberScrollState()))
        }
    }
}

@Preview(name = "Light", heightDp = 2000)
@Composable
private fun ListsGalleryLightPreview() {
    DotlinkTheme(dark = false) {
        Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
            ListsGallerySection(Modifier.verticalScroll(rememberScrollState()))
        }
    }
}
