/**
 * Dotline type: Doto (dot-matrix display), Geist (reading), Geist Mono (labels and data).
 *
 * All three are variable fonts, so each weight is the same file with a different
 * `FontVariation.weight`. Never use Doto under 24 sp or for body text.
 */
package com.dotlink.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.dotlink.R

/** One `Font` entry of a variable font file, pinned to [weight]. */
private fun variableFont(
    resId: Int,
    weight: FontWeight,
) = Font(
    resId = resId,
    weight = weight,
    variationSettings = FontVariation.Settings(FontVariation.weight(weight.weight)),
)

val Doto =
    FontFamily(
        variableFont(R.font.doto, FontWeight.Normal),
        variableFont(R.font.doto, FontWeight.Bold),
        variableFont(R.font.doto, FontWeight.Black),
    )

val Geist =
    FontFamily(
        variableFont(R.font.geist, FontWeight.Normal),
        variableFont(R.font.geist, FontWeight.Medium),
        variableFont(R.font.geist, FontWeight.SemiBold),
        variableFont(R.font.geist, FontWeight.Bold),
    )

val GeistMono =
    FontFamily(
        variableFont(R.font.geist_mono, FontWeight.Normal),
        variableFont(R.font.geist_mono, FontWeight.Medium),
        variableFont(R.font.geist_mono, FontWeight.Bold),
    )

@Immutable
data class DotlineType(
    /** One hero number (sync "2 MIN AGO"). */
    val dotXl: TextStyle =
        TextStyle(fontFamily = Doto, fontSize = 72.sp, lineHeight = 72.sp, fontWeight = FontWeight.Black, letterSpacing = (-0.02).em),
    /** Widget values. */
    val dotL: TextStyle = TextStyle(fontFamily = Doto, fontSize = 48.sp, lineHeight = 52.sp, fontWeight = FontWeight.Black),
    /** Screen titles in CAPS (`PERSONAL`, `SEARCH`). */
    val dotM: TextStyle = TextStyle(fontFamily = Doto, fontSize = 28.sp, lineHeight = 32.sp, fontWeight = FontWeight.Bold),
    /** Sheet and dialog titles. */
    val title: TextStyle =
        TextStyle(
            fontFamily = Geist,
            fontSize = 22.sp,
            lineHeight = 28.sp,
            fontWeight = FontWeight.Medium,
            letterSpacing = (-0.01).em,
        ),
    /** Row titles, big buttons. */
    val headline: TextStyle = TextStyle(fontFamily = Geist, fontSize = 17.sp, lineHeight = 24.sp, fontWeight = FontWeight.Medium),
    /** Paragraphs, buttons. */
    val body: TextStyle = TextStyle(fontFamily = Geist, fontSize = 15.sp, lineHeight = 22.sp, fontWeight = FontWeight.Normal),
    /** Row subtitles, helper text. */
    val bodyS: TextStyle = TextStyle(fontFamily = Geist, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Normal),
    /** Overlines, tab labels, chips. Always uppercase the string yourself. */
    val label: TextStyle =
        TextStyle(fontFamily = GeistMono, fontSize = 11.sp, lineHeight = 14.sp, fontWeight = FontWeight.Normal, letterSpacing = 0.12.em),
    /** Paths, counts, status. */
    val data: TextStyle = TextStyle(fontFamily = GeistMono, fontSize = 13.sp, lineHeight = 18.sp, fontWeight = FontWeight.Normal),
)
