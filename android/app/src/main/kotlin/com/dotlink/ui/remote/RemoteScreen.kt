/**
 * The REMOTE tab: the phone as touchpad and keyboard for the PC.
 *
 * Touchpad: one finger moves, tap = left click, two-finger tap = right click, three-finger
 * tap = middle click, two fingers scroll, hold then move = drag.
 */
package com.dotlink.ui.remote

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.Link
import com.dotlink.link.LinkState
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.dotline.bottomNavHeight
import com.dotlink.ui.theme.Dotline
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONArray
import org.json.JSONObject
import kotlin.math.abs
import kotlin.math.hypot
import kotlinx.coroutines.delay

/** The `/ws/input` socket. Messages are dropped while it is not open. */
private class InputChannel(
    private val link: Link,
) {
    private var ws: WebSocket? = null

    fun open() {
        ws =
            link.openSocket(
                "/ws/input",
                object : WebSocketListener() {
                    override fun onMessage(
                        webSocket: WebSocket,
                        text: String,
                    ) {
                        JSONObject(text).optString("error").takeIf { it.isNotEmpty() }?.let { link.notices.tryEmit("Remote: $it") }
                    }

                    override fun onFailure(
                        webSocket: WebSocket,
                        t: Throwable,
                        response: Response?,
                    ) {
                        ws = null
                    }
                },
            )
    }

    fun close() {
        ws?.close(1000, null)
        ws = null
    }

    fun send(msg: JSONObject) {
        ws?.send(msg.toString())
    }

    // Moves and scrolls come with every touch event (up to 120 a second): plain strings, no
    // JSONObject per event.
    fun move(
        x: Float,
        y: Float,
    ) {
        ws?.send("{\"t\":\"move\",\"x\":$x,\"y\":$y}")
    }

    fun scroll(
        x: Float,
        y: Float,
    ) {
        ws?.send("{\"t\":\"scroll\",\"x\":$x,\"y\":$y}")
    }

    fun button(
        b: String,
        s: String,
    ) = send(JSONObject().put("t", "btn").put("b", b).put("s", s))

    fun key(k: String) = send(JSONObject().put("t", "key").put("k", k))

    fun combo(keys: List<String>) = send(JSONObject().put("t", "combo").put("k", JSONArray(keys)))

    fun text(s: String) = send(JSONObject().put("t", "text").put("s", s))
}

private const val SENTINEL = "  "
private const val RESET_MS = 400L
private const val TAP_MS = 250L
private const val HOLD_MS = 450L

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun RemoteScreen() {
    val link = DotlinkApp.link
    val state by link.state.collectAsStateWithLifecycle()
    val input = remember { InputChannel(link) }
    DisposableEffect(state is LinkState.Online) {
        input.open()
        onDispose { input.close() }
    }

    // Sticky modifiers: tap CTRL, then a key → Ctrl+key.
    val mods = remember { mutableStateListOf<String>() }

    fun sendKey(k: String) {
        if (mods.isEmpty()) input.key(k) else input.combo(mods + k)
        mods.clear()
    }

    fun sendTyped(s: String) {
        if (s.isEmpty()) return
        if (mods.isNotEmpty() && s.length == 1) {
            input.combo(mods + s.uppercase())
            mods.clear()
            return
        }
        // Newlines go as Enter presses so they also work in apps that ignore typed "\n".
        s.split('\n').forEachIndexed { i, part ->
            if (i > 0) input.key("ENTER")
            if (part.isNotEmpty()) input.text(part)
        }
    }

    val imeVisible = WindowInsets.isImeVisible
    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.statusBars)
            .imePadding()
            .padding(top = Dotline.space.s3),
    ) {
        ScreenTitle(
            title = "Remote",
            subline = if (state is LinkState.Online) "Tap: click · two fingers: scroll · hold: drag" else "Not connected",
        )
        Spacer(Modifier.height(Dotline.space.s4))
        Touchpad(
            input = input,
            speed = link.prefs.pointerSpeed,
            modifier =
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .padding(horizontal = Dotline.space.s4),
        )
        Spacer(Modifier.height(Dotline.space.s2))
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Dotline.space.s4),
            horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
        ) {
            MouseButton("L", "left", input, Modifier.weight(2f))
            MouseButton("M", "middle", input, Modifier.weight(1f))
            MouseButton("R", "right", input, Modifier.weight(2f))
        }
        Spacer(Modifier.height(Dotline.space.s2))
        KeyStrip(
            // Labels follow the PC's OS; read again whenever the link state changes (hello).
            modifiers = remember(state) { modifiers(link.prefs.pcOs) },
            mods = mods,
            onMod = { m -> if (m in mods) mods.remove(m) else mods.add(m) },
            onKey = ::sendKey,
            keyboardField = { KeyboardToggle(onTyped = ::sendTyped, onKey = ::sendKey) },
        )
        Spacer(Modifier.height(if (imeVisible) Dotline.space.s2 else bottomNavHeight()))
    }
}

@Composable
private fun Touchpad(
    input: InputChannel,
    speed: Float,
    modifier: Modifier = Modifier,
) {
    val haptic = LocalHapticFeedback.current
    Box(
        modifier =
            modifier
                .clip(Dotline.radius.l)
                .background(Dotline.colors.surface)
                .pointerInput(speed) {
                    val slop = viewConfiguration.touchSlop
                    awaitEachGesture {
                        awaitFirstDown(requireUnconsumed = false)
                        val start = System.currentTimeMillis()
                        var maxPointers = 1
                        var travelled = 0f
                        var dragging = false
                        while (true) {
                            // Wake up at the hold deadline even if the finger doesn't move.
                            val wait = if (!dragging && maxPointers == 1 && travelled < slop) HOLD_MS - (System.currentTimeMillis() - start).coerceAtLeast(0) else Long.MAX_VALUE
                            val event = if (wait > 0) withTimeoutOrNull(wait) { awaitPointerEvent() } else null
                            if (event == null) {
                                if (!dragging && maxPointers == 1 && travelled < slop) {
                                    dragging = true
                                    haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                                    input.button("left", "down")
                                }
                                continue
                            }
                            val pressed = event.changes.filter { it.pressed }
                            maxPointers = maxOf(maxPointers, pressed.size)
                            if (pressed.isEmpty()) break
                            var delta = Offset.Zero
                            pressed.forEach { delta += it.position - it.previousPosition }
                            delta /= pressed.size.toFloat()
                            travelled += hypot(delta.x, delta.y)
                            if (pressed.size == 1 && maxPointers == 1) {
                                // Faster swipes move further (simple pointer acceleration).
                                val accel = 1f + (abs(delta.x) + abs(delta.y)).coerceAtMost(40f) / 25f
                                if (delta != Offset.Zero) input.move(delta.x * speed * accel, delta.y * speed * accel)
                            } else if (pressed.size == 2 && delta != Offset.Zero) {
                                // Natural scrolling, like a laptop touchpad.
                                input.scroll(-delta.x * 2f, -delta.y * 2f)
                            }
                            event.changes.forEach { it.consume() }
                        }
                        val duration = System.currentTimeMillis() - start
                        when {
                            dragging -> {
                                haptic.performHapticFeedback(HapticFeedbackType.GestureEnd)
                                input.button("left", "up")
                            }
                            travelled < slop * 2 && duration < TAP_MS + 150 -> {
                                // A tick for a left click, a firmer one for right and middle.
                                haptic.performHapticFeedback(if (maxPointers == 1) HapticFeedbackType.VirtualKey else HapticFeedbackType.ContextClick)
                                when (maxPointers) {
                                    1 -> input.button("left", "click")
                                    2 -> input.button("right", "click")
                                    else -> input.button("middle", "click")
                                }
                            }
                        }
                    }
                },
        contentAlignment = Alignment.Center,
    ) {
        Text("TOUCHPAD", style = Dotline.type.label, color = Dotline.colors.inkFaint)
    }
}

@Composable
private fun MouseButton(
    label: String,
    button: String,
    input: InputChannel,
    modifier: Modifier = Modifier,
) {
    var held by remember { mutableStateOf(false) }
    val haptic = LocalHapticFeedback.current
    Box(
        modifier =
            modifier
                .height(52.dp)
                .clip(Dotline.radius.m)
                .background(if (held) Dotline.colors.navSelected else Dotline.colors.surfaceRaised)
                .pointerInput(button) {
                    awaitEachGesture {
                        awaitFirstDown()
                        held = true
                        haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
                        input.button(button, "down")
                        do {
                            val ev = awaitPointerEvent()
                        } while (ev.changes.any { it.pressed })
                        input.button(button, "up")
                        held = false
                    }
                },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Dotline.type.label, color = Dotline.colors.ink)
    }
}

/**
 * The modifier chips for the PC's OS as (label, key). The keys are the same everywhere (the PC
 * maps SUPER to the Windows / Command key and ALT to Option); only the labels change.
 */
private fun modifiers(os: String): List<Pair<String, String>> =
    when (os) {
        "macos" -> listOf("⌘ CMD" to "SUPER", "⌥ OPT" to "ALT", "⌃ CTRL" to "CTRL", "⇧ SHIFT" to "SHIFT")
        "windows" -> listOf("CTRL" to "CTRL", "ALT" to "ALT", "SHIFT" to "SHIFT", "WIN" to "SUPER")
        else -> listOf("CTRL" to "CTRL", "ALT" to "ALT", "SHIFT" to "SHIFT", "SUPER" to "SUPER")
    }
private val KEYS =
    listOf(
        "ESC" to "ESC",
        "TAB" to "TAB",
        "⏎" to "ENTER",
        "⌫" to "BACKSPACE",
        "←" to "LEFT",
        "↑" to "UP",
        "↓" to "DOWN",
        "→" to "RIGHT",
        "HOME" to "HOME",
        "END" to "END",
        "DEL" to "DELETE",
        "VOL−" to "VOLUMEDOWN",
        "VOL+" to "VOLUMEUP",
        "⏯" to "PLAYPAUSE",
    )

@Composable
private fun KeyStrip(
    modifiers: List<Pair<String, String>>,
    mods: List<String>,
    onMod: (String) -> Unit,
    onKey: (String) -> Unit,
    keyboardField: @Composable () -> Unit,
) {
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = Dotline.space.s4),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        keyboardField()
        modifiers.forEach { (label, key) ->
            val active = key in mods
            KeyChip(label, active = active, feedback = if (active) HapticFeedbackType.ToggleOff else HapticFeedbackType.ToggleOn) { onMod(key) }
        }
        KEYS.forEach { (label, key) -> KeyChip(label) { onKey(key) } }
    }
}

@Composable
private fun KeyChip(
    label: String,
    active: Boolean = false,
    feedback: HapticFeedbackType = HapticFeedbackType.KeyboardTap,
    onClick: () -> Unit,
) {
    val colors = Dotline.colors
    val haptic = LocalHapticFeedback.current
    Box(
        modifier =
            Modifier
                .height(40.dp)
                .widthIn(min = 48.dp)
                .clip(Dotline.radius.pill)
                .background(if (active) colors.ink else colors.bg)
                .border(Dotline.size.stroke, if (active) colors.ink else colors.lineStrong, Dotline.radius.pill)
                .pointerInput(onClick, feedback) {
                    awaitEachGesture {
                        awaitFirstDown()
                        val up = waitForUpOrCancellation()
                        if (up != null) {
                            haptic.performHapticFeedback(feedback)
                            onClick()
                        }
                    }
                }.padding(horizontal = Dotline.space.s3),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Dotline.type.label, color = if (active) colors.onInk else colors.ink, textAlign = TextAlign.Center)
    }
}

/**
 * A keyboard button with an invisible text field behind it. The field starts with a two-space
 * sentinel; whatever the keyboard adds is sent and each removed character becomes a
 * Backspace. Changes are measured against the last text seen, not the sentinel: a keyboard
 * that commits again before our reset reaches it still holds the previous character, which
 * would otherwise be sent twice. The sentinel comes back once typing pauses, and right away
 * after a Backspace so there is always something left to delete.
 */
@Composable
private fun KeyboardToggle(
    onTyped: (String) -> Unit,
    onKey: (String) -> Unit,
) {
    val focus = remember { FocusRequester() }
    val focusManager = LocalFocusManager.current
    val keyboard = LocalSoftwareKeyboardController.current
    var focused by remember { mutableStateOf(false) }
    val sentinel = TextFieldValue(SENTINEL, TextRange(SENTINEL.length))
    var value by remember { mutableStateOf(sentinel) }
    LaunchedEffect(value.text) {
        if (value.text.length > SENTINEL.length) {
            delay(RESET_MS)
            value = sentinel
        }
    }
    Box {
        BasicTextField(
            value = value,
            onValueChange = { v ->
                val old = value.text
                val t = v.text
                val common = t.commonPrefixWith(old).length
                repeat(old.length - common) { onKey("BACKSPACE") }
                onTyped(t.substring(common))
                value = if (t.length < SENTINEL.length) sentinel else v
            },
            keyboardOptions =
                KeyboardOptions(
                    keyboardType = KeyboardType.Password, // no suggestions or autocorrect
                    autoCorrectEnabled = false,
                    imeAction = ImeAction.Send,
                ),
            keyboardActions = KeyboardActions(onSend = { onKey("ENTER") }),
            modifier =
                Modifier
                    .size(1.dp)
                    .alpha(0f)
                    .focusRequester(focus)
                    .onFocusChanged { focused = it.isFocused },
        )
        KeyChipIcon(active = focused) {
            if (focused) {
                focusManager.clearFocus()
                keyboard?.hide()
            } else {
                focus.requestFocus()
                keyboard?.show()
            }
        }
    }
}

@Composable
private fun KeyChipIcon(
    active: Boolean,
    onClick: () -> Unit,
) {
    val colors = Dotline.colors
    val haptic = LocalHapticFeedback.current
    Box(
        modifier =
            Modifier
                .height(40.dp)
                .widthIn(min = 56.dp)
                .clip(Dotline.radius.pill)
                .background(if (active) colors.ink else colors.surfaceRaised)
                .pointerInput(onClick) {
                    awaitEachGesture {
                        awaitFirstDown()
                        if (waitForUpOrCancellation() != null) {
                            haptic.performHapticFeedback(HapticFeedbackType.KeyboardTap)
                            onClick()
                        }
                    }
                },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            DotlineIcons.Keyboard,
            contentDescription = "Keyboard",
            tint = if (active) colors.onInk else colors.ink,
            modifier = Modifier.size(20.dp),
        )
    }
}
