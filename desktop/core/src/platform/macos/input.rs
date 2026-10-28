//! Mouse and keyboard through CoreGraphics events (CGEventPost). Key names are the same as on
//! Linux (`key_by_name`); ⌘ is `SUPER`, ⌥ is `ALT`. `Text` types each character as Unicode, so
//! any character works in any keyboard layout. Named letter keys ("T" in ⌘⇧T) are the US
//! layout's key positions.
//!
//! Posting events needs the Accessibility permission (System Settings → Privacy & Security →
//! Accessibility). From `cargo run` it's the terminal app that needs it; TCC remembers it per
//! code signature, so an unsigned build loses it on every rebuild.

use crate::input::{InputMsg, NOTCH};
use anyhow::{Result, bail};
use std::ffi::c_void;
use std::time::{Duration, Instant};

type CGEventRef = *mut c_void;
type CFTypeRef = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGPoint,
}

impl CGRect {
    fn contains(&self, p: CGPoint) -> bool {
        p.x >= self.origin.x
            && p.y >= self.origin.y
            && p.x < self.origin.x + self.size.x
            && p.y < self.origin.y + self.size.y
    }

    fn clamp(&self, p: CGPoint) -> CGPoint {
        CGPoint {
            x: p.x.clamp(self.origin.x, self.origin.x + self.size.x - 1.),
            y: p.y.clamp(self.origin.y, self.origin.y + self.size.y - 1.),
        }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, at: CGPoint, button: u32) -> CGEventRef;
    fn CGEventCreateScrollWheelEvent2(
        source: *const c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
        wheel2: i32,
        wheel3: i32,
    ) -> CGEventRef;
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> CGEventRef;
    fn CGEventKeyboardSetUnicodeString(event: CGEventRef, len: usize, chars: *const u16);
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGGetActiveDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: CFTypeRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CFTypeRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    static kAXTrustedCheckOptionPrompt: CFTypeRef;
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> bool;
}

// CGEventType
const LEFT_DOWN: u32 = 1;
const LEFT_UP: u32 = 2;
const RIGHT_DOWN: u32 = 3;
const RIGHT_UP: u32 = 4;
const MOVED: u32 = 5;
const LEFT_DRAGGED: u32 = 6;
const RIGHT_DRAGGED: u32 = 7;
const OTHER_DOWN: u32 = 25;
const OTHER_UP: u32 = 26;
const OTHER_DRAGGED: u32 = 27;
// CGEventField
const CLICK_STATE: u32 = 1;
const DELTA_X: u32 = 4;
const DELTA_Y: u32 = 5;
const HID_TAP: u32 = 0;
const SCROLL_PIXELS: u32 = 0;
// CGEventFlags
const SHIFT: u64 = 1 << 17;
const CONTROL: u64 = 1 << 18;
const OPTION: u64 = 1 << 19;
const COMMAND: u64 = 1 << 20;

/// Pixels of smooth scrolling per wheel notch: about three lines, as a mouse wheel scrolls.
const NOTCH_PIXELS: f32 = 40.;
/// How long between two clicks still counts as a double click (macOS' default is 0.5 s).
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// Whether macOS lets this process post input events.
pub fn trusted() -> bool {
    // SAFETY: no arguments, no side effects.
    unsafe { AXIsProcessTrusted() }
}

/// Asks macOS to show its "would like to control this computer" prompt, which adds Dotlink
/// to the Accessibility list (switched off) and links to it. Returns whether it's allowed.
pub fn ask_trust() -> bool {
    // SAFETY: a one-entry CF dictionary of CF constants, released after use.
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        if options.is_null() {
            return AXIsProcessTrusted();
        }
        let trusted = AXIsProcessTrustedWithOptions(options);
        CFRelease(options);
        trusted
    }
}

/// An event we created; released when dropped.
struct Event(CGEventRef);

impl Event {
    fn new(event: CGEventRef) -> Result<Self> {
        if event.is_null() {
            bail!("macOS refused to create the input event");
        }
        Ok(Self(event))
    }

    fn post(self) {
        // SAFETY: a valid event; CGEventPost doesn't take ownership.
        unsafe { CGEventPost(HID_TAP, self.0) }
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: created by a CGEventCreate* call, released once.
        unsafe { CFRelease(self.0 as CFTypeRef) }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Button {
    Left,
    Right,
    Middle,
}

impl Button {
    fn events(self) -> (u32, u32, u32) {
        match self {
            Button::Left => (LEFT_DOWN, LEFT_UP, LEFT_DRAGGED),
            Button::Right => (RIGHT_DOWN, RIGHT_UP, RIGHT_DRAGGED),
            Button::Middle => (OTHER_DOWN, OTHER_UP, OTHER_DRAGGED),
        }
    }

    fn number(self) -> u32 {
        match self {
            Button::Left => 0,
            Button::Right => 1,
            Button::Middle => 2,
        }
    }
}

pub struct Input {
    /// Pointer position with the fractions the phone's moves leave.
    pos: Option<CGPoint>,
    scroll_rem: (f32, f32),
    /// The button held down, for drag events instead of plain moves.
    held: Option<Button>,
    /// The last click's button, time and count, for double and triple clicks.
    last_click: Option<(Button, Instant, i64)>,
}

impl Input {
    pub fn new() -> Result<Self> {
        if !trusted() {
            bail!(
                "allow Dotlink in System Settings → Privacy & Security → Accessibility (from a terminal: allow the terminal app)"
            );
        }
        Ok(Self { pos: None, scroll_rem: (0., 0.), held: None, last_click: None })
    }

    pub fn handle(&mut self, msg: InputMsg) -> Result<()> {
        match msg {
            InputMsg::Move { x, y } => self.move_by(x as f64, y as f64)?,
            InputMsg::Scroll { x, y } => {
                // Positive y is down on the phone; positive wheel1 scrolls up here.
                let fx = -x / NOTCH as f32 * NOTCH_PIXELS + self.scroll_rem.0;
                let fy = -y / NOTCH as f32 * NOTCH_PIXELS + self.scroll_rem.1;
                let (px, py) = (fx.trunc() as i32, fy.trunc() as i32);
                self.scroll_rem = (fx - px as f32, fy - py as f32);
                if px != 0 || py != 0 {
                    // SAFETY: plain values; the result is checked.
                    Event::new(unsafe {
                        CGEventCreateScrollWheelEvent2(std::ptr::null(), SCROLL_PIXELS, 2, py, px, 0)
                    })?
                    .post();
                }
            }
            InputMsg::Btn { b, s } => {
                let button = match b.as_str() {
                    "right" => Button::Right,
                    "middle" => Button::Middle,
                    _ => Button::Left,
                };
                match s.as_str() {
                    "down" => self.press(button, true)?,
                    "up" => self.press(button, false)?,
                    _ => {
                        self.press(button, true)?;
                        self.press(button, false)?;
                    }
                }
            }
            InputMsg::Key { k } => match k.to_ascii_uppercase().as_str() {
                // No Print Screen key: ⌘⇧3 takes the screenshot.
                "PRINT" => tap(&[KVK_COMMAND, KVK_SHIFT, 0x14])?,
                name => match media_key(name) {
                    Some(nx) => media(nx)?,
                    None => {
                        if let Some(code) = key_by_name(&k) {
                            tap(&[code])?;
                        }
                    }
                },
            },
            InputMsg::Combo { k } => {
                let keys: Vec<u16> = k.iter().filter_map(|n| key_by_name(n)).collect();
                if !keys.is_empty() {
                    tap(&keys)?;
                }
            }
            InputMsg::Text { s } => {
                let mut chunk: Vec<u16> = Vec::new();
                for c in s.chars() {
                    match c {
                        // Apps expect the Return and Tab keys, not the characters.
                        '\n' | '\t' => {
                            type_units(&chunk)?;
                            chunk.clear();
                            tap(&[if c == '\n' { KVK_RETURN } else { KVK_TAB }])?;
                        }
                        '\r' => {}
                        c => {
                            let mut units = [0u16; 2];
                            chunk.extend_from_slice(c.encode_utf16(&mut units));
                        }
                    }
                }
                type_units(&chunk)?;
            }
        }
        Ok(())
    }

    fn move_by(&mut self, dx: f64, dy: f64) -> Result<()> {
        let current = match self.pos {
            Some(p) => p,
            None => cursor()?,
        };
        let target = clamp_to_displays(CGPoint { x: current.x + dx, y: current.y + dy }, current);
        self.pos = Some(target);
        let (kind, button) = match self.held {
            Some(b) => (b.events().2, b.number()),
            None => (MOVED, 0),
        };
        // SAFETY: plain values; the result is checked.
        let event = Event::new(unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, target, button) })?;
        // Games and 3D apps read the deltas, not the position.
        // SAFETY: a valid event.
        unsafe {
            CGEventSetIntegerValueField(event.0, DELTA_X, dx.round() as i64);
            CGEventSetIntegerValueField(event.0, DELTA_Y, dy.round() as i64);
        }
        event.post();
        Ok(())
    }

    fn press(&mut self, button: Button, down: bool) -> Result<()> {
        // The real cursor: the user may have moved the Mac's own mouse since.
        let at = cursor()?;
        self.pos = Some(at);
        if down {
            let now = Instant::now();
            let count = match self.last_click {
                Some((b, when, n)) if b == button && now - when < DOUBLE_CLICK => n + 1,
                _ => 1,
            };
            self.last_click = Some((button, now, count));
            self.held = Some(button);
        } else if self.held == Some(button) {
            self.held = None;
        }
        let count = self.last_click.map_or(1, |(_, _, n)| n);
        let (down_kind, up_kind, _) = button.events();
        // SAFETY: plain values; the result is checked.
        let event = Event::new(unsafe {
            CGEventCreateMouseEvent(std::ptr::null(), if down { down_kind } else { up_kind }, at, button.number())
        })?;
        // Finder opens things on a click count of 2, not on two clicks.
        // SAFETY: a valid event.
        unsafe { CGEventSetIntegerValueField(event.0, CLICK_STATE, count) };
        event.post();
        Ok(())
    }
}

/// Where the pointer is now, in global display coordinates (top left origin).
fn cursor() -> Result<CGPoint> {
    // SAFETY: an empty event only to read the location; released by Event.
    let event = Event::new(unsafe { CGEventCreate(std::ptr::null()) })?;
    // SAFETY: a valid event.
    Ok(unsafe { CGEventGetLocation(event.0) })
}

/// Keeps the pointer on a screen: inside any display it may go, otherwise it stays on the
/// display it was on (screens side by side at different heights leave gaps).
fn clamp_to_displays(target: CGPoint, from: CGPoint) -> CGPoint {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: the buffer holds `ids.len()` ids.
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0 || count == 0 {
        return target;
    }
    // SAFETY: ids from CGGetActiveDisplayList.
    let bounds: Vec<CGRect> = ids[..count as usize].iter().map(|&id| unsafe { CGDisplayBounds(id) }).collect();
    if bounds.iter().any(|b| b.contains(target)) {
        return target;
    }
    bounds.iter().find(|b| b.contains(from)).unwrap_or(&bounds[0]).clamp(target)
}

fn modifier(code: u16) -> u64 {
    match code {
        KVK_COMMAND => COMMAND,
        KVK_SHIFT => SHIFT,
        KVK_OPTION => OPTION,
        KVK_CONTROL => CONTROL,
        _ => 0,
    }
}

/// Every key down in order, then up in reverse, each event carrying the modifiers held at
/// that moment (macOS reads ⌘ from the flags, not from the ⌘ key's own event).
fn tap(keys: &[u16]) -> Result<()> {
    let mut flags = 0;
    for &code in keys {
        flags |= modifier(code);
        key_event(code, true, flags)?;
    }
    for &code in keys.iter().rev() {
        flags &= !modifier(code);
        key_event(code, false, flags)?;
    }
    Ok(())
}

fn key_event(code: u16, down: bool, flags: u64) -> Result<()> {
    // SAFETY: plain values; the result is checked.
    let event = Event::new(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), code, down) })?;
    // SAFETY: a valid event.
    unsafe { CGEventSetFlags(event.0, flags) };
    event.post();
    Ok(())
}

/// Types UTF-16 text. A keyboard event carries at most 20 units of it.
fn type_units(units: &[u16]) -> Result<()> {
    for chunk in units.chunks(20) {
        for down in [true, false] {
            // SAFETY: plain values; the result is checked.
            let event = Event::new(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), 0, down) })?;
            // SAFETY: a valid event, `chunk.len()` units at the pointer. No flags: a modifier
            // still held on the Mac's own keyboard mustn't turn the text into shortcuts.
            unsafe {
                CGEventSetFlags(event.0, 0);
                CGEventKeyboardSetUnicodeString(event.0, chunk.len(), chunk.as_ptr());
            }
            event.post();
        }
    }
    Ok(())
}

// ---------- media keys ----------

/// NX_KEYTYPE_* of the media keys: they aren't key codes but "system defined" events.
fn media_key(name: &str) -> Option<i64> {
    Some(match name {
        "VOLUMEUP" => 0,
        "VOLUMEDOWN" => 1,
        "MUTE" => 7,
        "PLAYPAUSE" => 16,
        "NEXTSONG" => 17,
        "PREVIOUSSONG" => 18,
        _ => return None,
    })
}

type Id = *mut c_void;
type Sel = *const c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const std::ffi::c_char) -> Id;
    fn sel_registerName(name: *const std::ffi::c_char) -> Sel;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}

// NSEvent lives in AppKit; the headless build doesn't link it otherwise.
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

/// Presses and releases a media key: an NSEvent of type NSEventTypeSystemDefined (14),
/// subtype 8, posted through its CGEvent.
fn media(key: i64) -> Result<()> {
    type OtherEvent = unsafe extern "C" fn(Id, Sel, u64, CGPoint, u64, f64, i64, Id, i16, i64, i64) -> Id;
    type CgEvent = unsafe extern "C" fn(Id, Sel) -> CGEventRef;
    // SAFETY: objc_msgSend called through the exact signature of each method; the NSEvents are
    // autoreleased into the pool popped below, and CGEventPost doesn't take the CGEvent.
    unsafe {
        let class = objc_getClass(c"NSEvent".as_ptr());
        if class.is_null() {
            bail!("media keys need AppKit");
        }
        let other = sel_registerName(
            c"otherEventWithType:location:modifierFlags:timestamp:windowNumber:context:subtype:data1:data2:".as_ptr(),
        );
        let cg = sel_registerName(c"CGEvent".as_ptr());
        let send_other: OtherEvent = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let send_cg: CgEvent = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let pool = objc_autoreleasePoolPush();
        for state in [0xa_i64, 0xb] {
            let flags = (state as u64) << 8;
            let data1 = (key << 16) | (state << 8);
            let event = send_other(
                class,
                other,
                14,
                CGPoint { x: 0., y: 0. },
                flags,
                0.,
                0,
                std::ptr::null_mut(),
                8,
                data1,
                -1,
            );
            if !event.is_null() {
                let cg_event = send_cg(event, cg);
                if !cg_event.is_null() {
                    CGEventPost(HID_TAP, cg_event);
                }
            }
        }
        objc_autoreleasePoolPop(pool);
    }
    Ok(())
}

// ---------- key codes (Carbon's kVK_*) ----------

const KVK_RETURN: u16 = 0x24;
const KVK_TAB: u16 = 0x30;
const KVK_COMMAND: u16 = 0x37;
const KVK_SHIFT: u16 = 0x38;
const KVK_OPTION: u16 = 0x3A;
const KVK_CONTROL: u16 = 0x3B;

fn key_by_name(name: &str) -> Option<u16> {
    let n = name.to_ascii_uppercase();
    Some(match n.as_str() {
        "ENTER" => KVK_RETURN,
        "BACKSPACE" => 0x33,
        "TAB" => KVK_TAB,
        "ESC" => 0x35,
        "SPACE" => 0x31,
        "UP" => 0x7E,
        "DOWN" => 0x7D,
        "LEFT" => 0x7B,
        "RIGHT" => 0x7C,
        "HOME" => 0x73,
        "END" => 0x77,
        "PAGEUP" => 0x74,
        "PAGEDOWN" => 0x79,
        "DELETE" => 0x75,
        // The Help key sits where Insert is on a PC keyboard.
        "INSERT" => 0x72,
        "CTRL" => KVK_CONTROL,
        "SHIFT" => KVK_SHIFT,
        "ALT" => KVK_OPTION,
        "SUPER" => KVK_COMMAND,
        f if f.starts_with('F') && f.len() > 1 => {
            const F: [u16; 20] = [
                0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A, 0x40,
                0x4F, 0x50, 0x5A,
            ];
            let i: usize = f[1..].parse().ok()?;
            *F.get(i.checked_sub(1)?)?
        }
        s if s.chars().count() == 1 => char_key(s.chars().next()?.to_ascii_lowercase())?,
        _ => return None,
    })
}

/// The ANSI (US) key that types `c`.
fn char_key(c: char) -> Option<u16> {
    Some(match c {
        'a' => 0x00,
        's' => 0x01,
        'd' => 0x02,
        'f' => 0x03,
        'h' => 0x04,
        'g' => 0x05,
        'z' => 0x06,
        'x' => 0x07,
        'c' => 0x08,
        'v' => 0x09,
        'b' => 0x0B,
        'q' => 0x0C,
        'w' => 0x0D,
        'e' => 0x0E,
        'r' => 0x0F,
        'y' => 0x10,
        't' => 0x11,
        '1' => 0x12,
        '2' => 0x13,
        '3' => 0x14,
        '4' => 0x15,
        '6' => 0x16,
        '5' => 0x17,
        '=' => 0x18,
        '9' => 0x19,
        '7' => 0x1A,
        '-' => 0x1B,
        '8' => 0x1C,
        '0' => 0x1D,
        ']' => 0x1E,
        'o' => 0x1F,
        'u' => 0x20,
        '[' => 0x21,
        'i' => 0x22,
        'p' => 0x23,
        'l' => 0x25,
        'j' => 0x26,
        '\'' => 0x27,
        'k' => 0x28,
        ';' => 0x29,
        '\\' => 0x2A,
        ',' => 0x2B,
        '/' => 0x2C,
        'n' => 0x2D,
        'm' => 0x2E,
        '.' => 0x2F,
        '`' => 0x32,
        ' ' => 0x31,
        _ => return None,
    })
}
