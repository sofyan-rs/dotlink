package com.dotlink.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.dotlink.ui.dotline.SegmentedControl
import com.dotlink.ui.dotline.SegmentOption
import androidx.compose.runtime.setValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.getValue
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.ui.draw.clipToBounds
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.ui.dotline.SectionLabel
import com.dotlink.ui.dotline.bottomNavHeight
import com.dotlink.ui.theme.Dotline

/** Whether to draw dark: the Settings theme choice, or the system's when it is "System". */
@Composable
fun isAppInDarkTheme(): Boolean {
    val mode by DotlinkApp.link.themeMode.collectAsStateWithLifecycle()
    return when (mode) {
        1 -> false
        2 -> true
        else -> isSystemInDarkTheme()
    }
}

/** A scrolling tab screen: status-bar inset on top, room for the bottom nav at the end. */
@Composable
fun TabScreen(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier =
            // Inset first, then scroll: content scrolls under nothing, not under the status bar.
            modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.statusBars)
                .clipToBounds()
                .verticalScroll(rememberScrollState())
                .padding(top = Dotline.space.s3),
    ) {
        content()
        Spacer(Modifier.height(bottomNavHeight() + Dotline.space.s4))
    }
}

/** Section overline with the standard gaps (24 dp above, 8 dp below), inset 16 dp. */
@Composable
fun Section(
    label: String,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(Modifier.padding(horizontal = Dotline.space.s4)) {
        Spacer(Modifier.height(Dotline.space.s5))
        SectionLabel(label, Modifier.padding(start = Dotline.space.s4, bottom = Dotline.space.s2))
        content()
    }
}

/** Grey helper text under a group. */
@Composable
fun Hint(text: String) {
    Text(
        text = text,
        style = Dotline.type.bodyS,
        color = Dotline.colors.inkMuted,
        modifier = Modifier.padding(start = Dotline.space.s4, end = Dotline.space.s4, top = Dotline.space.s2),
    )
}

fun formatSize(bytes: Long): String =
    when {
        bytes < 0 -> "?"
        bytes < 1024 -> "$bytes B"
        bytes < 1024 * 1024 -> "%.0f KB".format(bytes / 1024.0)
        bytes < 1024L * 1024 * 1024 -> "%.1f MB".format(bytes / 1048576.0)
        else -> "%.2f GB".format(bytes / 1073741824.0)
    }

/** A segmented control that remembers its own selection; [onSelect] stores it. */
@Composable
fun Choice(
    labels: List<String>,
    initial: Int,
    onSelect: (Int) -> Unit,
) {
    var selected by remember { mutableIntStateOf(initial) }
    SegmentedControl(
        options = labels.map { SegmentOption(it) },
        selectedIndex = selected,
        onSelect = {
            selected = it
            onSelect(it)
        },
        modifier = Modifier.fillMaxWidth(),
    )
}
