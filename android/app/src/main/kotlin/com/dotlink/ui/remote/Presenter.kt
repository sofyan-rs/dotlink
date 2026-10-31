/**
 * The Remote's Presenter mode, for slides: Previous on the left third, Next on the rest (big
 * enough to hit without looking), End / Black, a timer, and the volume keys as Next and
 * Previous. Only keys go to the PC, as from the keyboard: nothing on the PC knows about it.
 *
 * No Start: each app starts its slideshow with other keys (F5 in PowerPoint and Impress,
 * ⌥⌘P in Keynote, Ctrl+Alt+P in Canva) and F5 reloads a browser tab (Google Slides, Canva).
 * The slideshow is started on the PC; the arrows, Esc and B work in all of them.
 */
package com.dotlink.ui.remote

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.dotlink.ui.theme.Dotline
import kotlinx.coroutines.delay

/**
 * Volume up / down while Presenter is on screen. MainActivity hands the keys here first and
 * consumes them when a handler is set.
 */
object VolumeKeys {
    /** Called with true for volume up. */
    @Volatile var handler: ((Boolean) -> Unit)? = null
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
internal fun Presenter(
    onKey: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val haptic = LocalHapticFeedback.current
    val next = {
        haptic.performHapticFeedback(HapticFeedbackType.VirtualKey)
        onKey("RIGHT")
    }
    val prev = {
        haptic.performHapticFeedback(HapticFeedbackType.ContextClick)
        onKey("LEFT")
    }
    // The screen stays on, and the volume keys turn slides.
    val view = LocalView.current
    DisposableEffect(Unit) {
        view.keepScreenOn = true
        VolumeKeys.handler = { up -> if (up) onKey("RIGHT") else onKey("LEFT") }
        onDispose {
            view.keepScreenOn = false
            VolumeKeys.handler = null
        }
    }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        Row(Modifier.weight(1f).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
            Zone("◀\nPREVIOUS", Modifier.weight(1f), onClick = prev)
            Zone("NEXT\n▶", Modifier.weight(2f), primary = true, onClick = next)
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
            Chip("END", Modifier.weight(1f), onClick = { onKey("ESC") })
            Chip("BLACK", Modifier.weight(1f), onClick = { onKey("B") })
            Timer(Modifier.weight(1f))
        }
    }
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun Zone(
    label: String,
    modifier: Modifier,
    primary: Boolean = false,
    onClick: () -> Unit,
) {
    val colors = Dotline.colors
    Box(
        modifier
            .fillMaxHeight()
            .clip(Dotline.radius.l)
            .background(if (primary) colors.surfaceRaised else colors.surface)
            .combinedClickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Dotline.type.label, color = if (primary) colors.ink else colors.inkMuted, textAlign = TextAlign.Center)
    }
}

@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
private fun Chip(
    label: String,
    modifier: Modifier,
    active: Boolean = false,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
) {
    val colors = Dotline.colors
    val haptic = LocalHapticFeedback.current
    Box(
        modifier
            .height(48.dp)
            .clip(Dotline.radius.pill)
            .background(if (active) colors.ink else colors.bg)
            .border(Dotline.size.stroke, if (active) colors.ink else colors.lineStrong, Dotline.radius.pill)
            .combinedClickable(
                onClick = {
                    haptic.performHapticFeedback(HapticFeedbackType.VirtualKey)
                    onClick()
                },
                onLongClick =
                    onLongClick?.let {
                        {
                            haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                            it()
                        }
                    },
            ).padding(horizontal = Dotline.space.s2),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Dotline.type.label, color = if (active) colors.onInk else colors.ink, textAlign = TextAlign.Center)
    }
}

/** Tap starts or pauses, hold resets. Kept across leaving the tab. */
@Composable
private fun Timer(modifier: Modifier) {
    var running by rememberSaveable { mutableStateOf(false) }
    var elapsed by rememberSaveable { mutableLongStateOf(0L) }
    LaunchedEffect(running) {
        var last = System.currentTimeMillis()
        while (running) {
            delay(250)
            val now = System.currentTimeMillis()
            elapsed += now - last
            last = now
        }
    }
    val s = elapsed / 1000
    val text = if (s >= 3600) "%d:%02d:%02d".format(s / 3600, s / 60 % 60, s % 60) else "%02d:%02d".format(s / 60, s % 60)
    Chip(
        label = if (running || elapsed > 0) text else "TIMER",
        modifier = modifier,
        active = running,
        onClick = { running = !running },
        onLongClick = {
            running = false
            elapsed = 0
        },
    )
}
