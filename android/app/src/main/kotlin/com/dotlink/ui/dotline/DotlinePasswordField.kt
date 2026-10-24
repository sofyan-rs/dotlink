/**
 * Password field for the Dotline design: like [DotlineTextField], with the text hidden and an
 * eye button to show it. Screens: Set-Password (5.1), Unlock (5.2), Change-Password (5.3).
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

/**
 * A password input: dots instead of letters, optional "show password" eye button.
 *
 * @param state the password; create it with `rememberTextFieldState()`.
 * @param label the overline above the box ("Password").
 * @param passwordVisible true to show the password as plain text.
 * @param onPasswordVisibleChange called when the eye button is tapped. Pass `null` to hide the
 *   button (e.g. a confirm field that shows [trailing] MATCH instead).
 * @param trailing slot shown when there is no eye button, usually [DotlineTextFieldStatus].
 * Other parameters (including `inSheet`) as in [DotlineTextField].
 */
@Composable
fun DotlinePasswordField(
    state: TextFieldState,
    label: String?,
    passwordVisible: Boolean,
    onPasswordVisibleChange: ((Boolean) -> Unit)?,
    modifier: Modifier = Modifier,
    trailing: (@Composable () -> Unit)? = null,
    /** Grey hint shown while the field is empty, like [DotlineTextField]'s. */
    placeholder: String? = null,
    helperText: String? = null,
    errorText: String? = null,
    enabled: Boolean = true,
    inSheet: Boolean = false,
    keyboardOptions: KeyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
    onKeyboardAction: KeyboardActionHandler? = null,
    /** Applied to the editable text itself, e.g. `Modifier.focusRequester(...)` to focus it. */
    textFieldModifier: Modifier = Modifier,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val focused by interactionSource.collectIsFocusedAsState()
    // Passwords use Geist Mono at the input size (17 sp) with the `label` tracking (+0.12em).
    val textStyle = Dotline.type.data.copy(fontSize = 17.sp, lineHeight = 24.sp, letterSpacing = 0.12.em)

    // When there is an eye button, it replaces the `trailing` slot.
    val trailingContent: (@Composable () -> Unit)? =
        if (onPasswordVisibleChange != null) {
            {
                DotlineTextFieldIconButton(
                    icon = if (passwordVisible) DotlineIcons.EyeOff else DotlineIcons.Eye,
                    contentDescription = if (passwordVisible) "Hide password" else "Show password",
                    onClick = { onPasswordVisibleChange(!passwordVisible) },
                    // Brighter eye while the password is shown, so the state is visible.
                    tint = if (passwordVisible) Dotline.colors.ink else Dotline.colors.inkMuted,
                )
            }
        } else {
            trailing
        }

    FieldColumn(label = label, helperText = helperText, errorText = errorText, modifier = modifier) {
        WithDotlineSelectionColors {
            BasicSecureTextField(
                state = state,
                modifier = Modifier.fillMaxWidth().then(textFieldModifier).fieldSemantics(label, errorText),
                enabled = enabled,
                textStyle = textStyle.copy(color = if (enabled) Dotline.colors.ink else Dotline.colors.inkMuted),
                keyboardOptions = keyboardOptions,
                onKeyboardAction = onKeyboardAction,
                interactionSource = interactionSource,
                cursorBrush = SolidColor(Dotline.colors.accent),
                // RevealLastTyped = dots, with the last typed character shown briefly (Android default);
                // Visible = plain text.
                textObfuscationMode = if (passwordVisible) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped,
                decorator = { innerTextField ->
                    FieldBox(
                        innerTextField = innerTextField,
                        showPlaceholder = placeholder != null && state.text.isEmpty(),
                        placeholder = placeholder,
                        placeholderStyle = textStyle,
                        leadingIcon = null,
                        suffix = null,
                        trailing = trailingContent,
                        focused = focused,
                        isError = errorText != null,
                        inSheet = inSheet,
                    )
                },
            )
        }
    }
}
