/**
 * Dotline text inputs: [DotlineTextField] for names and search,
 * [DotlinePasswordField] for passwords, plus two small trailing helpers,
 * [DotlineTextFieldIconButton] (clear, show password) and [DotlineTextFieldStatus] (MATCH, a count).
 *
 * Used on: Rename (NAME + ".md" + clear), New-Item (NAME + ".md", placeholder), Set-Password and
 * Change-Password (password, show password, MATCH, error), Unlock (VAULT PASSWORD) and
 * Quick-Switcher (search icon + result count).
 *
 * Nothing OS 5.0 style: label above in `label` style, 8 dp gap; the box is a 56 dp borderless pill,
 * filled `surface` on a screen and `surfaceRaised` in a sheet (so it always stands out from what
 * is behind it). A 1.5 dp border shows only while focused (`lineStrong`) or on error (`accent`). 16 dp padding
 * (4 dp on the right when a 48 dp icon button sits there); helper or error text 8 dp below
 * (`bodyS`, error in `accentText`). Text is 17 sp Geist; passwords are 17 sp Geist Mono
 * with +0.12em tracking. The caret is a 2 dp `accent` line.
 *
 * These use the state-based `BasicTextField(state = TextFieldState)` API: the caller creates the
 * state with `rememberTextFieldState()` and reads `state.text`.
 */
package com.dotlink.ui.dotline

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicSecureTextField
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.KeyboardActionHandler
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.TextObfuscationMode
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.foundation.text.selection.TextSelectionColors
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.error
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.dotlink.ui.theme.Dotline
import com.dotlink.ui.theme.DotlinkTheme

/** Height of the input box (mockups: 56 dp). */
private val FieldHeight = 56.dp

/** Leading icon (search) size and gap to the text (Quick-Switcher: 22 dp, 12 dp). */
private val LeadingIconSize = 22.dp

/**
 * A single-line text input with a label, optional suffix, trailing slot and helper/error text.
 *
 * @param state the text; create it with `rememberTextFieldState("Example")`.
 * @param label the overline above the box ("Name"), shown uppercase. `null` for no label
 *   (then give [placeholder] so screen readers still know what the field is for).
 * @param placeholder grey hint shown while the field is empty ("Find a note by name").
 * @param leadingIcon optional icon at the start of the box (search).
 * @param suffix fixed text after the input that the user can't edit (".md").
 * @param trailing a slot at the end of the box, usually [DotlineTextFieldIconButton] or
 *   [DotlineTextFieldStatus]. A "slot" is a composable parameter: the caller decides what goes there.
 * @param helperText grey text under the box. Hidden while [errorText] is shown.
 * @param errorText when not null the border turns `accent` and this text is shown in `accentText`.
 * @param inSheet true when the field sits in a bottom sheet: it is filled `surfaceRaised`
 *   instead of `surface`, which is the sheet's own colour.
 */
@Composable
fun DotlineTextField(
    state: TextFieldState,
    label: String?,
    modifier: Modifier = Modifier,
    placeholder: String? = null,
    leadingIcon: ImageVector? = null,
    suffix: String? = null,
    trailing: (@Composable () -> Unit)? = null,
    helperText: String? = null,
    errorText: String? = null,
    enabled: Boolean = true,
    inSheet: Boolean = false,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    onKeyboardAction: KeyboardActionHandler? = null,
    /** Applied to the editable text itself, e.g. `Modifier.focusRequester(...)` to focus it. */
    textFieldModifier: Modifier = Modifier,
) {
    // We create the InteractionSource so we can read "is focused" for the border colour.
    val interactionSource = remember { MutableInteractionSource() }
    val focused by interactionSource.collectIsFocusedAsState()
    val textStyle = Dotline.type.headline.copy(fontWeight = FontWeight.Normal)

    FieldColumn(label = label, helperText = helperText, errorText = errorText, modifier = modifier) {
        WithDotlineSelectionColors {
            BasicTextField(
                state = state,
                modifier = Modifier.fillMaxWidth().then(textFieldModifier).fieldSemantics(label, errorText),
                enabled = enabled,
                textStyle = textStyle.copy(color = if (enabled) Dotline.colors.ink else Dotline.colors.inkMuted),
                keyboardOptions = keyboardOptions,
                onKeyboardAction = onKeyboardAction,
                lineLimits = TextFieldLineLimits.SingleLine,
                interactionSource = interactionSource,
                // The caret: BasicTextField draws it 2 dp wide in this brush's colour.
                cursorBrush = SolidColor(Dotline.colors.accent),
                // The decorator draws everything around the editable text; we must call
                // innerTextField() exactly once, where the text should go.
                decorator = { innerTextField ->
                    FieldBox(
                        innerTextField = innerTextField,
                        showPlaceholder = placeholder != null && state.text.isEmpty(),
                        placeholder = placeholder,
                        placeholderStyle = textStyle,
                        leadingIcon = leadingIcon,
                        suffix = suffix,
                        trailing = trailing,
                        focused = focused,
                        isError = errorText != null,
                        inSheet = inSheet,
                    )
                },
            )
        }
    }
}

/**
 * A 48 dp icon button for the end of a text field (clear, show password).
 *
 * @param contentDescription what the button does ("Clear name"); required, the icon has no text.
 * @param iconSize 22 dp by default (eye); the clear "×" in Rename is 18 dp.
 */
@Composable
fun DotlineTextFieldIconButton(
    icon: ImageVector,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    iconSize: Dp = 22.dp,
    tint: Color = Dotline.colors.inkMuted,
) {
    val interactionSource = remember { MutableInteractionSource() }
    Box(
        modifier =
            modifier
                .pressScale(interactionSource)
                .focusRing(interactionSource, CircleShape)
                .size(Dotline.size.touchTarget)
                .clickable(
                    interactionSource = interactionSource,
                    indication = null,
                    role = Role.Button,
                    onClick = onClick,
                ),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = contentDescription, tint = tint, modifier = Modifier.size(iconSize))
    }
}

/**
 * Short status text at the end of a text field, in `label` style.
 *
 * @param text e.g. "Match" (shown uppercase) or a result count ("2").
 * @param positive true for a confirmation: `positive` colour with a check glyph in front
 *   (status never relies on colour alone). False: plain `inkMuted` text.
 */
@Composable
fun DotlineTextFieldStatus(
    text: String,
    modifier: Modifier = Modifier,
    positive: Boolean = false,
) {
    val color = if (positive) Dotline.colors.positive else Dotline.colors.inkMuted
    // 12 dp end padding + the field's 4 dp = the usual 16 dp from the border.
    Row(
        modifier = modifier.padding(end = Dotline.space.s3),
        horizontalArrangement = Arrangement.spacedBy(Dotline.space.s1),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (positive) {
            Icon(DotlineIcons.Check, contentDescription = null, tint = color, modifier = Modifier.size(14.dp))
        }
        Text(text = text.uppercase(), style = Dotline.type.label, color = color)
    }
}

// ---- Shared private pieces ------------------------------------------------------------------

/** Label above, the field in the middle, helper or error text below, 8 dp apart. */
@Composable
internal fun FieldColumn(
    label: String?,
    helperText: String?,
    errorText: String?,
    modifier: Modifier,
    field: @Composable () -> Unit,
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(Dotline.space.s2)) {
        if (label != null) {
            Text(
                text = label.uppercase(),
                style = Dotline.type.label,
                color = Dotline.colors.inkMuted,
                modifier = Modifier.padding(horizontal = Dotline.space.s1),
            )
        }
        field()
        val below = errorText ?: helperText
        if (below != null) {
            Text(
                text = below,
                style = Dotline.type.bodyS,
                color = if (errorText != null) Dotline.colors.accentText else Dotline.colors.inkMuted,
                modifier = Modifier.padding(horizontal = Dotline.space.s1),
            )
        }
    }
}

/** The 56 dp pill drawn around the editable text (used inside a `decorator`). */
@Composable
internal fun FieldBox(
    innerTextField: @Composable () -> Unit,
    showPlaceholder: Boolean,
    placeholder: String?,
    placeholderStyle: TextStyle,
    leadingIcon: ImageVector?,
    suffix: String?,
    trailing: (@Composable () -> Unit)?,
    focused: Boolean,
    isError: Boolean,
    inSheet: Boolean,
) {
    val colors = Dotline.colors
    val shape = Dotline.radius.pill
    // No border while idle; a soft one while typing, red on error.
    val borderColor by animateColorAsState(
        targetValue =
            when {
                isError -> colors.accent
                focused -> colors.lineStrong
                else -> Color.Transparent
            },
        animationSpec = tween(DOTLINE_MOTION_MILLIS),
        label = "fieldBorder",
    )
    Row(
        modifier =
            Modifier
                .height(FieldHeight)
                .background(if (inSheet) colors.surfaceRaised else colors.surface, shape)
                .border(Dotline.size.stroke, borderColor, shape)
                // A trailing 48 dp button brings its own space, so only 4 dp on the right then.
                .padding(start = Dotline.space.s4, end = if (trailing != null) Dotline.space.s1 else Dotline.space.s4),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (leadingIcon != null) {
            Icon(leadingIcon, contentDescription = null, tint = colors.inkMuted, modifier = Modifier.size(LeadingIconSize))
            Spacer(Modifier.width(Dotline.space.s3))
        }
        // weight(1f): the text takes all the space left over by the icon, suffix and trailing.
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (showPlaceholder && placeholder != null) {
                Text(placeholder, style = placeholderStyle, color = colors.inkFaint, maxLines = 1)
            }
            innerTextField()
        }
        if (suffix != null) {
            Text(
                text = suffix,
                style = Dotline.type.data,
                color = colors.inkFaint,
                modifier = Modifier.padding(end = if (trailing != null) Dotline.space.s1 else 0.dp),
            )
        }
        trailing?.invoke()
    }
}

/** Selected text gets a `lineStrong` background (mockup) and `accent` handles, like the caret. */
@Composable
internal fun WithDotlineSelectionColors(content: @Composable () -> Unit) {
    val selection = TextSelectionColors(handleColor = Dotline.colors.accent, backgroundColor = Dotline.colors.lineStrong)
    CompositionLocalProvider(LocalTextSelectionColors provides selection, content = content)
}

/** Screen readers read the label as the field's name and the error text as its error. */
internal fun Modifier.fieldSemantics(
    label: String?,
    errorText: String?,
): Modifier =
    semantics {
        if (label != null) contentDescription = label
        if (errorText != null) error(errorText)
    }

// ---- Previews -------------------------------------------------------------------------------

// Previews show the common cases; ControlsGallery.kt has every variant with live state.
@Composable
private fun TextFieldPreviewContent() {
    val name = rememberTextFieldState("Example")
    val current = rememberTextFieldState("wrongpass")
    var visible by remember { mutableStateOf(false) }
    Box(Modifier.background(Dotline.colors.bg).padding(16.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            DotlineTextField(
                state = name,
                label = "Name",
                suffix = ".md",
                trailing = { DotlineTextFieldIconButton(DotlineIcons.Close, "Clear name", onClick = {}, iconSize = 18.dp) },
                helperText = "The file is renamed on disk. Open tabs follow it.",
            )
            DotlinePasswordField(
                state = current,
                label = "Current password",
                passwordVisible = visible,
                onPasswordVisibleChange = { visible = it },
                errorText = "That's not the current password. Try again.",
            )
        }
    }
}

@Preview(name = "Dark")
@Composable
private fun TextFieldDarkPreview() {
    DotlinkTheme(dark = true) { TextFieldPreviewContent() }
}

@Preview(name = "Light")
@Composable
private fun TextFieldLightPreview() {
    DotlinkTheme(dark = false) { TextFieldPreviewContent() }
}
