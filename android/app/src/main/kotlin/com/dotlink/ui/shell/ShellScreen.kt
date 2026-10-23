/**
 * The SHELL tab: a real shell on the PC, drawn by xterm.js in a WebView.
 * The session keeps running on the PC when you leave; coming back replays the screen.
 */
package com.dotlink.ui.shell

import android.annotation.SuppressLint
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.util.Base64
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.webkit.JavascriptInterface
import android.webkit.WebView
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
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.dotlink.DotlinkApp
import com.dotlink.link.Link
import com.dotlink.link.LinkState
import com.dotlink.ui.dotline.DotlineIcons
import com.dotlink.ui.dotline.ScreenTitle
import com.dotlink.ui.dotline.ScreenTitleAction
import com.dotlink.ui.dotline.bottomNavHeight
import com.dotlink.ui.theme.Dotline
import kotlinx.coroutines.delay
import okhttp3.Call
import okhttp3.Callback
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import org.json.JSONObject
import java.io.IOException

/** Called by term.html (on the WebView's JavaBridge thread). */
class TermBridge(
    private val controller: TermController,
) {
    @JavascriptInterface fun input(data: String) = controller.onInput(data)

    @JavascriptInterface fun resize(
        cols: Int,
        rows: Int,
    ) = controller.onResize(cols, rows)

    @JavascriptInterface fun ready(
        cols: Int,
        rows: Int,
    ) = controller.onReady(cols, rows)
}

/** One shell session on the PC (a tab). */
data class TermTab(
    val id: Int,
    val title: String,
)

/** The session last shown, so coming back to the tab reopens it. */
private var lastSession: Int? = null

class TermController(
    private val link: Link,
) {
    var webView: WebView? = null
    private var ws: WebSocket? = null
    private val main = Handler(Looper.getMainLooper())
    private var cols = 80
    private var rows = 24
    private var ready = false

    var status by mutableStateOf("Not connected")

    /** [status] is an error (wrong PIN, shell turned off): shown in red. */
    var statusError by mutableStateOf(false)
    var ctrl by mutableStateOf(false)
    var alt by mutableStateOf(false)
    var sessions by mutableStateOf(emptyList<TermTab>())
    var active by mutableStateOf(lastSession)

    private val pinHeaders: Map<String, String>
        get() = link.prefs.terminalPin.let { if (it.isNotEmpty()) mapOf("X-Terminal-Pin" to it) else emptyMap() }

    fun onReady(
        c: Int,
        r: Int,
    ) {
        cols = c
        rows = r
        ready = true
        main.post {
            js("termFontSize(${link.prefs.terminalFontSize})")
            open(active)
        }
    }

    fun onResize(
        c: Int,
        r: Int,
    ) {
        cols = c
        rows = r
        ws?.send(JSONObject().put("t", "resize").put("cols", c).put("rows", r).toString())
        main.post { if (ws != null) status = "${link.prefs.serverName} · ${c}×$r" }
    }

    /** Text from xterm.js (keyboard) or the key row. Sticky Ctrl/Alt apply to the next key. */
    fun onInput(data: String) {
        var out = data
        if (ctrl && out.length == 1) {
            val c = out[0].uppercaseChar()
            if (c in '@'..'_') out = (c.code - 64).toChar().toString()
            main.post { ctrl = false }
        }
        if (alt) {
            out = "\u001b" + out
            main.post { alt = false }
        }
        ws?.send(out.encodeToByteArray().toByteString())
    }

    /** Shows session [id] (null = the first one), or starts a new one when [fresh]. */
    fun open(
        id: Int?,
        fresh: Boolean = false,
    ) {
        if (!ready) return
        ws?.close(1000, null)
        ws = null
        js("termReset()") // the PC replays the session's screen
        status = "Connecting…"
        statusError = false
        val query = "/ws/term?cols=$cols&rows=$rows" + (if (fresh) "&new=true" else id?.let { "&id=$it" }.orEmpty())
        val socket =
            link.openSocket(
                query,
                object : WebSocketListener() {
                    // Output from a socket we already replaced must not reach the screen.
                    private fun current(webSocket: WebSocket) = webSocket === ws

                    override fun onMessage(
                        webSocket: WebSocket,
                        bytes: ByteString,
                    ) {
                        val b64 = Base64.encodeToString(bytes.toByteArray(), Base64.NO_WRAP)
                        main.post { if (current(webSocket)) js("termWrite('$b64')") }
                    }

                    override fun onMessage(
                        webSocket: WebSocket,
                        text: String,
                    ) {
                        val msg = JSONObject(text)
                        main.post {
                            if (!current(webSocket)) return@post
                            when (msg.optString("t")) {
                                "session" -> {
                                    active = msg.getInt("id")
                                    lastSession = active
                                    status = "${link.prefs.serverName} · ${cols}×$rows"
                                    refresh()
                                }
                                "exit" -> {
                                    status = "Shell exited"
                                    refresh()
                                }
                            }
                        }
                    }

                    override fun onFailure(
                        webSocket: WebSocket,
                        t: Throwable,
                        response: Response?,
                    ) {
                        val reason =
                            if (response?.code == 403) {
                                response.body?.string()?.ifBlank { null } ?: "Not allowed"
                            } else {
                                t.message ?: "Disconnected"
                            }
                        main.post {
                            if (current(webSocket)) {
                                status = reason
                                statusError = true
                            }
                        }
                    }
                },
                headers = pinHeaders,
            )
        ws = socket
        if (socket == null) status = "Not connected"
    }

    /** Reloads the tab list (titles follow the program running in each session). */
    fun refresh() {
        val req = link.apiRequest("/terms", pinHeaders)?.build() ?: return
        link.http().newCall(req).enqueue(
            object : Callback {
                override fun onFailure(
                    call: Call,
                    e: IOException,
                ) = Unit

                override fun onResponse(
                    call: Call,
                    response: Response,
                ) {
                    val body = response.use { if (it.isSuccessful) it.body?.string() else null } ?: return
                    val arr = JSONObject(body).optJSONArray("sessions") ?: return
                    val tabs = (0 until arr.length()).map { arr.getJSONObject(it).let { o -> TermTab(o.getInt("id"), o.optString("title")) } }
                    main.post { sessions = tabs }
                }
            },
        )
    }

    /** Ends session [id] on the PC and moves to a neighbour (or a new shell if it was the last). */
    fun closeSession(id: Int) {
        val req = link.apiRequest("/terms/$id", pinHeaders)?.delete()?.build() ?: return
        val next = sessions.filter { it.id != id }.let { rest -> rest.lastOrNull { it.id < id } ?: rest.firstOrNull() }
        link.http().newCall(req).enqueue(
            object : Callback {
                override fun onFailure(
                    call: Call,
                    e: IOException,
                ) = Unit

                override fun onResponse(
                    call: Call,
                    response: Response,
                ) {
                    response.close()
                    main.post {
                        if (active == id) open(next?.id, fresh = next == null) else refresh()
                    }
                }
            },
        )
    }

    fun close() {
        ws?.close(1000, null)
        ws = null
    }

    fun js(code: String) {
        webView?.evaluateJavascript(code, null)
    }

    fun showKeyboard() {
        val wv = webView ?: return
        wv.requestFocus()
        js("termFocus()")
        wv.context.getSystemService(InputMethodManager::class.java).showSoftInput(wv, 0)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@SuppressLint("SetJavaScriptEnabled")
@Composable
fun ShellScreen() {
    val link = DotlinkApp.link
    val state by link.state.collectAsStateWithLifecycle()
    val controller = remember { TermController(link) }
    val online = state is LinkState.Online
    DisposableEffect(online) {
        if (online) controller.open(controller.active)
        onDispose { controller.close() }
    }
    // Keep tab titles current ("zsh" → "htop") while the screen is open.
    LaunchedEffect(online) {
        while (online) {
            controller.refresh()
            delay(3_000)
        }
    }
    DisposableEffect(Unit) {
        onDispose {
            controller.webView?.destroy()
            controller.webView = null
        }
    }
    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.statusBars)
            .imePadding()
            .padding(top = Dotline.space.s3),
    ) {
        ScreenTitle(
            title = "Shell",
            subline = controller.status,
            sublineError = controller.statusError,
            actions = {
                ScreenTitleAction(DotlineIcons.Keyboard, "Show keyboard", onClick = controller::showKeyboard)
            },
        )
        Spacer(Modifier.height(Dotline.space.s2))
        SessionTabs(controller)
        Spacer(Modifier.height(Dotline.space.s3))
        AndroidView(
            factory = { ctx ->
                WebView(ctx).apply {
                    // AndroidView defaults to WRAP_CONTENT, which makes the page 0 px tall
                    // (xterm.js then fits a single row). Fill the space Compose gives us.
                    layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
                    setBackgroundColor(Color.BLACK)
                    settings.javaScriptEnabled = true
                    settings.domStorageEnabled = true
                    addJavascriptInterface(TermBridge(controller), "Bridge")
                    // Re-measure rows/cols whenever the view changes size (keyboard, rotation).
                    addOnLayoutChangeListener { _, _, top, _, bottom, _, oldTop, _, oldBottom ->
                        if (bottom - top != oldBottom - oldTop) controller.js("window.refit && refit()")
                    }
                    loadUrl("file:///android_asset/term/term.html")
                    controller.webView = this
                }
            },
            modifier =
                Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    // The screen margin, like the tabs and keys around it.
                    .padding(horizontal = Dotline.space.s4)
                    .clip(Dotline.radius.m)
                    .background(androidx.compose.ui.graphics.Color.Black),
        )
        Spacer(Modifier.height(Dotline.space.s3))
        ExtraKeys(controller)
        Spacer(Modifier.height(if (WindowInsets.isImeVisible) Dotline.space.s2 else bottomNavHeight()))
    }
}

/** One pill per session ("1 ZSH", "2 HTOP"); the active one carries its own ×. + opens a new one. */
@Composable
private fun SessionTabs(controller: TermController) {
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = Dotline.space.s4),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        controller.sessions.forEach { tab ->
            SessionTab(
                label = "${tab.id} ${tab.title}".uppercase(),
                selected = tab.id == controller.active,
                onSelect = { if (tab.id != controller.active) controller.open(tab.id) },
                onClose = { controller.closeSession(tab.id) },
            )
        }
        NewSessionButton { controller.open(null, fresh = true) }
    }
}

/** + at the end of the session tabs: a 40 dp circle like the tabs, with a real icon. */
@Composable
private fun NewSessionButton(onClick: () -> Unit) {
    val colors = Dotline.colors
    Box(
        modifier =
            Modifier
                .size(40.dp)
                .clip(Dotline.radius.pill)
                .border(Dotline.size.stroke, colors.lineStrong, Dotline.radius.pill)
                .pointerInput(onClick) {
                    awaitEachGesture {
                        awaitFirstDown()
                        if (waitForUpOrCancellation() != null) onClick()
                    }
                },
        contentAlignment = Alignment.Center,
    ) {
        Icon(DotlineIcons.Plus, contentDescription = "New session", tint = colors.ink, modifier = Modifier.size(20.dp))
    }
}

@Composable
private fun SessionTab(
    label: String,
    selected: Boolean,
    onSelect: () -> Unit,
    onClose: () -> Unit,
) {
    val colors = Dotline.colors
    val content = if (selected) colors.onInk else colors.ink
    Row(
        modifier =
            Modifier
                .height(40.dp)
                .clip(Dotline.radius.pill)
                .background(if (selected) colors.ink else colors.bg)
                .border(Dotline.size.stroke, if (selected) colors.ink else colors.lineStrong, Dotline.radius.pill)
                .tap(onSelect)
                .padding(start = Dotline.space.s3, end = if (selected) 0.dp else Dotline.space.s3),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = Dotline.type.label, color = content)
        if (selected) {
            // The × is part of the tab; its own 40 dp tap area keeps it easy to hit.
            Box(
                Modifier
                    .size(40.dp)
                    .tap(onClose),
                contentAlignment = Alignment.Center,
            ) {
                Icon(DotlineIcons.Close, contentDescription = "Close session", tint = content, modifier = Modifier.size(16.dp))
            }
        }
    }
}

/** Tap without a ripple (Dotline uses no Material ripples). */
private fun Modifier.tap(onClick: () -> Unit) =
    pointerInput(onClick) {
        awaitEachGesture {
            awaitFirstDown()
            if (waitForUpOrCancellation() != null) onClick()
        }
    }

private val KEYS =
    listOf(
        "ESC" to "\u001b",
        "TAB" to "\t",
        "←" to "\u001b[D",
        "↑" to "\u001b[A",
        "↓" to "\u001b[B",
        "→" to "\u001b[C",
        "|" to "|",
        "~" to "~",
        "/" to "/",
        "-" to "-",
        "HOME" to "\u001b[H",
        "END" to "\u001b[F",
        "PGUP" to "\u001b[5~",
        "PGDN" to "\u001b[6~",
    )

@Composable
private fun ExtraKeys(controller: TermController) {
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = Dotline.space.s4),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2),
    ) {
        Key("CTRL", active = controller.ctrl) { controller.ctrl = !controller.ctrl }
        Key("ALT", active = controller.alt) { controller.alt = !controller.alt }
        KEYS.forEach { (label, seq) -> Key(label) { controller.onInput(seq) } }
    }
}

@Composable
private fun Key(
    label: String,
    active: Boolean = false,
    onClick: () -> Unit,
) {
    val colors = Dotline.colors
    Box(
        modifier =
            Modifier
                .height(40.dp)
                .widthIn(min = 44.dp)
                .clip(Dotline.radius.pill)
                .background(if (active) colors.ink else colors.bg)
                .border(Dotline.size.stroke, if (active) colors.ink else colors.lineStrong, Dotline.radius.pill)
                .pointerInput(onClick) {
                    awaitEachGesture {
                        awaitFirstDown()
                        if (waitForUpOrCancellation() != null) onClick()
                    }
                }.padding(horizontal = Dotline.space.s3),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = Dotline.type.label, color = if (active) colors.onInk else colors.ink)
    }
}
