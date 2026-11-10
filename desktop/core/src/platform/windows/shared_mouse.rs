//! Shared mouse on Windows (`crate::shared_mouse`). One thread holds low-level mouse and
//! keyboard hooks and plays what the other PC sends with SendInput.
//!
//! Driving another PC: when the pointer reaches a screen edge with a linked PC beyond it, the
//! pointer is parked at the middle of its screen and hidden, and the hooks swallow the local
//! mouse and keyboard and send them over instead. Motion is the distance from the parked point
//! (Windows stops the pointer at the screen's edge, so the edge itself would lose it).
//!
//! Driven by another PC: the pointer is placed absolutely (so this PC's pointer acceleration
//! doesn't add to the other's), and it goes back when pushed out the edge it came in through.
//! This PC's own mouse moving ends it: whoever touches a mouse gets the pointer.
//!
//! Ctrl+Alt+Esc brings the pointer back while driving another PC, should it get stuck there.
//! Like the Remote, SendInput can't reach administrator windows (UAC, Task Manager), and the
//! hooks don't see the sign-in or Ctrl+Alt+Del screens.

use crate::config::ScreenSide;
use crate::shared_mouse::{Cmd, MouseMsg, Out};
use anyhow::{Result, anyhow};
use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;
use tokio::sync::mpsc::UnboundedSender;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITOR_FROM_FLAGS, MONITORINFO, MonitorFromPoint,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Wakes the thread for the commands waiting in its channel.
const WM_WAKE: u32 = WM_APP + 1;

/// The scan codes of the keys Ctrl+Alt+Esc is made of (either Ctrl and Alt).
const SC_ESC: u16 = 0x01;
const SC_CTRL: [u16; 2] = [0x1D, 0xE01D];
const SC_ALT: [u16; 2] = [0x38, 0xE038];

pub struct SharedMouse {
    thread_id: u32,
    tx: std_mpsc::Sender<Cmd>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl SharedMouse {
    pub fn start(out: UnboundedSender<Out>) -> Result<Self> {
        let (tx, rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let thread = std::thread::Builder::new().name("shared-mouse".into()).spawn(move || run(out, rx, ready_tx))?;
        let thread_id = ready_rx.recv().map_err(|_| anyhow!("the hook thread ended"))?.map_err(|e| anyhow!(e))?;
        Ok(Self { thread_id, tx, thread: Mutex::new(Some(thread)) })
    }

    pub fn send(&self, cmd: Cmd) {
        if self.tx.send(cmd).is_ok() {
            unsafe { PostThreadMessageW(self.thread_id, WM_WAKE, 0, 0) };
        }
    }

    /// Lets go of everything (service stop): the pointer shows again, held keys come up.
    pub fn stop(&self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn run(out: UnboundedSender<Out>, rx: std_mpsc::Receiver<Cmd>, ready: std_mpsc::Sender<Result<u32, String>>) {
    unsafe {
        // Monitors, the pointer and SendInput in physical pixels, like the hooks' coordinates.
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // The thread's message queue, before anyone posts to it.
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
        // A crash while driving another PC would have left the pointer hidden.
        show_pointer();
        let module = GetModuleHandleW(std::ptr::null());
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
        let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), module, 0);
        if mouse.is_null() || keys.is_null() {
            let error = std::io::Error::last_os_error();
            for hook in [mouse, keys] {
                if !hook.is_null() {
                    UnhookWindowsHookEx(hook);
                }
            }
            let _ = ready.send(Err(format!("can't watch the mouse and keyboard: {error}")));
            return;
        }
        STATE.with(|s| *s.borrow_mut() = Some(State::new(out)));
        let _ = ready.send(Ok(GetCurrentThreadId()));
        // The hooks are called from inside GetMessageW.
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_WAKE {
                let cmds = merge(rx.try_iter().collect());
                STATE.with(|s| {
                    if let Some(state) = s.borrow_mut().as_mut() {
                        for cmd in cmds {
                            state.command(cmd);
                        }
                    }
                });
            }
        }
        STATE.with(|s| {
            if let Some(mut state) = s.borrow_mut().take() {
                state.end_receiving(false);
                state.end_sending(None, true);
            }
        });
        UnhookWindowsHookEx(mouse);
        UnhookWindowsHookEx(keys);
    }
}

/// Motion and scrolling that piled up (the hooks kept the thread busy) go as one move and one
/// scroll: each SendInput is a redraw in the app under the pointer.
fn merge(cmds: Vec<Cmd>) -> Vec<Cmd> {
    let mut out: Vec<Cmd> = Vec::with_capacity(cmds.len());
    for cmd in cmds {
        if let (Some(Cmd::Input { from: last_from, msg: last }), Cmd::Input { from, msg }) = (out.last_mut(), &cmd)
            && last_from == from
        {
            match (last, msg) {
                (MouseMsg::Move { x, y }, MouseMsg::Move { x: dx, y: dy }) => {
                    *x += dx;
                    *y += dy;
                    continue;
                }
                (MouseMsg::Wheel { x, y, smooth }, MouseMsg::Wheel { x: dx, y: dy, smooth: s }) if smooth == s => {
                    *x += dx;
                    *y += dy;
                    continue;
                }
                _ => {}
            }
        }
        out.push(cmd);
    }
    out
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        // Our own SendInput (and the phone's Remote) comes back through here: only real input.
        if info.flags & LLMHF_INJECTED == 0 && with_state(|s| s.on_mouse(wparam as u32, info)) {
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if info.flags & LLKHF_INJECTED == 0 && with_state(|s| s.on_key(info)) {
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Runs `f` on the thread's state; false (let the input through) when it's busy, which only
/// happens if a hook is called from inside a command.
fn with_state(f: impl FnOnce(&mut State) -> bool) -> bool {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut s| s.as_mut().map(f)).unwrap_or(false))
}

/// Driving another PC.
struct Sending {
    id: String,
    side: ScreenSide,
    /// Where the pointer waits, hidden; motion is measured from here.
    park: POINT,
    /// Physical pixels per logical one on the monitor it waits on (1.5 at 150 %): motion goes
    /// in logical pixels, so the other PC's scaling doesn't change its speed.
    scale: f32,
    /// Where it left this screen.
    exit: POINT,
    ctrl: bool,
    alt: bool,
}

/// Driven by another PC.
struct Receiving {
    id: String,
    /// The edge of this PC's screens it came in through, and goes back through.
    edge: ScreenSide,
    pos: POINT,
    /// The fractions of a pixel motion and scrolling left, so slow moves add up instead of
    /// being lost.
    frac: (f32, f32),
    wheel_rem: (f32, f32),
    keys: Vec<u16>,
    buttons: [bool; 5],
}

struct State {
    out: UnboundedSender<Out>,
    /// Hold Ctrl+V back: files copied on another PC are on their way (`clip_files.rs`).
    armed: bool,
    /// The V of a Ctrl+V held back: its repeats and its release go too.
    held_v: bool,
    /// The linked PCs connected right now, and where their screen is.
    layout: Vec<(String, Option<ScreenSide>)>,
    sending: Option<Sending>,
    receiving: Option<Receiving>,
    /// Relaying: the pointer another PC drives here went on to a third PC.
    parked: Option<Parked>,
}

/// The pointer, driven here, went on to another PC (`crate::shared_mouse`'s relaying).
struct Parked {
    r: Receiving,
    /// The edge it went out by, where the other PC is.
    side: ScreenSide,
    to: String,
}

impl State {
    fn new(out: UnboundedSender<Out>) -> Self {
        Self { out, armed: false, held_v: false, layout: Vec::new(), sending: None, receiving: None, parked: None }
    }

    fn emit(&self, id: &str, msg: MouseMsg) {
        let _ = self.out.send(Out::Mouse(id.to_string(), msg));
    }

    /// The V of Ctrl+V (`ctrl`: held): true when held back for files on their way.
    fn hold_paste(&mut self, down: bool, ctrl: bool) -> bool {
        if down && (self.held_v || (self.armed && ctrl)) {
            if !self.held_v {
                let _ = self.out.send(Out::Paste);
            }
            self.held_v = true;
            return true;
        }
        !down && std::mem::take(&mut self.held_v)
    }

    // ---------- this PC's own mouse and keyboard ----------

    /// True: swallowed.
    fn on_mouse(&mut self, msg: u32, info: &MSLLHOOKSTRUCT) -> bool {
        // This PC's own mouse moved while another PC drives it: it takes the pointer back.
        if self.receiving.is_some() {
            self.end_receiving(true);
        }
        if let Some(p) = self.parked.take() {
            self.emit(&p.r.id, MouseMsg::Stop);
            self.emit(&p.to, MouseMsg::Stop);
        }
        let Some(s) = &self.sending else {
            return msg == WM_MOUSEMOVE && self.at_edge(info.pt);
        };
        let id = s.id.clone();
        // The wheel's distance is the high word, signed.
        let high = (info.mouseData >> 16) as u16 as i16 as i32;
        let x_button = if high == 2 { 4 } else { 3 };
        let out = match msg {
            WM_MOUSEMOVE => {
                let (x, y) = (info.pt.x - s.park.x, info.pt.y - s.park.y);
                (x != 0 || y != 0).then(|| MouseMsg::Move { x: x as f32 / s.scale, y: y as f32 / s.scale })
            }
            WM_LBUTTONDOWN => Some(MouseMsg::Btn { b: 0, down: true }),
            WM_LBUTTONUP => Some(MouseMsg::Btn { b: 0, down: false }),
            WM_RBUTTONDOWN => Some(MouseMsg::Btn { b: 1, down: true }),
            WM_RBUTTONUP => Some(MouseMsg::Btn { b: 1, down: false }),
            WM_MBUTTONDOWN => Some(MouseMsg::Btn { b: 2, down: true }),
            WM_MBUTTONUP => Some(MouseMsg::Btn { b: 2, down: false }),
            WM_XBUTTONDOWN => Some(MouseMsg::Btn { b: x_button, down: true }),
            WM_XBUTTONUP => Some(MouseMsg::Btn { b: x_button, down: false }),
            // A precision touchpad or a free-spinning wheel scrolls in less than notches.
            WM_MOUSEWHEEL => Some(MouseMsg::Wheel { x: 0., y: high as f32, smooth: high % 120 != 0 }),
            WM_MOUSEHWHEEL => Some(MouseMsg::Wheel { x: high as f32, y: 0., smooth: high % 120 != 0 }),
            _ => None,
        };
        if let Some(out) = out {
            self.emit(&id, out);
        }
        true
    }

    /// The pointer reached an edge: over to the PC beyond it, if there is one. True: the move
    /// is swallowed (the pointer was parked).
    fn at_edge(&mut self, pt: POINT) -> bool {
        let Some(side) = edge_at(pt) else { return false };
        let Some(id) = self.layout.iter().find(|(_, s)| *s == Some(side)).map(|(id, _)| id.clone()) else { return false };
        // Not in the middle of a drag: the button would stay down here.
        let held = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2];
        if held.iter().any(|&vk| unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000 != 0) {
            return false;
        }
        let rc = monitor_rect(pt, MONITOR_DEFAULTTONEAREST).unwrap_or_else(virtual_rect);
        let park = POINT { x: (rc.left + rc.right) / 2, y: (rc.top + rc.bottom) / 2 };
        unsafe { SetCursorPos(park.x, park.y) };
        hide_pointer();
        self.emit(&id, MouseMsg::Enter { edge: side.opposite(), pos: fraction(side, pt) });
        self.sending = Some(Sending { id, side, park, scale: monitor_scale(park), exit: pt, ctrl: false, alt: false });
        true
    }

    fn on_key(&mut self, info: &KBDLLHOOKSTRUCT) -> bool {
        let down = info.flags & LLKHF_UP == 0;
        if self.sending.is_none() {
            return info.vkCode == VK_V && self.hold_paste(down, pressed(VK_CONTROL) && !pressed(VK_MENU));
        }
        let Some(s) = self.sending.as_mut() else { return false };
        let mut sc = (info.scanCode & 0xff) as u16;
        if sc == 0 {
            // Some keys (media keys of some keyboards) come without one.
            sc = unsafe { MapVirtualKeyW(info.vkCode, MAPVK_VK_TO_VSC_EX) } as u16;
        } else if info.flags & LLKHF_EXTENDED != 0 {
            sc |= 0xE000;
        }
        if SC_CTRL.contains(&sc) {
            s.ctrl = down;
        } else if SC_ALT.contains(&sc) {
            s.alt = down;
        } else if sc == SC_ESC && down && s.ctrl && s.alt {
            let id = s.id.clone();
            self.end_sending(None, true);
            self.emit(&id, MouseMsg::Stop);
            return true;
        }
        if sc != 0 {
            let id = s.id.clone();
            self.emit(&id, MouseMsg::Key { sc, down });
        }
        true
    }

    /// Back to this PC's own screen: at `at`, or where it left.
    fn end_sending(&mut self, at: Option<POINT>, show: bool) {
        let Some(s) = self.sending.take() else { return };
        let to = at.unwrap_or_else(|| inward(s.exit, s.side));
        unsafe { SetCursorPos(to.x, to.y) };
        if show {
            show_pointer();
        }
        let _ = self.out.send(Out::Arrived);
    }

    // ---------- from the other PC ----------

    fn command(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Layout(layout) => {
                self.layout = layout;
                // Gone (disconnected, or unlinked): let go of it.
                if self.sending.as_ref().is_some_and(|s| !self.layout.iter().any(|(id, side)| *id == s.id && *side == Some(s.side))) {
                    self.end_sending(None, true);
                }
                if self.receiving.as_ref().is_some_and(|r| !self.layout.iter().any(|(id, _)| *id == r.id)) {
                    self.end_receiving(false);
                }
                let connected = |id: &str| self.layout.iter().any(|(i, _)| i == id);
                if let Some(p) = self.parked.take() {
                    if !connected(&p.r.id) {
                        // The driver is gone: so is the relay.
                        self.emit(&p.to, MouseMsg::Stop);
                    } else if !connected(&p.to) {
                        // The PC it went on to is gone: the pointer comes back here.
                        self.emit(&p.to, MouseMsg::Stop);
                        self.resume(p, None);
                    } else {
                        self.parked = Some(p);
                    }
                }
            }
            Cmd::Input { from, msg } => self.input(from, msg),
            Cmd::ArmPaste(on) => self.armed = on,
            Cmd::Resume { from, pos } => {
                if let Some(p) = self.parked.take_if(|p| p.r.id == from) {
                    self.resume(p, Some(pos));
                }
            }
            Cmd::Paste => {
                // Ctrl may still be down (held on the keyboard, or by the other PC).
                let ctrl = pressed(VK_CONTROL);
                let mut events = Vec::new();
                if !ctrl {
                    events.push(vk_key(VK_LCONTROL, true));
                }
                events.extend([vk_key(VK_V as VIRTUAL_KEY, true), vk_key(VK_V as VIRTUAL_KEY, false)]);
                if !ctrl {
                    events.push(vk_key(VK_LCONTROL, false));
                }
                send(&events);
            }
        }
    }

    fn input(&mut self, from: String, msg: MouseMsg) {
        match msg {
            MouseMsg::Enter { edge, pos } => {
                self.parked = None;
                // Both mice moved at once: the other PC wins.
                if let Some(s) = &self.sending {
                    let id = s.id.clone();
                    self.end_sending(None, true);
                    self.emit(&id, MouseMsg::Stop);
                }
                self.end_receiving(false);
                let at = entry_point(edge, pos);
                move_to(at);
                self.receiving = Some(Receiving {
                    id: from,
                    edge,
                    pos: at,
                    frac: (0., 0.),
                    wheel_rem: (0., 0.),
                    keys: Vec::new(),
                    buttons: [false; 5],
                });
                let _ = self.out.send(Out::Arrived);
            }
            MouseMsg::Leave { pos } => {
                if let Some(s) = self.sending.as_ref().filter(|s| s.id == from) {
                    let at = inward(entry_point(s.side, pos), s.side);
                    self.end_sending(Some(at), true);
                }
            }
            MouseMsg::Stop => {
                if self.parked.as_ref().is_some_and(|p| p.r.id == from) {
                    self.parked = None;
                }
                if self.sending.as_ref().is_some_and(|s| s.id == from) {
                    self.end_sending(None, true);
                }
                if self.receiving.as_ref().is_some_and(|r| r.id == from) {
                    self.end_receiving(false);
                }
            }
            msg => {
                let Some(r) = self.receiving.as_mut().filter(|r| r.id == from) else { return };
                match msg {
                    MouseMsg::Move { x, y } => self.receive_move(x, y),
                    MouseMsg::Btn { b, down } if (b as usize) < r.buttons.len() => {
                        r.buttons[b as usize] = down;
                        send(&[button(b, down)]);
                    }
                    MouseMsg::Wheel { x, y, .. } => {
                        // Windows' wheel units are the protocol's (120 a notch); apps that scroll
                        // smoothly take less than a notch.
                        let (fx, fy) = (x + r.wheel_rem.0, y + r.wheel_rem.1);
                        let (wx, wy) = (fx.trunc(), fy.trunc());
                        r.wheel_rem = (fx - wx, fy - wy);
                        let mut events = Vec::new();
                        if wy != 0. {
                            events.push(mouse(0, 0, wy as i32, MOUSEEVENTF_WHEEL));
                        }
                        if wx != 0. {
                            events.push(mouse(0, 0, wx as i32, MOUSEEVENTF_HWHEEL));
                        }
                        send(&events);
                    }
                    MouseMsg::Key { sc, down } => {
                        // The other PC's Ctrl+V, held back like this PC's own.
                        // SAFETY: plain values.
                        let is_v = sc & 0xff00 == 0 && unsafe { MapVirtualKeyW(sc as u32, MAPVK_VSC_TO_VK) } == VK_V;
                        if is_v {
                            let ctrl = r.keys.iter().any(|k| SC_CTRL.contains(k));
                            if self.hold_paste(down, ctrl) {
                                return;
                            }
                        }
                        let Some(r) = self.receiving.as_mut() else { return };
                        r.keys.retain(|&k| k != sc);
                        if down {
                            r.keys.push(sc);
                        }
                        send(&[key(sc, down)]);
                    }
                    _ => {}
                }
            }
        }
    }

    fn receive_move(&mut self, dx: f32, dy: f32) {
        let Some(r) = self.receiving.as_mut() else { return };
        // Logical pixels to this monitor's physical ones, the fractions kept for the next move.
        let scale = monitor_scale(r.pos);
        let (fx, fy) = (dx * scale + r.frac.0, dy * scale + r.frac.1);
        let (px, py) = (fx.trunc(), fy.trunc());
        r.frac = (fx - px, fy - py);
        if px == 0. && py == 0. {
            return;
        }
        let target = POINT { x: r.pos.x + px as i32, y: r.pos.y + py as i32 };
        let next = if monitor_rect(target, MONITOR_DEFAULTTONULL).is_some() {
            target
        } else {
            // Off every monitor: along the edge of the one it was on; back to the driver when
            // pushed out the edge it came in through, on to a third PC through another.
            let rc = monitor_rect(r.pos, MONITOR_DEFAULTTONEAREST).unwrap_or_else(virtual_rect);
            let clamped = clamp(target, rc);
            let out = [ScreenSide::Left, ScreenSide::Right, ScreenSide::Above, ScreenSide::Below]
                .into_iter()
                .find(|&side| pushed_out(target, rc, side) && outer(clamped, rc, side));
            if out == Some(r.edge) {
                let (id, pos) = (r.id.clone(), fraction(r.edge, clamped));
                self.end_receiving(false);
                self.emit(&id, MouseMsg::Leave { pos });
                return;
            }
            let beyond = out.and_then(|side| {
                let to = self.layout.iter().find(|(id, s)| *s == Some(side) && *id != r.id)?.0.clone();
                Some((side, to))
            });
            if let Some((side, to)) = beyond {
                let (from, pos) = (r.id.clone(), fraction(side, clamped));
                r.pos = clamped;
                let mut r = self.receiving.take().expect("receiving");
                release(&mut r);
                self.parked = Some(Parked { r, side, to: to.clone() });
                let _ = self.out.send(Out::Exit { from, to, edge: side, pos });
                return;
            }
            clamped
        };
        r.pos = next;
        move_to(next);
    }

    /// The pointer relayed on came back: driven here again, on the edge it left by (at `pos`
    /// along it; where it left without).
    fn resume(&mut self, p: Parked, pos: Option<f32>) {
        let mut r = p.r;
        let at = pos.map_or(r.pos, |pos| entry_point(p.side, pos));
        r.pos = inward(at, p.side);
        r.frac = (0., 0.);
        move_to(r.pos);
        self.receiving = Some(r);
    }

    /// Lets go of the keys and buttons the other PC holds down here. `tell`: this PC's own mouse
    /// took over, so the other PC stops sending.
    fn end_receiving(&mut self, tell: bool) {
        let Some(mut r) = self.receiving.take() else { return };
        release(&mut r);
        if tell {
            self.emit(&r.id, MouseMsg::Stop);
        }
    }
}

// ---------- screens ----------

/// Lets go of the keys and buttons held down through `r`.
fn release(r: &mut Receiving) {
    let mut events: Vec<INPUT> = r.keys.drain(..).rev().map(|sc| key(sc, false)).collect();
    for (b, down) in r.buttons.iter_mut().enumerate() {
        if std::mem::take(down) {
            events.push(button(b as u8, false));
        }
    }
    send(&events);
}

/// Whether `target` is past `side` of `rc`.
fn pushed_out(target: POINT, rc: RECT, side: ScreenSide) -> bool {
    match side {
        ScreenSide::Left => target.x < rc.left,
        ScreenSide::Right => target.x >= rc.right,
        ScreenSide::Above => target.y < rc.top,
        ScreenSide::Below => target.y >= rc.bottom,
    }
}

/// Whether `side` of monitor `rc` at `pt` is an outer edge: no monitor beyond it there.
fn outer(pt: POINT, rc: RECT, side: ScreenSide) -> bool {
    let beyond = match side {
        ScreenSide::Left => POINT { x: rc.left - 1, y: pt.y },
        ScreenSide::Right => POINT { x: rc.right, y: pt.y },
        ScreenSide::Above => POINT { x: pt.x, y: rc.top - 1 },
        ScreenSide::Below => POINT { x: pt.x, y: rc.bottom },
    };
    monitor_rect(beyond, MONITOR_DEFAULTTONULL).is_none()
}

/// Physical pixels per logical one on the monitor at `pt`: its scaling (1.5 at 150 %).
fn monitor_scale(pt: POINT) -> f32 {
    let mut dpi = (96u32, 96u32);
    // SAFETY: plain values and two out-parameters.
    let found = unsafe {
        let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi.0, &mut dpi.1)
    };
    if found == 0 && dpi.0 > 0 { dpi.0 as f32 / 96. } else { 1. }
}

fn monitor_rect(pt: POINT, flags: MONITOR_FROM_FLAGS) -> Option<RECT> {
    unsafe {
        let monitor = MonitorFromPoint(pt, flags);
        if monitor.is_null() {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        (GetMonitorInfoW(monitor, &mut info) != 0).then_some(info.rcMonitor)
    }
}

/// The rectangle around every monitor.
fn virtual_rect() -> RECT {
    unsafe {
        let (left, top) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        RECT {
            left,
            top,
            right: left + GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            bottom: top + GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        }
    }
}

/// The outer edge `pt` is on: an edge of its monitor with no other monitor beyond it.
fn edge_at(pt: POINT) -> Option<ScreenSide> {
    let rc = monitor_rect(pt, MONITOR_DEFAULTTONULL)?;
    let none_at = |x: i32, y: i32| monitor_rect(POINT { x, y }, MONITOR_DEFAULTTONULL).is_none();
    if pt.x <= rc.left && none_at(rc.left - 1, pt.y) {
        Some(ScreenSide::Left)
    } else if pt.x >= rc.right - 1 && none_at(rc.right, pt.y) {
        Some(ScreenSide::Right)
    } else if pt.y <= rc.top && none_at(pt.x, rc.top - 1) {
        Some(ScreenSide::Above)
    } else if pt.y >= rc.bottom - 1 && none_at(pt.x, rc.bottom) {
        Some(ScreenSide::Below)
    } else {
        None
    }
}

/// How far along `edge` of all the screens `pt` is, 0 to 1.
fn fraction(edge: ScreenSide, pt: POINT) -> f32 {
    let v = virtual_rect();
    let f = match edge {
        ScreenSide::Left | ScreenSide::Right => (pt.y - v.top) as f32 / (v.bottom - v.top - 1).max(1) as f32,
        ScreenSide::Above | ScreenSide::Below => (pt.x - v.left) as f32 / (v.right - v.left - 1).max(1) as f32,
    };
    f.clamp(0.0, 1.0)
}

/// The point on `edge` of this PC's screens at `pos` along it (0 to 1).
fn entry_point(edge: ScreenSide, pos: f32) -> POINT {
    let v = virtual_rect();
    let pos = if pos.is_finite() { pos.clamp(0.0, 1.0) } else { 0.5 };
    let along = |a: i32, b: i32| a + ((b - a - 1) as f32 * pos).round() as i32;
    let pt = match edge {
        ScreenSide::Left => POINT { x: v.left, y: along(v.top, v.bottom) },
        ScreenSide::Right => POINT { x: v.right - 1, y: along(v.top, v.bottom) },
        ScreenSide::Above => POINT { x: along(v.left, v.right), y: v.top },
        ScreenSide::Below => POINT { x: along(v.left, v.right), y: v.bottom - 1 },
    };
    // Monitors of different sizes leave parts of that rectangle without a monitor: onto the
    // nearest one, on its edge.
    let rc = monitor_rect(pt, MONITOR_DEFAULTTONEAREST).unwrap_or(v);
    let mut pt = clamp(pt, rc);
    match edge {
        ScreenSide::Left => pt.x = rc.left,
        ScreenSide::Right => pt.x = rc.right - 1,
        ScreenSide::Above => pt.y = rc.top,
        ScreenSide::Below => pt.y = rc.bottom - 1,
    }
    pt
}

/// A little in from `side`, so the pointer coming back doesn't go straight over again.
fn inward(pt: POINT, side: ScreenSide) -> POINT {
    const STEP: i32 = 2;
    match side {
        ScreenSide::Left => POINT { x: pt.x + STEP, y: pt.y },
        ScreenSide::Right => POINT { x: pt.x - STEP, y: pt.y },
        ScreenSide::Above => POINT { x: pt.x, y: pt.y + STEP },
        ScreenSide::Below => POINT { x: pt.x, y: pt.y - STEP },
    }
}

fn clamp(pt: POINT, rc: RECT) -> POINT {
    POINT { x: pt.x.clamp(rc.left, rc.right - 1), y: pt.y.clamp(rc.top, rc.bottom - 1) }
}

// ---------- the pointer ----------

/// The system cursors Windows shows, all swapped for a blank one while driving another PC.
const CURSORS: [SYSTEM_CURSOR_ID; 13] = [
    OCR_NORMAL,
    OCR_IBEAM,
    OCR_WAIT,
    OCR_CROSS,
    OCR_UP,
    OCR_SIZENWSE,
    OCR_SIZENESW,
    OCR_SIZEWE,
    OCR_SIZENS,
    OCR_SIZEALL,
    OCR_NO,
    OCR_HAND,
    OCR_APPSTARTING,
];

fn hide_pointer() {
    // 32×32, every pixel transparent (AND mask all ones, XOR mask zero).
    let and = [0xffu8; 32 * 32 / 8];
    let xor = [0u8; 32 * 32 / 8];
    for id in CURSORS {
        unsafe {
            // SetSystemCursor takes the cursor over (and destroys it): a new one each time.
            let blank = CreateCursor(GetModuleHandleW(std::ptr::null()), 0, 0, 32, 32, and.as_ptr().cast(), xor.as_ptr().cast());
            if !blank.is_null() {
                SetSystemCursor(blank, id);
            }
        }
    }
}

/// The user's cursors again, from their settings.
fn show_pointer() {
    unsafe { SystemParametersInfoW(SPI_SETCURSORS, 0, std::ptr::null_mut(), 0) };
}

// ---------- SendInput ----------

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        tracing::debug!("shared mouse: Windows blocked the input (an administrator window in front?)");
    }
}

/// Puts the pointer at `pt`, in the 0–65535 range SendInput's absolute moves use over all the
/// monitors (rounded up, so it maps back to the same pixel).
fn move_to(pt: POINT) {
    let v = virtual_rect();
    let scale = |p: i32, lo: i32, hi: i32| {
        let size = (hi - lo).max(1) as i64;
        (((p - lo) as i64 * 65536 + size - 1) / size) as i32
    };
    let (x, y) = (scale(pt.x, v.left, v.right), scale(pt.y, v.top, v.bottom));
    send(&[mouse(x, y, 0, MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK)]);
}

fn mouse(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn button(b: u8, down: bool) -> INPUT {
    let (flags, data) = match (b, down) {
        (0, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (0, false) => (MOUSEEVENTF_LEFTUP, 0),
        (1, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (1, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (2, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (2, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (b, true) => (MOUSEEVENTF_XDOWN, if b == 4 { 2 } else { 1 }),
        (b, false) => (MOUSEEVENTF_XUP, if b == 4 { 2 } else { 1 }),
    };
    mouse(0, 0, data, flags)
}

/// The virtual key of V (Ctrl+V pastes by it, wherever the layout puts it).
const VK_V: u32 = 0x56;

fn pressed(vk: VIRTUAL_KEY) -> bool {
    // SAFETY: plain values.
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// A key by its virtual key.
fn vk_key(vk: VIRTUAL_KEY, down: bool) -> INPUT {
    // SAFETY: plain values.
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: if down { 0 } else { KEYEVENTF_KEYUP }, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// A key by its scan code, so this PC's keyboard layout decides what it types.
fn key(sc: u16, down: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if sc & 0xff00 == 0xe000 {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if !down {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: 0, wScan: sc & 0xff, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(from: &str, msg: MouseMsg) -> Cmd {
        Cmd::Input { from: from.into(), msg }
    }

    #[test]
    fn piled_up_motion_and_scrolling_merge() {
        let merged = merge(vec![
            input("a", MouseMsg::Move { x: 1., y: 2.5 }),
            input("a", MouseMsg::Move { x: 3., y: -1. }),
            input("a", MouseMsg::Wheel { x: 0., y: 6., smooth: true }),
            input("a", MouseMsg::Wheel { x: 0., y: 9., smooth: true }),
            input("a", MouseMsg::Wheel { x: 0., y: 120., smooth: false }),
            input("a", MouseMsg::Btn { b: 0, down: true }),
            input("a", MouseMsg::Move { x: 1., y: 0. }),
            input("b", MouseMsg::Move { x: 5., y: 5. }),
        ]);
        let msgs: Vec<(String, MouseMsg)> = merged
            .into_iter()
            .map(|c| match c {
                Cmd::Input { from, msg } => (from, msg),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            msgs,
            [
                ("a".into(), MouseMsg::Move { x: 4., y: 1.5 }),
                ("a".into(), MouseMsg::Wheel { x: 0., y: 15., smooth: true }),
                ("a".into(), MouseMsg::Wheel { x: 0., y: 120., smooth: false }),
                ("a".into(), MouseMsg::Btn { b: 0, down: true }),
                ("a".into(), MouseMsg::Move { x: 1., y: 0. }),
                ("b".into(), MouseMsg::Move { x: 5., y: 5. }),
            ]
        );
    }
}
