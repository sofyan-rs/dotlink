//! Mouse and keyboard through SendInput. Key names are the same as on Linux (`key_by_name`).
//! `Text` types each character as Unicode, so any character works in any keyboard layout.
//!
//! SendInput can't reach elevated windows (UAC prompts, Task Manager) from a normal process;
//! Dotlink doesn't run elevated, so those stay out of reach.

use crate::input::InputMsg;
use anyhow::{Result, bail};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

pub struct Input {
    rem: (f32, f32),
    scroll_rem: (f32, f32),
}

impl Input {
    pub fn new() -> Result<Self> {
        Ok(Self { rem: (0.0, 0.0), scroll_rem: (0.0, 0.0) })
    }

    pub fn handle(&mut self, msg: InputMsg) -> Result<()> {
        match msg {
            InputMsg::Move { x, y } => {
                // Relative, like the Linux virtual touchpad: Windows' pointer speed applies.
                let fx = x + self.rem.0;
                let fy = y + self.rem.1;
                let (dx, dy) = (fx.trunc() as i32, fy.trunc() as i32);
                self.rem = (fx - dx as f32, fy - dy as f32);
                if dx != 0 || dy != 0 {
                    send(&[mouse(dx, dy, 0, MOUSEEVENTF_MOVE)])?;
                }
            }
            InputMsg::Scroll { x, y } => {
                // Same hi-res units as WHEEL_DELTA (120 = one notch); Windows' wheel is
                // positive up, the phone's y positive down.
                let fx = x + self.scroll_rem.0;
                let fy = -y + self.scroll_rem.1;
                let (hx, hy) = (fx.trunc() as i32, fy.trunc() as i32);
                self.scroll_rem = (fx - hx as f32, fy - hy as f32);
                let mut events = Vec::new();
                if hy != 0 {
                    events.push(mouse(0, 0, hy, MOUSEEVENTF_WHEEL));
                }
                if hx != 0 {
                    events.push(mouse(0, 0, hx, MOUSEEVENTF_HWHEEL));
                }
                if !events.is_empty() {
                    send(&events)?;
                }
            }
            InputMsg::Btn { b, s } => {
                let (down, up) = match b.as_str() {
                    "right" => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
                    "middle" => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
                    _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
                };
                match s.as_str() {
                    "down" => send(&[mouse(0, 0, 0, down)])?,
                    "up" => send(&[mouse(0, 0, 0, up)])?,
                    _ => send(&[mouse(0, 0, 0, down), mouse(0, 0, 0, up)])?,
                }
            }
            InputMsg::Key { k } => {
                if let Some(vk) = key_by_name(&k) {
                    tap(&[vk])?;
                }
            }
            InputMsg::Combo { k } => {
                let keys: Vec<VIRTUAL_KEY> = k.iter().filter_map(|n| key_by_name(n)).collect();
                if !keys.is_empty() {
                    tap(&keys)?;
                }
            }
            InputMsg::Text { s } => {
                let mut events = Vec::new();
                for c in s.chars() {
                    match c {
                        // Apps expect the Enter and Tab keys, not the characters.
                        '\n' => events.extend(key_events(&[VK_RETURN])),
                        '\t' => events.extend(key_events(&[VK_TAB])),
                        '\r' => {}
                        c => {
                            let mut units = [0u16; 2];
                            for &unit in c.encode_utf16(&mut units).iter() {
                                events.push(unicode(unit, 0));
                                events.push(unicode(unit, KEYEVENTF_KEYUP));
                            }
                        }
                    }
                }
                if !events.is_empty() {
                    send(&events)?;
                }
            }
        }
        Ok(())
    }
}

fn send(inputs: &[INPUT]) -> Result<()> {
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // Blocked by UIPI: the foreground window runs as administrator.
        bail!("Windows blocked the input (is an administrator window in front?)");
    }
    Ok(())
}

fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    // The scan code too: games and remote desktop clients read that instead of the virtual key.
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn unicode(unit: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: 0, wScan: unit, dwFlags: KEYEVENTF_UNICODE | flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Every key down in order, then up in reverse.
fn key_events(keys: &[VIRTUAL_KEY]) -> Vec<INPUT> {
    keys.iter().map(|&vk| key(vk, false)).chain(keys.iter().rev().map(|&vk| key(vk, true))).collect()
}

fn tap(keys: &[VIRTUAL_KEY]) -> Result<()> {
    send(&key_events(keys))
}

/// Keys of the extended block (arrows, Insert/Delete/Home/End/Page Up/Down, Windows key):
/// without the flag they come out as the numpad's.
fn is_extended(vk: VIRTUAL_KEY) -> bool {
    matches!(
        vk,
        VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT | VK_HOME | VK_END | VK_PRIOR | VK_NEXT | VK_INSERT | VK_DELETE
            | VK_LWIN | VK_RWIN | VK_RCONTROL | VK_RMENU | VK_SNAPSHOT | VK_DIVIDE | VK_NUMLOCK
    )
}

fn key_by_name(name: &str) -> Option<VIRTUAL_KEY> {
    let n = name.to_ascii_uppercase();
    Some(match n.as_str() {
        "ENTER" => VK_RETURN,
        "BACKSPACE" => VK_BACK,
        "TAB" => VK_TAB,
        "ESC" => VK_ESCAPE,
        "SPACE" => VK_SPACE,
        "UP" => VK_UP,
        "DOWN" => VK_DOWN,
        "LEFT" => VK_LEFT,
        "RIGHT" => VK_RIGHT,
        "HOME" => VK_HOME,
        "END" => VK_END,
        "PAGEUP" => VK_PRIOR,
        "PAGEDOWN" => VK_NEXT,
        "DELETE" => VK_DELETE,
        "INSERT" => VK_INSERT,
        "CTRL" => VK_LCONTROL,
        "SHIFT" => VK_LSHIFT,
        "ALT" => VK_LMENU,
        "SUPER" => VK_LWIN,
        "PRINT" => VK_SNAPSHOT,
        "VOLUMEUP" => VK_VOLUME_UP,
        "VOLUMEDOWN" => VK_VOLUME_DOWN,
        "MUTE" => VK_VOLUME_MUTE,
        "PLAYPAUSE" => VK_MEDIA_PLAY_PAUSE,
        "NEXTSONG" => VK_MEDIA_NEXT_TRACK,
        "PREVIOUSSONG" => VK_MEDIA_PREV_TRACK,
        f if f.starts_with('F') && f.len() > 1 => {
            let i: u16 = f[1..].parse().ok()?;
            match i {
                1..=24 => VK_F1 + i - 1,
                _ => return None,
            }
        }
        // A character: the key that types it in the current layout ("T" in Ctrl+Shift+T).
        s if s.chars().count() == 1 => {
            let c = name.chars().next()?.to_ascii_lowercase();
            let mut units = [0u16; 2];
            let units = c.encode_utf16(&mut units);
            if units.len() != 1 {
                return None;
            }
            let scan = unsafe { VkKeyScanW(units[0]) };
            if scan == -1 {
                return None;
            }
            (scan as u16) & 0xff
        }
        _ => return None,
    })
}
