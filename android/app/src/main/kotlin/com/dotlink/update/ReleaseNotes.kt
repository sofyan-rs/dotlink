/**
 * Release notes as written on GitHub: the bit of Markdown the notes use (headings, list items,
 * **bold**, `code`, [links](…)), drawn with the app's type instead of shown as raw text.
 */
package com.dotlink.update

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.withStyle
import com.dotlink.ui.dotline.SectionLabel
import com.dotlink.ui.theme.Dotline

@Composable
fun ReleaseNotes(
    markdown: String,
    modifier: Modifier = Modifier,
) {
    val ink = Dotline.colors.ink
    val mono = Dotline.type.data.fontFamily
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        for (block in blocks(markdown)) {
            when (block) {
                is Block.Heading ->
                    SectionLabel(block.text, Modifier.padding(top = Dotline.space.s2))
                is Block.Item ->
                    Row(horizontalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
                        Text("•", style = Dotline.type.body, color = Dotline.colors.inkMuted)
                        Text(inline(block.text, ink, mono), style = Dotline.type.body, color = Dotline.colors.inkMuted)
                    }
                is Block.Paragraph ->
                    Text(inline(block.text, ink, mono), style = Dotline.type.body, color = Dotline.colors.inkMuted)
            }
        }
    }
}

private sealed interface Block {
    val text: String

    data class Heading(
        override val text: String,
    ) : Block

    data class Item(
        override val text: String,
    ) : Block

    data class Paragraph(
        override val text: String,
    ) : Block
}

/** Lines into headings, list items and paragraphs; a list item's wrapped lines join it. */
private fun blocks(markdown: String): List<Block> {
    val out = mutableListOf<Block>()
    for (raw in markdown.lines()) {
        val line = raw.trimEnd()
        val trimmed = line.trimStart()
        when {
            trimmed.isEmpty() -> continue
            trimmed.startsWith("#") -> out += Block.Heading(trimmed.trimStart('#').trim())
            trimmed.startsWith("- ") || trimmed.startsWith("* ") -> out += Block.Item(trimmed.substring(2).trim())
            // A continuation of the previous item or paragraph (indented, or a soft line break).
            out.lastOrNull() is Block.Item && line.startsWith(" ") ->
                out[out.lastIndex] = Block.Item((out.last() as Block.Item).text + " " + trimmed)
            out.lastOrNull() is Block.Paragraph ->
                out[out.lastIndex] = Block.Paragraph((out.last() as Block.Paragraph).text + " " + trimmed)
            else -> out += Block.Paragraph(trimmed)
        }
    }
    return out
}

/** **bold** in the ink colour, `code` in mono, [text](url) as its text. */
private fun inline(
    text: String,
    ink: androidx.compose.ui.graphics.Color,
    mono: androidx.compose.ui.text.font.FontFamily?,
): AnnotatedString =
    buildAnnotatedString {
        var i = 0
        while (i < text.length) {
            when {
                text.startsWith("**", i) && text.indexOf("**", i + 2) > 0 -> {
                    val end = text.indexOf("**", i + 2)
                    withStyle(SpanStyle(fontWeight = FontWeight.Medium, color = ink)) { append(text.substring(i + 2, end)) }
                    i = end + 2
                }
                text[i] == '`' && text.indexOf('`', i + 1) > 0 -> {
                    val end = text.indexOf('`', i + 1)
                    withStyle(SpanStyle(fontFamily = mono, color = ink)) { append(text.substring(i + 1, end)) }
                    i = end + 1
                }
                text[i] == '[' && text.indexOf("](", i) > 0 && text.indexOf(')', text.indexOf("](", i)) > 0 -> {
                    val close = text.indexOf("](", i)
                    append(text.substring(i + 1, close))
                    i = text.indexOf(')', close) + 1
                }
                else -> {
                    append(text[i])
                    i++
                }
            }
        }
    }
