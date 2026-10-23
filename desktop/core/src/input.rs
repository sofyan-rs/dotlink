//! Remote screen messages from the phone. Each platform's `Input` turns them into real
//! pointer and key events.

use serde::Deserialize;

/// One wheel notch in hi-res units (Linux kernel convention, also what the phone sends).
pub const NOTCH: i32 = 120;

#[derive(Debug, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum InputMsg {
    /// Pointer motion in pixels.
    Move { x: f32, y: f32 },
    /// Scroll in hi-res wheel units (120 = one notch); positive y scrolls down.
    Scroll { x: f32, y: f32 },
    /// `b`: left|right|middle, `s`: down|up|click.
    Btn { b: String, s: String },
    /// Tap one named key ("ENTER", "F5", "a"…).
    Key { k: String },
    /// Hold every key in order, then release in reverse: ["CTRL", "SHIFT", "T"].
    Combo { k: Vec<String> },
    /// Type text (US layout mapping; other characters are skipped).
    Text { s: String },
}
