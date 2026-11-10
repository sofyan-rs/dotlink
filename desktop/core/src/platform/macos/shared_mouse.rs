//! Shared mouse on macOS (`crate::shared_mouse`). A Quartz event tap sees the Mac's own mouse
//! and keyboard; what the other PC sends is posted as CoreGraphics events, like the Remote.
//!
//! Driving another PC: at a screen edge with a linked PC beyond it, the pointer is detached
//! from the mouse (`CGAssociateMouseAndMouseCursorPosition`) and hidden, and the tap swallows
//! the mouse and keyboard and sends their deltas and keys over instead.
//!
//! Driven by another PC: the pointer is placed where the other PC's motion takes it, and goes
//! back when pushed out the edge it came in through. The Mac's own mouse moving ends it.
//!
//! Keys go as PC scan codes (see `crate::shared_mouse`), translated from and to Mac key codes
//! by position. ⌘ and Ctrl trade places on the way, so the shortcuts each side's fingers know
//! keep working: Ctrl+C on the PC is ⌘C on the Mac, ⌘C on the Mac is Ctrl+C on the PC, and the
//! Mac's Ctrl is the PC's Windows key.
//!
//! Ctrl+⌥+Esc brings the pointer back while driving another PC. The tap needs the
//! Accessibility permission (the same one the Remote needs); allowed later, Dotlink has to be
//! restarted for it.

use super::input::{
    CFRelease, CGDisplayBounds, CGEventCreateKeyboardEvent, CGEventCreateMouseEvent, CGEventCreateScrollWheelEvent2,
    CGEventGetLocation, CGEventRef, CGEventSetFlags, CGEventSetIntegerValueField, CGGetActiveDisplayList, CGPoint, CGRect,
    CFTypeRef, CLICK_STATE, COMMAND, CONTROL, DELTA_X, DELTA_Y, DOUBLE_CLICK, Event, LEFT_DOWN, LEFT_DRAGGED, LEFT_UP,
    MOVED, NOTCH_PIXELS, OPTION, OTHER_DOWN, OTHER_DRAGGED, OTHER_UP, RIGHT_DOWN, RIGHT_DRAGGED, RIGHT_UP, SCROLL_PIXELS,
    SHIFT, kCFBooleanTrue,
};
use crate::config::ScreenSide;
use crate::shared_mouse::{Cmd, MouseMsg, Out};
use anyhow::{Result, anyhow, bail};
use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

type TapCallback = unsafe extern "C" fn(*mut c_void, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventTapCreate(tap: u32, place: u32, options: u32, mask: u64, callback: TapCallback, info: *mut c_void) -> *mut c_void;
    fn CGEventTapEnable(tap: *mut c_void, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    fn CGWarpMouseCursorPosition(to: CGPoint) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFRunLoopCommonModes: CFTypeRef;
    fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: CFTypeRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(run_loop: *mut c_void);
    fn CFStringCreateWithCString(allocator: *const c_void, s: *const c_char, encoding: u32) -> CFTypeRef;
}

// CGEventTapLocation, CGEventTapPlacement, CGEventTapOptions
const SESSION_TAP: u32 = 1;
const HEAD_INSERT: u32 = 0;
const TAP_DEFAULT: u32 = 0;
// CGEventType (the mouse ones come from input.rs)
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const SCROLL_WHEEL: u32 = 22;
const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
// CGEventField
const BUTTON_NUMBER: u32 = 3;
const KEYCODE: u32 = 9;
const POINT_DELTA_AXIS_1: u32 = 96;
const POINT_DELTA_AXIS_2: u32 = 97;
const USER_DATA: u32 = 42;
/// CGEventFlags: Caps Lock is on.
const ALPHA_SHIFT: u64 = 1 << 16;
/// kCGEventSourceStateHIDSystemState: the buttons as the hardware has them.
const HID_STATE: i32 = 1;
const UTF8: u32 = 0x0800_0100;

/// Marks the events posted here (`USER_DATA`), so the tap lets them through.
const MAGIC: i64 = 0x444c_4e4b;

/// Mac key codes of the modifiers.
const KVK_ESCAPE: u16 = 0x35;
const KVK_COMMAND: u16 = 0x37;
const KVK_RIGHT_COMMAND: u16 = 0x36;
const KVK_CONTROL: u16 = 0x3B;
const KVK_RIGHT_CONTROL: u16 = 0x3E;
const KVK_CAPS_LOCK: u16 = 0x39;
const KVK_V: u16 = 0x09;

pub struct SharedMouse {
    shared: Arc<Shared>,
    run_loop: usize,
    thread: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    state: Mutex<State>,
    /// The tap, to switch it back on when macOS turns it off (it took too long).
    tap: AtomicPtr<c_void>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        // A panic is no reason to stop the mouse.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl SharedMouse {
    pub fn start(out: UnboundedSender<Out>) -> Result<Self> {
        if !super::input::trusted() {
            bail!("allow Dotlink in System Settings → Privacy & Security → Accessibility, then restart it");
        }
        allow_background_cursor();
        let shared = Arc::new(Shared { state: Mutex::new(State::new(out)), tap: AtomicPtr::new(std::ptr::null_mut()) });
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let for_thread = shared.clone();
        let thread = std::thread::Builder::new().name("shared-mouse".into()).spawn(move || run(for_thread, ready_tx))?;
        let run_loop = ready_rx.recv().map_err(|_| anyhow!("the event tap thread ended"))?.map_err(|e| anyhow!(e))?;
        Ok(Self { shared, run_loop, thread: Mutex::new(Some(thread)) })
    }

    pub fn send(&self, cmd: Cmd) {
        self.shared.state().command(cmd);
    }

    /// Lets go of everything (service stop): the pointer shows again, held keys come up.
    pub fn stop(&self) {
        {
            let mut state = self.shared.state();
            state.end_receiving(false);
            state.end_sending(None);
        }
        // SAFETY: the thread's run loop, still running (it only ends here).
        unsafe { CFRunLoopStop(self.run_loop as *mut c_void) };
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

fn run(shared: Arc<Shared>, ready: std_mpsc::Sender<Result<usize, String>>) {
    let mask = [
        MOVED,
        LEFT_DOWN,
        LEFT_UP,
        RIGHT_DOWN,
        RIGHT_UP,
        LEFT_DRAGGED,
        RIGHT_DRAGGED,
        OTHER_DOWN,
        OTHER_UP,
        OTHER_DRAGGED,
        SCROLL_WHEEL,
        KEY_DOWN,
        KEY_UP,
        FLAGS_CHANGED,
    ]
    .iter()
    .fold(0u64, |m, t| m | (1 << t));
    // SAFETY: `shared` outlives the tap: this thread holds it until the run loop ends, and the
    // tap is gone after that.
    unsafe {
        let info = Arc::as_ptr(&shared) as *mut c_void;
        let tap = CGEventTapCreate(SESSION_TAP, HEAD_INSERT, TAP_DEFAULT, mask, tap_callback, info);
        if tap.is_null() {
            let _ = ready.send(Err("macOS refused the event tap: allow Dotlink under Accessibility and Input Monitoring".into()));
            return;
        }
        shared.tap.store(tap, Ordering::SeqCst);
        let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
        let run_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);
        let _ = ready.send(Ok(run_loop as usize));
        CFRunLoopRun();
        CGEventTapEnable(tap, false);
        shared.tap.store(std::ptr::null_mut(), Ordering::SeqCst);
        CFRelease(source as CFTypeRef);
        CFRelease(tap as CFTypeRef);
    }
}

unsafe extern "C" fn tap_callback(_proxy: *mut c_void, kind: u32, event: CGEventRef, info: *mut c_void) -> CGEventRef {
    // SAFETY: `info` is the Arc'd Shared the tap was made with (see `run`).
    let shared = unsafe { &*(info as *const Shared) };
    if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER_INPUT {
        let tap = shared.tap.load(Ordering::SeqCst);
        if !tap.is_null() {
            // SAFETY: the live tap.
            unsafe { CGEventTapEnable(tap, true) };
        }
        return event;
    }
    // SAFETY: a valid event, handed to the tap.
    if unsafe { CGEventGetIntegerValueField(event, USER_DATA) } == MAGIC {
        return event;
    }
    if shared.state().on_event(kind, event) { std::ptr::null_mut() } else { event }
}

/// Driving another PC.
struct Sending {
    id: String,
    side: ScreenSide,
    /// Where the pointer left this screen; it waits there, hidden.
    exit: CGPoint,
}

/// Driven by another PC.
struct Receiving {
    id: String,
    edge: ScreenSide,
    pos: CGPoint,
    /// Mac key codes held down.
    keys: Vec<u16>,
    buttons: [bool; 5],
    /// The last click's button, time and count, for double clicks (Finder reads the count).
    last_click: Option<(u8, Instant, i64)>,
    scroll_rem: (f32, f32),
}

struct State {
    out: UnboundedSender<Out>,
    /// Hold ⌘V back: files copied on another PC are on their way (`clip_files.rs`).
    armed: bool,
    /// The V of a ⌘V held back: its repeats and its release go too.
    held_v: bool,
    layout: Vec<(String, Option<ScreenSide>)>,
    sending: Option<Sending>,
    receiving: Option<Receiving>,
    hidden: bool,
}

impl State {
    fn new(out: UnboundedSender<Out>) -> Self {
        Self { out, armed: false, held_v: false, layout: Vec::new(), sending: None, receiving: None, hidden: false }
    }

    fn emit(&self, id: &str, msg: MouseMsg) {
        let _ = self.out.send(Out::Mouse(id.to_string(), msg));
    }

    /// The V of ⌘V (`command`: held): true when held back for files on their way.
    fn hold_paste(&mut self, down: bool, command: bool) -> bool {
        if down && (self.held_v || (self.armed && command)) {
            if !self.held_v {
                let _ = self.out.send(Out::Paste);
            }
            self.held_v = true;
            return true;
        }
        !down && std::mem::take(&mut self.held_v)
    }

    // ---------- the Mac's own mouse and keyboard ----------

    /// True: swallowed.
    fn on_event(&mut self, kind: u32, event: CGEventRef) -> bool {
        let is_mouse = !matches!(kind, KEY_DOWN | KEY_UP | FLAGS_CHANGED);
        // The Mac's own mouse moved while another PC drives it: it takes the pointer back.
        if is_mouse && self.receiving.is_some() {
            self.end_receiving(true);
        }
        let Some(s) = &self.sending else {
            if matches!(kind, KEY_DOWN | KEY_UP) {
                // SAFETY: a valid event for each read.
                let (code, flags) = unsafe { (CGEventGetIntegerValueField(event, KEYCODE) as u16, CGEventGetFlags(event)) };
                let command = flags & COMMAND != 0 && flags & (CONTROL | OPTION) == 0;
                return code == KVK_V && self.hold_paste(kind == KEY_DOWN, command);
            }
            // SAFETY: a valid event.
            return kind == MOVED && self.at_edge(unsafe { CGEventGetLocation(event) });
        };
        let id = s.id.clone();
        // SAFETY: a valid event for each read.
        let field = |f: u32| unsafe { CGEventGetIntegerValueField(event, f) };
        let other_button = || match field(BUTTON_NUMBER) {
            3 => 3,
            4 => 4,
            _ => 2,
        };
        let msg = match kind {
            MOVED | LEFT_DRAGGED | RIGHT_DRAGGED | OTHER_DRAGGED => {
                let (x, y) = (field(DELTA_X) as i32, field(DELTA_Y) as i32);
                (x != 0 || y != 0).then_some(MouseMsg::Move { x, y })
            }
            LEFT_DOWN => Some(MouseMsg::Btn { b: 0, down: true }),
            LEFT_UP => Some(MouseMsg::Btn { b: 0, down: false }),
            RIGHT_DOWN => Some(MouseMsg::Btn { b: 1, down: true }),
            RIGHT_UP => Some(MouseMsg::Btn { b: 1, down: false }),
            OTHER_DOWN => Some(MouseMsg::Btn { b: other_button(), down: true }),
            OTHER_UP => Some(MouseMsg::Btn { b: other_button(), down: false }),
            SCROLL_WHEEL => {
                // Pixels; a wheel notch is about NOTCH_PIXELS. Positive axis 1 scrolls up, like
                // the PC's wheel; positive axis 2 scrolls left, the PC's right.
                let per_pixel = 120. / NOTCH_PIXELS as f64;
                let y = (field(POINT_DELTA_AXIS_1) as f64 * per_pixel).round() as i32;
                let x = (-field(POINT_DELTA_AXIS_2) as f64 * per_pixel).round() as i32;
                (x != 0 || y != 0).then_some(MouseMsg::Wheel { x, y })
            }
            KEY_DOWN | KEY_UP => {
                let code = field(KEYCODE) as u16;
                let down = kind == KEY_DOWN;
                // SAFETY: a valid event.
                let flags = unsafe { CGEventGetFlags(event) };
                if down && code == KVK_ESCAPE && flags & CONTROL != 0 && flags & OPTION != 0 {
                    self.end_sending(None);
                    self.emit(&id, MouseMsg::Stop);
                    return true;
                }
                scan_code(code).map(|sc| MouseMsg::Key { sc, down })
            }
            FLAGS_CHANGED => {
                let code = field(KEYCODE) as u16;
                // SAFETY: a valid event.
                let flags = unsafe { CGEventGetFlags(event) };
                match (scan_code(code), modifier_flag(code)) {
                    // Caps Lock reports its light, not the key: a press either way.
                    (Some(sc), _) if code == KVK_CAPS_LOCK => {
                        self.emit(&id, MouseMsg::Key { sc, down: true });
                        Some(MouseMsg::Key { sc, down: false })
                    }
                    (Some(sc), flag) if flag != 0 => Some(MouseMsg::Key { sc, down: flags & flag != 0 }),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(msg) = msg {
            self.emit(&id, msg);
        }
        true
    }

    /// The pointer reached an edge: over to the PC beyond it, if there is one. True: the move is
    /// swallowed.
    fn at_edge(&mut self, pt: CGPoint) -> bool {
        let Some(side) = edge_at(pt) else { return false };
        let Some(id) = self.layout.iter().find(|(_, s)| *s == Some(side)).map(|(id, _)| id.clone()) else { return false };
        // Not in the middle of a drag: the button would stay down here.
        // SAFETY: plain values.
        if (0..5).any(|b| unsafe { CGEventSourceButtonState(HID_STATE, b) }) {
            return false;
        }
        // SAFETY: plain values.
        unsafe { CGAssociateMouseAndMouseCursorPosition(0) };
        self.hide_pointer();
        self.emit(&id, MouseMsg::Enter { edge: side.opposite(), pos: fraction(side, pt) });
        self.sending = Some(Sending { id, side, exit: pt });
        true
    }

    /// Back to the Mac's own screen: at `at`, or where it left.
    fn end_sending(&mut self, at: Option<CGPoint>) {
        let Some(s) = self.sending.take() else { return };
        let to = at.unwrap_or_else(|| inward(s.exit, s.side));
        // SAFETY: plain values.
        unsafe {
            CGWarpMouseCursorPosition(to);
            CGAssociateMouseAndMouseCursorPosition(1);
        }
        self.show_pointer();
        let _ = self.out.send(Out::Arrived);
    }

    fn hide_pointer(&mut self) {
        if !self.hidden {
            // SAFETY: the main display (0); balanced by show_pointer.
            unsafe { CGDisplayHideCursor(0) };
            self.hidden = true;
        }
    }

    fn show_pointer(&mut self) {
        if self.hidden {
            // SAFETY: as in hide_pointer.
            unsafe { CGDisplayShowCursor(0) };
            self.hidden = false;
        }
    }

    // ---------- from the other PC ----------

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Layout(layout) => {
                self.layout = layout;
                if self.sending.as_ref().is_some_and(|s| !self.layout.iter().any(|(id, side)| *id == s.id && *side == Some(s.side))) {
                    self.end_sending(None);
                }
                if self.receiving.as_ref().is_some_and(|r| !self.layout.iter().any(|(id, _)| *id == r.id)) {
                    self.end_receiving(false);
                }
            }
            Cmd::Input { from, msg } => self.input(from, msg),
            Cmd::ArmPaste(on) => self.armed = on,
            Cmd::Paste => {
                // ⌘ goes in the flags: no need to press it.
                post_key(KVK_V, true, COMMAND);
                post_key(KVK_V, false, COMMAND);
            }
        }
    }

    fn input(&mut self, from: String, msg: MouseMsg) {
        match msg {
            MouseMsg::Enter { edge, pos } => {
                // Both mice moved at once: the other PC wins.
                if let Some(s) = &self.sending {
                    let id = s.id.clone();
                    self.end_sending(None);
                    self.emit(&id, MouseMsg::Stop);
                }
                self.end_receiving(false);
                let at = entry_point(edge, pos);
                post_mouse(MOVED, at, 0, (0, 0));
                self.receiving = Some(Receiving {
                    id: from,
                    edge,
                    pos: at,
                    keys: Vec::new(),
                    buttons: [false; 5],
                    last_click: None,
                    scroll_rem: (0., 0.),
                });
                let _ = self.out.send(Out::Arrived);
            }
            MouseMsg::Leave { pos } => {
                if let Some(s) = self.sending.as_ref().filter(|s| s.id == from) {
                    let at = inward(entry_point(s.side, pos), s.side);
                    self.end_sending(Some(at));
                }
            }
            MouseMsg::Stop => {
                if self.sending.as_ref().is_some_and(|s| s.id == from) {
                    self.end_sending(None);
                }
                if self.receiving.as_ref().is_some_and(|r| r.id == from) {
                    self.end_receiving(false);
                }
            }
            MouseMsg::Move { x, y } if self.receiving.as_ref().is_some_and(|r| r.id == from) => self.receive_move(x, y),
            msg => {
                let Some(r) = self.receiving.as_mut().filter(|r| r.id == from) else { return };
                match msg {
                    MouseMsg::Btn { b, down } if (b as usize) < r.buttons.len() => {
                        r.buttons[b as usize] = down;
                        let count = if down {
                            let now = Instant::now();
                            let n = match r.last_click {
                                Some((last, when, n)) if last == b && now - when < DOUBLE_CLICK => n + 1,
                                _ => 1,
                            };
                            r.last_click = Some((b, now, n));
                            n
                        } else {
                            r.last_click.map_or(1, |(_, _, n)| n)
                        };
                        post_button(b, down, r.pos, count);
                    }
                    MouseMsg::Wheel { x, y } => {
                        let fy = y as f32 / 120. * NOTCH_PIXELS + r.scroll_rem.1;
                        let fx = -x as f32 / 120. * NOTCH_PIXELS + r.scroll_rem.0;
                        let (px, py) = (fx.trunc() as i32, fy.trunc() as i32);
                        r.scroll_rem = (fx - px as f32, fy - py as f32);
                        if px != 0 || py != 0 {
                            // SAFETY: plain values; checked by Event::new.
                            if let Ok(event) =
                                Event::new(unsafe { CGEventCreateScrollWheelEvent2(std::ptr::null(), SCROLL_PIXELS, 2, py, px, 0) })
                            {
                                post(event);
                            }
                        }
                    }
                    MouseMsg::Key { sc, down } => {
                        let Some(code) = mac_key(sc) else { return };
                        // The other PC's paste (⌘V here), held back like the Mac's own.
                        if code == KVK_V {
                            let command = r.keys.iter().any(|&k| modifier_flag(k) == COMMAND);
                            if self.hold_paste(down, command) {
                                return;
                            }
                        }
                        let Some(r) = self.receiving.as_mut() else { return };
                        r.keys.retain(|&k| k != code);
                        if down {
                            r.keys.push(code);
                        }
                        let flags = r.keys.iter().fold(0, |f, &k| f | modifier_flag(k));
                        post_key(code, down, flags);
                    }
                    _ => {}
                }
            }
        }
    }

    fn receive_move(&mut self, dx: i32, dy: i32) {
        let Some(r) = self.receiving.as_mut() else { return };
        let screens = displays();
        let target = CGPoint { x: r.pos.x + dx as f64, y: r.pos.y + dy as f64 };
        let next = if screens.iter().any(|d| d.contains(target)) {
            target
        } else {
            let rc = screens.iter().find(|d| d.contains(r.pos)).copied().unwrap_or_else(|| union(&screens));
            let clamped = rc.clamp(target);
            let pushed_out = match r.edge {
                ScreenSide::Left => target.x < rc.origin.x,
                ScreenSide::Right => target.x >= rc.origin.x + rc.size.x,
                ScreenSide::Above => target.y < rc.origin.y,
                ScreenSide::Below => target.y >= rc.origin.y + rc.size.y,
            };
            if pushed_out && edge_at(clamped) == Some(r.edge) {
                let (id, pos) = (r.id.clone(), fraction(r.edge, clamped));
                self.end_receiving(false);
                self.emit(&id, MouseMsg::Leave { pos });
                return;
            }
            clamped
        };
        r.pos = next;
        // A held button makes it a drag, which apps tell from a move.
        let (kind, button) = match r.buttons.iter().position(|&b| b) {
            Some(0) => (LEFT_DRAGGED, 0),
            Some(1) => (RIGHT_DRAGGED, 1),
            Some(b) => (OTHER_DRAGGED, b as u32),
            None => (MOVED, 0),
        };
        post_mouse(kind, next, button, (dx, dy));
    }

    /// Lets go of the keys and buttons the other PC holds down here. `tell`: the Mac's own mouse
    /// took over, so the other PC stops sending.
    fn end_receiving(&mut self, tell: bool) {
        let Some(r) = self.receiving.take() else { return };
        for &code in r.keys.iter().rev() {
            post_key(code, false, 0);
        }
        for (b, _) in r.buttons.iter().enumerate().filter(|(_, down)| **down) {
            post_button(b as u8, false, r.pos, 1);
        }
        if tell {
            self.emit(&r.id, MouseMsg::Stop);
        }
    }
}

// ---------- screens ----------

fn displays() -> Vec<CGRect> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    // SAFETY: the buffer holds `ids.len()` ids.
    if unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0 {
        return Vec::new();
    }
    // SAFETY: ids from CGGetActiveDisplayList.
    ids[..count as usize].iter().map(|&id| unsafe { CGDisplayBounds(id) }).collect()
}

/// The rectangle around every display.
fn union(screens: &[CGRect]) -> CGRect {
    let Some(first) = screens.first() else {
        return CGRect { origin: CGPoint { x: 0., y: 0. }, size: CGPoint { x: 1., y: 1. } };
    };
    let (mut x0, mut y0) = (first.origin.x, first.origin.y);
    let (mut x1, mut y1) = (x0 + first.size.x, y0 + first.size.y);
    for d in screens {
        x0 = x0.min(d.origin.x);
        y0 = y0.min(d.origin.y);
        x1 = x1.max(d.origin.x + d.size.x);
        y1 = y1.max(d.origin.y + d.size.y);
    }
    CGRect { origin: CGPoint { x: x0, y: y0 }, size: CGPoint { x: x1 - x0, y: y1 - y0 } }
}

/// The outer edge `pt` is on: an edge of its display with no other display beyond it.
fn edge_at(pt: CGPoint) -> Option<ScreenSide> {
    let screens = displays();
    let d = screens.iter().find(|d| d.contains(pt))?;
    let none_at = |x: f64, y: f64| !screens.iter().any(|s| s.contains(CGPoint { x, y }));
    let (left, top) = (d.origin.x, d.origin.y);
    let (right, bottom) = (left + d.size.x, top + d.size.y);
    if pt.x <= left && none_at(left - 1., pt.y) {
        Some(ScreenSide::Left)
    } else if pt.x >= right - 1. && none_at(right, pt.y) {
        Some(ScreenSide::Right)
    } else if pt.y <= top && none_at(pt.x, top - 1.) {
        Some(ScreenSide::Above)
    } else if pt.y >= bottom - 1. && none_at(pt.x, bottom) {
        Some(ScreenSide::Below)
    } else {
        None
    }
}

/// How far along `edge` of all the displays `pt` is, 0 to 1.
fn fraction(edge: ScreenSide, pt: CGPoint) -> f32 {
    let v = union(&displays());
    let f = match edge {
        ScreenSide::Left | ScreenSide::Right => (pt.y - v.origin.y) / (v.size.y - 1.).max(1.),
        ScreenSide::Above | ScreenSide::Below => (pt.x - v.origin.x) / (v.size.x - 1.).max(1.),
    };
    f.clamp(0., 1.) as f32
}

/// The point on `edge` of the displays at `pos` along it (0 to 1).
fn entry_point(edge: ScreenSide, pos: f32) -> CGPoint {
    let screens = displays();
    let v = union(&screens);
    let pos = if pos.is_finite() { pos.clamp(0., 1.) as f64 } else { 0.5 };
    let (left, top) = (v.origin.x, v.origin.y);
    let (right, bottom) = (left + v.size.x - 1., top + v.size.y - 1.);
    let pt = match edge {
        ScreenSide::Left => CGPoint { x: left, y: top + (bottom - top) * pos },
        ScreenSide::Right => CGPoint { x: right, y: top + (bottom - top) * pos },
        ScreenSide::Above => CGPoint { x: left + (right - left) * pos, y: top },
        ScreenSide::Below => CGPoint { x: left + (right - left) * pos, y: bottom },
    };
    // Displays of different sizes leave parts of that rectangle without one: onto the nearest
    // display, on its edge.
    let nearest = |d: &&CGRect| {
        let c = d.clamp(pt);
        (c.x - pt.x).powi(2) + (c.y - pt.y).powi(2)
    };
    let Some(d) = screens.iter().min_by(|a, b| nearest(a).total_cmp(&nearest(b))) else { return pt };
    let mut pt = d.clamp(pt);
    match edge {
        ScreenSide::Left => pt.x = d.origin.x,
        ScreenSide::Right => pt.x = d.origin.x + d.size.x - 1.,
        ScreenSide::Above => pt.y = d.origin.y,
        ScreenSide::Below => pt.y = d.origin.y + d.size.y - 1.,
    }
    pt
}

/// A little in from `side`, so the pointer coming back doesn't go straight over again.
fn inward(pt: CGPoint, side: ScreenSide) -> CGPoint {
    const STEP: f64 = 2.;
    match side {
        ScreenSide::Left => CGPoint { x: pt.x + STEP, y: pt.y },
        ScreenSide::Right => CGPoint { x: pt.x - STEP, y: pt.y },
        ScreenSide::Above => CGPoint { x: pt.x, y: pt.y + STEP },
        ScreenSide::Below => CGPoint { x: pt.x, y: pt.y - STEP },
    }
}

/// Lets a background app hide the pointer: CGDisplayHideCursor does nothing for one otherwise.
/// A private WindowServer property, looked up at run time (what Deskflow does too); without it,
/// the pointer just stays visible at the edge.
fn allow_background_cursor() {
    type DefaultConnection = unsafe extern "C" fn() -> i32;
    type SetProperty = unsafe extern "C" fn(i32, i32, CFTypeRef, CFTypeRef) -> i32;
    // SAFETY: symbol lookups; each is called only when found, with the signature it has.
    unsafe {
        let connection = libc::dlsym(libc::RTLD_DEFAULT, c"_CGSDefaultConnection".as_ptr());
        let set = libc::dlsym(libc::RTLD_DEFAULT, c"CGSSetConnectionProperty".as_ptr());
        if connection.is_null() || set.is_null() {
            return;
        }
        let connection: DefaultConnection = std::mem::transmute(connection);
        let set: SetProperty = std::mem::transmute(set);
        let key = CFStringCreateWithCString(std::ptr::null(), c"SetsCursorInBackground".as_ptr(), UTF8);
        if key.is_null() {
            return;
        }
        let cid = connection();
        set(cid, cid, key, kCFBooleanTrue);
        CFRelease(key);
    }
}

// ---------- posting ----------

fn post(event: Event) {
    // SAFETY: a valid event.
    unsafe { CGEventSetIntegerValueField(event.0, USER_DATA, MAGIC) };
    event.post();
}

fn post_mouse(kind: u32, at: CGPoint, button: u32, delta: (i32, i32)) {
    // SAFETY: plain values; checked by Event::new.
    let Ok(event) = Event::new(unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, at, button) }) else { return };
    // Games and 3D apps read the deltas, not the position.
    // SAFETY: a valid event.
    unsafe {
        CGEventSetIntegerValueField(event.0, DELTA_X, delta.0 as i64);
        CGEventSetIntegerValueField(event.0, DELTA_Y, delta.1 as i64);
    }
    post(event);
}

fn post_button(b: u8, down: bool, at: CGPoint, count: i64) {
    let (kind, button) = match (b, down) {
        (0, true) => (LEFT_DOWN, 0),
        (0, false) => (LEFT_UP, 0),
        (1, true) => (RIGHT_DOWN, 1),
        (1, false) => (RIGHT_UP, 1),
        (b, true) => (OTHER_DOWN, b as u32),
        (b, false) => (OTHER_UP, b as u32),
    };
    // SAFETY: plain values; checked by Event::new.
    let Ok(event) = Event::new(unsafe { CGEventCreateMouseEvent(std::ptr::null(), kind, at, button) }) else { return };
    // SAFETY: a valid event.
    unsafe { CGEventSetIntegerValueField(event.0, CLICK_STATE, count) };
    post(event);
}

/// A key with the modifiers held at that moment: macOS reads ⌘ from the flags.
fn post_key(code: u16, down: bool, flags: u64) {
    // SAFETY: plain values; checked by Event::new.
    let Ok(event) = Event::new(unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), code, down) }) else { return };
    // SAFETY: a valid event.
    unsafe { CGEventSetFlags(event.0, flags) };
    post(event);
}

// ---------- keys ----------

/// The flag a modifier key sets; 0 for other keys.
fn modifier_flag(code: u16) -> u64 {
    match code {
        KVK_COMMAND | KVK_RIGHT_COMMAND => COMMAND,
        0x38 | 0x3C => SHIFT,
        0x3A | 0x3D => OPTION,
        KVK_CONTROL | KVK_RIGHT_CONTROL => CONTROL,
        KVK_CAPS_LOCK => ALPHA_SHIFT,
        _ => 0,
    }
}

/// ⌘ and Ctrl trade places (see the top): (PC scan code, Mac key code).
const SWAPPED: [(u16, u16); 4] =
    [(0x1D, KVK_COMMAND), (0xE01D, KVK_RIGHT_COMMAND), (0xE05B, KVK_CONTROL), (0xE05C, KVK_RIGHT_CONTROL)];

/// PC scan code (set 1) → Mac key code, by position on the keyboard.
const KEYS: &[(u16, u16)] = &[
    (0x01, 0x35), // Esc
    (0x02, 0x12), // 1
    (0x03, 0x13),
    (0x04, 0x14),
    (0x05, 0x15),
    (0x06, 0x17),
    (0x07, 0x16),
    (0x08, 0x1A),
    (0x09, 0x1C),
    (0x0A, 0x19),
    (0x0B, 0x1D), // 0
    (0x0C, 0x1B), // -
    (0x0D, 0x18), // =
    (0x0E, 0x33), // Backspace
    (0x0F, 0x30), // Tab
    (0x10, 0x0C), // Q
    (0x11, 0x0D),
    (0x12, 0x0E),
    (0x13, 0x0F),
    (0x14, 0x11),
    (0x15, 0x10),
    (0x16, 0x20),
    (0x17, 0x22),
    (0x18, 0x1F),
    (0x19, 0x23), // P
    (0x1A, 0x21), // [
    (0x1B, 0x1E), // ]
    (0x1C, 0x24), // Enter
    (0x1E, 0x00), // A
    (0x1F, 0x01),
    (0x20, 0x02),
    (0x21, 0x03),
    (0x22, 0x05),
    (0x23, 0x04),
    (0x24, 0x26),
    (0x25, 0x28),
    (0x26, 0x25), // L
    (0x27, 0x29), // ;
    (0x28, 0x27), // '
    (0x29, 0x32), // `
    (0x2A, 0x38), // Left Shift
    (0x2B, 0x2A), // \
    (0x2C, 0x06), // Z
    (0x2D, 0x07),
    (0x2E, 0x08),
    (0x2F, 0x09),
    (0x30, 0x0B),
    (0x31, 0x2D),
    (0x32, 0x2E), // M
    (0x33, 0x2B), // ,
    (0x34, 0x2F), // .
    (0x35, 0x2C), // /
    (0x36, 0x3C), // Right Shift
    (0x37, 0x43), // keypad *
    (0x38, 0x3A), // Left Alt: Option
    (0x39, 0x31), // Space
    (0x3A, KVK_CAPS_LOCK),
    (0x3B, 0x7A), // F1
    (0x3C, 0x78),
    (0x3D, 0x63),
    (0x3E, 0x76),
    (0x3F, 0x60),
    (0x40, 0x61),
    (0x41, 0x62),
    (0x42, 0x64),
    (0x43, 0x65),
    (0x44, 0x6D), // F10
    (0x45, 0x47), // Num Lock: Clear
    (0x47, 0x59), // keypad 7
    (0x48, 0x5B),
    (0x49, 0x5C),
    (0x4A, 0x4E), // keypad -
    (0x4B, 0x56),
    (0x4C, 0x57),
    (0x4D, 0x58),
    (0x4E, 0x45), // keypad +
    (0x4F, 0x53),
    (0x50, 0x54),
    (0x51, 0x55),
    (0x52, 0x52), // keypad 0
    (0x53, 0x41), // keypad .
    (0x56, 0x0A), // the ISO key next to Left Shift
    (0x57, 0x67), // F11
    (0x58, 0x6F), // F12
    (0x64, 0x69), // F13
    (0x65, 0x6B),
    (0x66, 0x71),
    (0x67, 0x6A),
    (0x68, 0x40),
    (0x69, 0x4F),
    (0x6A, 0x50), // F19
    (0x73, 0x5E), // JIS _
    (0x7D, 0x5D), // JIS ¥
    (0xE01C, 0x4C), // keypad Enter
    (0xE020, 0x4A), // Mute
    (0xE02E, 0x49), // Volume Down
    (0xE030, 0x48), // Volume Up
    (0xE035, 0x4B), // keypad /
    (0xE038, 0x3D), // Right Alt: Right Option
    (0xE047, 0x73), // Home
    (0xE048, 0x7E), // Up
    (0xE049, 0x74), // Page Up
    (0xE04B, 0x7B), // Left
    (0xE04D, 0x7C), // Right
    (0xE04F, 0x77), // End
    (0xE050, 0x7D), // Down
    (0xE051, 0x79), // Page Down
    (0xE052, 0x72), // Insert: Help
    (0xE053, 0x75), // Delete (forward)
    (0xE05D, 0x6E), // Menu
];

fn mac_key(sc: u16) -> Option<u16> {
    SWAPPED.iter().chain(KEYS).find(|(s, _)| *s == sc).map(|(_, k)| *k)
}

fn scan_code(code: u16) -> Option<u16> {
    SWAPPED.iter().chain(KEYS).find(|(_, k)| *k == code).map(|(s, _)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_both_ways() {
        for &(sc, code) in SWAPPED.iter().chain(KEYS) {
            assert_eq!(mac_key(sc), Some(code), "{sc:#x}");
            assert_eq!(scan_code(code), Some(sc), "{code:#x}");
        }
        // Ctrl+C on the PC is ⌘C here.
        assert_eq!(mac_key(0x1D), Some(KVK_COMMAND));
        assert_eq!(scan_code(KVK_CONTROL), Some(0xE05B));
    }
}
