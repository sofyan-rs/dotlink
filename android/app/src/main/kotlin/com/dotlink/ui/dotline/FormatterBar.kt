/**
 * Formatter bar: the editor's row of 48 dp formatting buttons on `surface`, with a status
 * line under it ("LN 1, COL 8 · 645 CHARS"). It scrolls sideways when the buttons don't fit.
 *
 * Used by Editor-Live and Editor-Source.
 *
 * The bar sits directly above the keyboard: the caller adds `Modifier.imePadding()` to the
 * screen layout that holds it. The bar is data-driven: pass the [FormatAction]s to show, the
 * ones that are active at the cursor, and handle taps in `onAction`.
 */
package com.dotlink.ui.dotline

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Icon size inside the bar (mockups: 22 dp, a bit smaller than the 24 dp grid). */
private val BarIconSize = 22.dp

/** Size of the B / I / S glyphs (mockups: 18 px Geist). */
private val GlyphFontSize = 18.sp

/** Size of the mono H1 and inline-code glyphs (mockups: 14 px Geist Mono). */
private val MonoGlyphFontSize = 14.sp

/** Height of the separator between groups (mockups: 1×24). */
private val SeparatorHeight = 24.dp

/** Space under the status line (mockups: `padding-bottom: 20px`). */
private val StatusBottomPadding = 20.dp

/**
 * One formatting button. [label] is what TalkBack reads; [group] decides where separators go
 * (a thin line is drawn wherever the group changes between neighbours).
 */
enum class FormatAction(
    val label: String,
    val group: Int,
) {
    Bold("Bold", group = 0),
    Italic("Italic", group = 0),
    Strikethrough("Strikethrough", group = 0),
    InlineCode("Inline code", group = 0),
    Heading("Heading", group = 1),
    Link("Link", group = 1),
    Image("Image", group = 1),
    Checklist("Checklist", group = 1),
    BulletList("Bulleted list", group = 1),
    NumberedList("Numbered list", group = 1),
    CodeBlock("Code block", group = 1),
    Quote("Quote", group = 1),
    Table("Table", group = 1),
    Rule("Horizontal rule", group = 1),
}

/** The button sets shown in the mockups. */
object FormatterBarDefaults {
    /** Editor-Live (3.1). */
    val LiveActions: List<FormatAction> =
        listOf(
            FormatAction.Bold,
            FormatAction.Italic,
            FormatAction.Strikethrough,
            FormatAction.Heading,
            FormatAction.Link,
            FormatAction.Image,
            FormatAction.Checklist,
            FormatAction.BulletList,
            FormatAction.CodeBlock,
            FormatAction.Quote,
            FormatAction.Table,
        )

    /** Editor-Source (3.2). */
    val SourceActions: List<FormatAction> =
        listOf(
            FormatAction.Bold,
            FormatAction.Italic,
            FormatAction.Strikethrough,
            FormatAction.Heading,
            FormatAction.Link,
            FormatAction.Image,
            FormatAction.NumberedList,
            FormatAction.CodeBlock,
            FormatAction.Rule,
        )
}

/**
 * The formatter bar and its status line.
 *
 * @param actions buttons to show, in order (see [FormatterBarDefaults]).
 * @param activeActions buttons drawn as "on" (e.g. Heading while the cursor is in a heading).
 * @param line 1-based cursor line, [column] 1-based cursor column, [chars] character count.
 * @param lineEnding shown after the count in Source mode, e.g. "LF" or "CRLF". Null hides it.
 * @param headingLevel the heading level (1–6) of the cursor's line, 0 if it isn't a heading:
 *   the Heading button then reads "H2", "H3", ... instead of "H1".
 */
@Composable
fun FormatterBar(
    actions: List<FormatAction>,
    onAction: (FormatAction) -> Unit,
    line: Int,
    column: Int,
    chars: Int,
    modifier: Modifier = Modifier,
    activeActions: Set<FormatAction> = emptySet(),
    lineEnding: String? = null,
    headingLevel: Int = 0,
) {
    Column(modifier.fillMaxWidth().background(Dotline.colors.surface)) {
        // 1 dp hairline on top (mockups: `border-top: 1px solid line`).
        Box(Modifier.fillMaxWidth().height(1.dp).background(Dotline.colors.line))
        Row(
            modifier =
                Modifier
                    .fillMaxWidth()
                    // rememberScrollState keeps the scroll position across recompositions.
                    .horizontalScroll(rememberScrollState())
                    .padding(horizontal = Dotline.space.s2, vertical = Dotline.space.s1),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            actions.forEachIndexed { index, action ->
                if (index > 0 && actions[index - 1].group != action.group) {
                    Separator()
                }
                FormatButton(
                    action = action,
                    active = action in activeActions,
                    onClick = { onAction(action) },
                    glyph = if (action == FormatAction.Heading && headingLevel in 1..6) "H$headingLevel" else glyphFor(action),
                )
            }
        }
        StatusLine(line = line, column = column, chars = chars, lineEnding = lineEnding)
    }
}

/** One 48 dp button. Active or pressed buttons get a `surfaceRaised` square behind them. */
@Composable
private fun FormatButton(
    action: FormatAction,
    active: Boolean,
    onClick: () -> Unit,
    glyph: String,
) {
    // One InteractionSource shared by clickable, focusRing and our pressed look, so they
    // all react to the same touch. `remember` keeps the same object across recompositions.
    val interactionSource = remember { MutableInteractionSource() }
    val pressed by interactionSource.collectIsPressedAsState()
    val shape = Dotline.radius.s
    val background = if (active || pressed) Dotline.colors.surfaceRaised else Color.Transparent
    Box(
        modifier =
            Modifier
                .size(Dotline.size.touchTarget)
                .focusRing(interactionSource, shape)
                .background(background, shape)
                // indication = null: no Material ripple, the background above is our feedback.
                .clickable(interactionSource = interactionSource, indication = null, role = Role.Button, onClick = onClick)
                .semantics {
                    contentDescription = action.label
                    selected = active
                },
        contentAlignment = Alignment.Center,
    ) {
        val icon = iconFor(action)
        if (icon != null) {
            Icon(icon, contentDescription = null, tint = Dotline.colors.ink, modifier = Modifier.size(BarIconSize))
        } else {
            // clearAndSetSemantics {}: the glyph "B" is decoration; TalkBack reads "Bold" instead.
            Text(
                text = glyph,
                style = glyphStyle(action),
                color = Dotline.colors.ink,
                modifier = Modifier.clearAndSetSemantics {},
            )
        }
    }
}

/** Icon for the action, or null for actions drawn as a text glyph. */
private fun iconFor(action: FormatAction): ImageVector? =
    when (action) {
        FormatAction.Link -> DotlineIcons.Link
        FormatAction.Image -> DotlineIcons.Image
        FormatAction.Checklist -> DotlineIcons.Checklist
        FormatAction.BulletList -> DotlineIcons.BulletList
        FormatAction.NumberedList -> DotlineIcons.NumberedList
        FormatAction.CodeBlock -> DotlineIcons.Code
        FormatAction.Quote -> DotlineIcons.Quote
        FormatAction.Table -> DotlineIcons.Table
        FormatAction.Rule -> DotlineIcons.Rule
        FormatAction.Bold, FormatAction.Italic, FormatAction.Strikethrough,
        FormatAction.InlineCode, FormatAction.Heading,
        -> null
    }

private fun glyphFor(action: FormatAction): String =
    when (action) {
        FormatAction.Bold -> "B"
        FormatAction.Italic -> "I"
        FormatAction.Strikethrough -> "S"
        FormatAction.Heading -> "H1"
        // Not in the mockups: a backtick pair around a letter, like Markdown inline code.
        FormatAction.InlineCode -> "`a`"
        else -> ""
    }

/** B is bold, I italic, S struck through; H1 and inline code use the mono face. */
@Composable
private fun glyphStyle(action: FormatAction): TextStyle {
    val text = Dotline.type.body.copy(fontSize = GlyphFontSize, lineHeight = GlyphFontSize)
    val mono = Dotline.type.data.copy(fontSize = MonoGlyphFontSize, lineHeight = MonoGlyphFontSize)
    return when (action) {
        FormatAction.Bold -> text.copy(fontWeight = FontWeight.Bold)
        FormatAction.Italic -> text.copy(fontStyle = FontStyle.Italic)
        FormatAction.Strikethrough -> text.copy(textDecoration = TextDecoration.LineThrough)
        else -> mono
    }
}

/** Thin vertical line between groups (1×24 dp, `line`, 4 dp each side). */
@Composable
private fun Separator() {
    Box(
        Modifier
            .padding(horizontal = Dotline.space.s1)
            .width(1.dp)
            .height(SeparatorHeight)
            .background(Dotline.colors.line),
    )
}

/** "LN 1, COL 8" on the left, "645 CHARS · LF" on the right, in `label` style. */
@Composable
private fun StatusLine(
    line: Int,
    column: Int,
    chars: Int,
    lineEnding: String?,
) {
    val style = Dotline.type.label
    val color = Dotline.colors.inkFaint
    val count = if (lineEnding != null) "$chars CHARS · $lineEnding" else "$chars CHARS"
    Row(
        modifier =
            Modifier
                .fillMaxWidth()
                .padding(start = Dotline.space.s4, end = Dotline.space.s4, bottom = StatusBottomPadding),
    ) {
        Text("LN $line, COL $column", style = style, color = color, modifier = Modifier.weight(1f))
        Text(count, style = style, color = color)
    }
}

@Composable
private fun FormatterBarPreviewContent() {
    Box(Modifier.background(Dotline.colors.bg).padding(vertical = 16.dp)) {
        Column {
            FormatterBar(
                actions = FormatterBarDefaults.LiveActions,
                activeActions = setOf(FormatAction.Heading),
                onAction = {},
                line = 1,
                column = 8,
                chars = 645,
            )
            Box(Modifier.height(16.dp))
            FormatterBar(
                actions = FormatterBarDefaults.SourceActions,
                activeActions = setOf(FormatAction.NumberedList),
                onAction = {},
                line = 20,
                column = 15,
                chars = 652,
                lineEnding = "LF",
            )
        }
    }
}

@Preview(name = "Dark", widthDp = 390)
@Composable
private fun FormatterBarDarkPreview() {
    DotlinkTheme(dark = true) { FormatterBarPreviewContent() }
}

@Preview(name = "Light", widthDp = 390)
@Composable
private fun FormatterBarLightPreview() {
    DotlinkTheme(dark = false) { FormatterBarPreviewContent() }
}
