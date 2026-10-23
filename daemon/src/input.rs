//! Virtual mouse and keyboard through /dev/uinput. Hyprland picks them up like real devices,
//! so the user's own keyboard layout applies to key codes.

use anyhow::{Context, Result};
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode};
use serde::Deserialize;

/// One wheel notch in hi-res units (kernel convention).
const NOTCH: i32 = 120;

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum InputMsg {
    /// Pointer motion in pixels.
    Move { x: f32, y: f32 },
    /// Scroll in hi-res wheel units (120 = one notch); positive y scrolls down.
    Scroll { x: f32, y: f32 },
    /// `b`: left|right|middle, `s`: down|up|click.
    Btn { b: String, s: String },
    /// Tap one named key.
    Key { k: String },
    /// Hold every key in order, then release in reverse: ["CTRL", "SHIFT", "T"].
    Combo { k: Vec<String> },
    /// Type text (US layout mapping; other characters are skipped).
    Text { s: String },
}

pub struct Input {
    mouse: VirtualDevice,
    keyboard: VirtualDevice,
    rem: (f32, f32),
    scroll_rem: (f32, f32),
    notch_acc: (i32, i32),
}

impl Input {
    pub fn new() -> Result<Self> {
        let mut buttons = AttributeSet::<KeyCode>::new();
        for b in [KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE] {
            buttons.insert(b);
        }
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        for a in [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        ] {
            axes.insert(a);
        }
        let mouse = VirtualDevice::builder()
            .context("open /dev/uinput (see README: uinput access)")?
            .name("Dotlink Touchpad")
            .with_keys(&buttons)?
            .with_relative_axes(&axes)?
            .build()?;

        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 1..=248u16 {
            keys.insert(KeyCode(code));
        }
        let keyboard = VirtualDevice::builder()?.name("Dotlink Keyboard").with_keys(&keys)?.build()?;
        Ok(Self { mouse, keyboard, rem: (0.0, 0.0), scroll_rem: (0.0, 0.0), notch_acc: (0, 0) })
    }

    pub fn handle(&mut self, msg: InputMsg) -> Result<()> {
        match msg {
            InputMsg::Move { x, y } => {
                let fx = x + self.rem.0;
                let fy = y + self.rem.1;
                let (dx, dy) = (fx.trunc() as i32, fy.trunc() as i32);
                self.rem = (fx - dx as f32, fy - dy as f32);
                if dx != 0 || dy != 0 {
                    self.mouse.emit(&[rel(RelativeAxisCode::REL_X, dx), rel(RelativeAxisCode::REL_Y, dy)])?;
                }
            }
            InputMsg::Scroll { x, y } => {
                // Wheel "up" is positive in evdev, finger-up on the phone is negative y.
                let fx = x + self.scroll_rem.0;
                let fy = -y + self.scroll_rem.1;
                let (hx, hy) = (fx.trunc() as i32, fy.trunc() as i32);
                self.scroll_rem = (fx - hx as f32, fy - hy as f32);
                let mut events = Vec::new();
                if hy != 0 {
                    events.push(rel(RelativeAxisCode::REL_WHEEL_HI_RES, hy));
                    self.notch_acc.1 += hy;
                    let notches = self.notch_acc.1 / NOTCH;
                    if notches != 0 {
                        self.notch_acc.1 -= notches * NOTCH;
                        events.push(rel(RelativeAxisCode::REL_WHEEL, notches));
                    }
                }
                if hx != 0 {
                    events.push(rel(RelativeAxisCode::REL_HWHEEL_HI_RES, hx));
                    self.notch_acc.0 += hx;
                    let notches = self.notch_acc.0 / NOTCH;
                    if notches != 0 {
                        self.notch_acc.0 -= notches * NOTCH;
                        events.push(rel(RelativeAxisCode::REL_HWHEEL, notches));
                    }
                }
                if !events.is_empty() {
                    self.mouse.emit(&events)?;
                }
            }
            InputMsg::Btn { b, s } => {
                let code = match b.as_str() {
                    "right" => KeyCode::BTN_RIGHT,
                    "middle" => KeyCode::BTN_MIDDLE,
                    _ => KeyCode::BTN_LEFT,
                };
                match s.as_str() {
                    "down" => self.mouse.emit(&[key(code, 1)])?,
                    "up" => self.mouse.emit(&[key(code, 0)])?,
                    _ => {
                        self.mouse.emit(&[key(code, 1)])?;
                        self.mouse.emit(&[key(code, 0)])?;
                    }
                }
            }
            InputMsg::Key { k } => {
                if let Some(code) = key_by_name(&k) {
                    self.tap(&[code])?;
                }
            }
            InputMsg::Combo { k } => {
                let codes: Vec<KeyCode> = k.iter().filter_map(|n| key_by_name(n)).collect();
                if !codes.is_empty() {
                    self.tap(&codes)?;
                }
            }
            InputMsg::Text { s } => {
                for c in s.chars() {
                    if let Some((code, shift)) = key_for_char(c) {
                        if shift {
                            self.tap(&[KeyCode::KEY_LEFTSHIFT, code])?;
                        } else {
                            self.tap(&[code])?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn tap(&mut self, codes: &[KeyCode]) -> Result<()> {
        for &c in codes {
            self.keyboard.emit(&[key(c, 1)])?;
        }
        for &c in codes.iter().rev() {
            self.keyboard.emit(&[key(c, 0)])?;
        }
        Ok(())
    }
}

fn rel(code: RelativeAxisCode, v: i32) -> InputEvent {
    InputEvent::new(EventType::RELATIVE.0, code.0, v)
}

fn key(code: KeyCode, v: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code.0, v)
}

fn key_by_name(name: &str) -> Option<KeyCode> {
    let n = name.to_ascii_uppercase();
    Some(match n.as_str() {
        "ENTER" => KeyCode::KEY_ENTER,
        "BACKSPACE" => KeyCode::KEY_BACKSPACE,
        "TAB" => KeyCode::KEY_TAB,
        "ESC" => KeyCode::KEY_ESC,
        "SPACE" => KeyCode::KEY_SPACE,
        "UP" => KeyCode::KEY_UP,
        "DOWN" => KeyCode::KEY_DOWN,
        "LEFT" => KeyCode::KEY_LEFT,
        "RIGHT" => KeyCode::KEY_RIGHT,
        "HOME" => KeyCode::KEY_HOME,
        "END" => KeyCode::KEY_END,
        "PAGEUP" => KeyCode::KEY_PAGEUP,
        "PAGEDOWN" => KeyCode::KEY_PAGEDOWN,
        "DELETE" => KeyCode::KEY_DELETE,
        "INSERT" => KeyCode::KEY_INSERT,
        "CTRL" => KeyCode::KEY_LEFTCTRL,
        "SHIFT" => KeyCode::KEY_LEFTSHIFT,
        "ALT" => KeyCode::KEY_LEFTALT,
        "SUPER" => KeyCode::KEY_LEFTMETA,
        "PRINT" => KeyCode::KEY_SYSRQ,
        "VOLUMEUP" => KeyCode::KEY_VOLUMEUP,
        "VOLUMEDOWN" => KeyCode::KEY_VOLUMEDOWN,
        "MUTE" => KeyCode::KEY_MUTE,
        "PLAYPAUSE" => KeyCode::KEY_PLAYPAUSE,
        "NEXTSONG" => KeyCode::KEY_NEXTSONG,
        "PREVIOUSSONG" => KeyCode::KEY_PREVIOUSSONG,
        f if f.starts_with('F') && f.len() > 1 => {
            let i: u16 = f[1..].parse().ok()?;
            match i {
                1..=10 => KeyCode(KeyCode::KEY_F1.0 + i - 1),
                11 => KeyCode::KEY_F11,
                12 => KeyCode::KEY_F12,
                _ => return None,
            }
        }
        s if s.chars().count() == 1 => return key_for_char(s.chars().next()?.to_ascii_lowercase()).map(|k| k.0),
        _ => return None,
    })
}

/// Key code and whether Shift is needed for `c` on a US layout.
fn key_for_char(c: char) -> Option<(KeyCode, bool)> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KEY_A, KeyCode::KEY_B, KeyCode::KEY_C, KeyCode::KEY_D, KeyCode::KEY_E, KeyCode::KEY_F,
        KeyCode::KEY_G, KeyCode::KEY_H, KeyCode::KEY_I, KeyCode::KEY_J, KeyCode::KEY_K, KeyCode::KEY_L,
        KeyCode::KEY_M, KeyCode::KEY_N, KeyCode::KEY_O, KeyCode::KEY_P, KeyCode::KEY_Q, KeyCode::KEY_R,
        KeyCode::KEY_S, KeyCode::KEY_T, KeyCode::KEY_U, KeyCode::KEY_V, KeyCode::KEY_W, KeyCode::KEY_X,
        KeyCode::KEY_Y, KeyCode::KEY_Z,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::KEY_0, KeyCode::KEY_1, KeyCode::KEY_2, KeyCode::KEY_3, KeyCode::KEY_4,
        KeyCode::KEY_5, KeyCode::KEY_6, KeyCode::KEY_7, KeyCode::KEY_8, KeyCode::KEY_9,
    ];
    if c.is_ascii_lowercase() {
        return Some((LETTERS[(c as u8 - b'a') as usize], false));
    }
    if c.is_ascii_uppercase() {
        return Some((LETTERS[(c as u8 - b'A') as usize], true));
    }
    if c.is_ascii_digit() {
        return Some((DIGITS[(c as u8 - b'0') as usize], false));
    }
    use KeyCode as K;
    Some(match c {
        ' ' => (K::KEY_SPACE, false),
        '\n' => (K::KEY_ENTER, false),
        '\t' => (K::KEY_TAB, false),
        '-' => (K::KEY_MINUS, false),
        '_' => (K::KEY_MINUS, true),
        '=' => (K::KEY_EQUAL, false),
        '+' => (K::KEY_EQUAL, true),
        '[' => (K::KEY_LEFTBRACE, false),
        '{' => (K::KEY_LEFTBRACE, true),
        ']' => (K::KEY_RIGHTBRACE, false),
        '}' => (K::KEY_RIGHTBRACE, true),
        '\\' => (K::KEY_BACKSLASH, false),
        '|' => (K::KEY_BACKSLASH, true),
        ';' => (K::KEY_SEMICOLON, false),
        ':' => (K::KEY_SEMICOLON, true),
        '\'' => (K::KEY_APOSTROPHE, false),
        '"' => (K::KEY_APOSTROPHE, true),
        '`' => (K::KEY_GRAVE, false),
        '~' => (K::KEY_GRAVE, true),
        ',' => (K::KEY_COMMA, false),
        '<' => (K::KEY_COMMA, true),
        '.' => (K::KEY_DOT, false),
        '>' => (K::KEY_DOT, true),
        '/' => (K::KEY_SLASH, false),
        '?' => (K::KEY_SLASH, true),
        '!' => (K::KEY_1, true),
        '@' => (K::KEY_2, true),
        '#' => (K::KEY_3, true),
        '$' => (K::KEY_4, true),
        '%' => (K::KEY_5, true),
        '^' => (K::KEY_6, true),
        '&' => (K::KEY_7, true),
        '*' => (K::KEY_8, true),
        '(' => (K::KEY_9, true),
        ')' => (K::KEY_0, true),
        _ => return None,
    })
}
