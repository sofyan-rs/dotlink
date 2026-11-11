//! Shared mouse: one mouse and keyboard for this PC and the PCs linked to it. Each linked PC can
//! be placed next to this one's screen ([ScreenSide]); pushing the pointer against that edge
//! sends it over, with the keyboard, until it comes back through the same edge.
//!
//! Every PC both sends and receives: the one whose own mouse moved drives the other. The
//! platform part (`platform::shared_mouse`) watches the local mouse and keyboard, swallows them
//! while they drive another PC, and plays what another PC sends. This module carries its
//! messages over the PC link and keeps it told which PCs are connected and where.
//!
//! The messages, on the control connection between the two PCs ([MouseMsg]):
//!
//! | `t` | from | what |
//! |---|---|---|
//! | `mouse_enter` | driver | the pointer comes in through `edge` of the receiver's screens, at `pos` (0 to 1 along it) |
//! | `mouse_move` | driver | pointer motion in logical pixels (a Mac's points, a Windows PC's pixels at 100 %), fractions kept |
//! | `mouse_btn` | driver | button `b` (0 left, 1 right, 2 middle, 3 back, 4 forward) down or up |
//! | `mouse_wheel` | driver | scrolling, 120 per wheel notch ([WHEEL_PIXELS] logical pixels); positive `y` away from the user, positive `x` right; `smooth`: from a trackpad (or a precision wheel) rather than notches |
//! | `mouse_key` | driver | key `sc` down or up: a PC/AT set 1 scan code, `0xE0xx` for the extended ones |
//! | `mouse_leave` | receiver | the pointer went back out the edge it came in, at `pos` |
//! | `mouse_stop` | either | that PC's own mouse moved (receiver), or Ctrl+Alt+Esc (driver): it's over |
//! | `mouse_layout` | either | the user placed this PC on `side` of the sender's screen (None: not shared) |
//!
//! Scan codes rather than key names, so the receiver's own keyboard layout applies, like a
//! keyboard plugged into it.
//!
//! More than two PCs: each PC only knows its neighbours. When the pointer a PC is driven to
//! goes out another edge, one with a third PC beyond it, this PC relays: it sends that PC
//! `mouse_enter` and passes on everything the driver sends, until the third PC sends
//! `mouse_leave` (back here) or `mouse_stop`. A chain of PCs relays hop by hop.

use crate::config::{DeviceKind, ScreenSide};
use crate::events::Event;
use crate::platform;
use crate::server::App;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum MouseMsg {
    #[serde(rename = "mouse_enter")]
    Enter { edge: ScreenSide, pos: f32 },
    #[serde(rename = "mouse_move")]
    Move { x: f32, y: f32 },
    #[serde(rename = "mouse_btn")]
    Btn { b: u8, down: bool },
    #[serde(rename = "mouse_wheel")]
    Wheel {
        x: f32,
        y: f32,
        #[serde(default)]
        smooth: bool,
    },
    #[serde(rename = "mouse_key")]
    Key { sc: u16, down: bool },
    #[serde(rename = "mouse_leave")]
    Leave { pos: f32 },
    #[serde(rename = "mouse_stop")]
    Stop,
}

/// Logical pixels one wheel notch (120) scrolls, as Chrome and Edge scroll it: a trackpad's
/// pixels turn into wheel units and back by this, so content follows the fingers.
pub const WHEEL_PIXELS: f32 = 100.;

/// What the platform part tells.
#[derive(Debug)]
pub enum Out {
    /// For that PC.
    Mouse(String, MouseMsg),
    /// Ctrl+V / ⌘V while armed ([Cmd::ArmPaste]): held back until the files are here.
    Paste,
    /// The pointer came to this PC: another PC brought it, or it came back.
    Arrived,
    /// The pointer `from` drives here went out `edge` at `pos`, with PC `to` beyond it: this PC
    /// relays to it from now on.
    Exit { from: String, to: String, edge: ScreenSide, pos: f32 },
}

/// What the platform part is told.
#[derive(Debug)]
pub enum Cmd {
    /// The linked PCs connected right now, with where their screen is (None: not placed).
    Layout(Vec<(String, Option<ScreenSide>)>),
    /// A message from that PC.
    Input { from: String, msg: MouseMsg },
    /// Hold Ctrl+V / ⌘V back (files copied on another PC are on their way, `clip_files.rs`).
    ArmPaste(bool),
    /// Press Ctrl+V / ⌘V: the files held back for are here.
    Paste,
    /// The pointer relayed on ([Out::Exit]) came back, at `pos` along the edge it left by;
    /// `from` drives it here again.
    Resume { from: String, pos: f32 },
}

/// The platform part, once started.
#[derive(Default)]
pub struct SharedMouse {
    inner: Mutex<Option<platform::shared_mouse::SharedMouse>>,
    /// The linked PCs whose Dotlink said it takes part (`mouse` in their `hello` / `info`):
    /// only those get the pointer, else it would vanish into a PC that ignores it.
    capable: Mutex<HashSet<String>>,
    /// Linked PC id → how much faster the pointer goes there than here (its Pointer speed).
    speeds: Mutex<HashMap<String, f32>>,
    /// Relaying: the PC driving the pointer → the PC it went on to from here.
    relays: Mutex<HashMap<String, String>>,
}

impl SharedMouse {
    /// Whether it runs on this PC.
    pub fn running(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    /// The Pointer speed for a linked PC (by id).
    fn speed(&self, id: &str) -> f32 {
        self.speeds.lock().unwrap().get(id).copied().unwrap_or(1.)
    }

    /// Whether a linked PC (by id) takes part, as it said when it last connected.
    pub fn capable(&self, id: &str) -> bool {
        self.capable.lock().unwrap().contains(id)
    }

    pub fn arm_paste(&self, on: bool) {
        self.send(Cmd::ArmPaste(on));
    }

    pub fn paste(&self) {
        self.send(Cmd::Paste);
    }

    fn send(&self, cmd: Cmd) {
        if let Some(inner) = self.inner.lock().unwrap().as_ref() {
            inner.send(cmd);
        }
    }
}

/// Starts watching the local mouse and keyboard (service start). Where the OS isn't supported
/// yet, this PC neither sends nor receives.
pub fn start(app: &Arc<App>) {
    let (tx, rx) = mpsc::unbounded_channel();
    match platform::shared_mouse::SharedMouse::start(tx) {
        Ok(inner) => *app.mouse.inner.lock().unwrap() = Some(inner),
        Err(e) => {
            tracing::info!("shared mouse: {e:#}");
            return;
        }
    }
    refresh(app);
    tokio::spawn(forward(app.clone(), rx));
    // Which PCs are connected follows the device list.
    let (app, mut events) = (app.clone(), app.events.subscribe());
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(Event::Devices) | Err(broadcast::error::RecvError::Lagged(_)) => refresh(&app),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

/// Lets go of the mouse and keyboard (service stop): a hidden pointer shows again, keys held
/// down by another PC come up.
pub fn stop(app: &App) {
    if let Some(inner) = app.mouse.inner.lock().unwrap().take() {
        inner.stop();
    }
}

/// Sends what the platform part has for the other PCs. Pointer motion that piled up (a slow
/// link) goes as one move.
async fn forward(app: Arc<App>, mut rx: mpsc::UnboundedReceiver<Out>) {
    let mut next = None;
    loop {
        let out = match next.take() {
            Some(out) => out,
            None => match rx.recv().await {
                Some(out) => out,
                None => return,
            },
        };
        let (id, mut msg) = match out {
            Out::Mouse(id, msg) => (id, msg),
            Out::Paste => {
                crate::clip_files::paste_pressed(&app);
                continue;
            }
            Out::Arrived => {
                crate::clip_files::arrived(&app);
                continue;
            }
            Out::Exit { from, to, edge, pos } => {
                app.mouse.relays.lock().unwrap().insert(from, to.clone());
                send_msg(&app, &to, &MouseMsg::Enter { edge: edge.opposite(), pos });
                continue;
            }
        };
        if msg == MouseMsg::Stop {
            // This PC's own mouse took over (or Ctrl+Alt+Esc): no more relaying either way.
            app.mouse.relays.lock().unwrap().retain(|driver, to| *driver != id && *to != id);
        }
        // Scroll steps that piled up (a slow link) go as one; the receiving PC evens out their
        // pace (its smoother), so they go as they come rather than in batches.
        if let MouseMsg::Wheel { x, y, smooth } = &mut msg {
            while let Ok(out) = rx.try_recv() {
                match out {
                    Out::Mouse(other, MouseMsg::Wheel { x: dx, y: dy, smooth: s }) if other == id && s == *smooth => {
                        *x += dx;
                        *y += dy;
                    }
                    out => {
                        next = Some(out);
                        break;
                    }
                }
            }
        }
        if let MouseMsg::Move { x, y } = &mut msg {
            while let Ok(out) = rx.try_recv() {
                match out {
                    Out::Mouse(other, MouseMsg::Move { x: dx, y: dy }) if other == id => {
                        *x += dx;
                        *y += dy;
                    }
                    out => {
                        next = Some(out);
                        break;
                    }
                }
            }
            let speed = app.mouse.speed(&id);
            *x *= speed;
            *y *= speed;
        }
        send_msg(&app, &id, &msg);
    }
}

fn send_msg(app: &App, id: &str, msg: &MouseMsg) {
    let Ok(text) = serde_json::to_string(msg) else { return };
    if !app.send_to_pc(id, text) {
        tracing::debug!("shared mouse: {id} isn't connected");
    }
}

/// A message from a linked PC: passed on when this PC relays it, else played here.
fn route(app: &App, from: &str, msg: MouseMsg) {
    let relay_to = app.mouse.relays.lock().unwrap().get(from).cloned();
    if let Some(to) = relay_to {
        // From the PC driving the pointer, which is past this PC now.
        if msg == MouseMsg::Stop {
            app.mouse.relays.lock().unwrap().remove(from);
            send_msg(app, &to, &MouseMsg::Stop);
            app.mouse.send(Cmd::Input { from: from.to_string(), msg });
        } else {
            send_msg(app, &to, &msg);
        }
        return;
    }
    let driver = app.mouse.relays.lock().unwrap().iter().find(|(_, to)| *to == from).map(|(driver, _)| driver.clone());
    if let Some(driver) = driver {
        // From the PC this one relays to.
        match msg {
            MouseMsg::Leave { pos } => {
                app.mouse.relays.lock().unwrap().remove(&driver);
                app.mouse.send(Cmd::Resume { from: driver, pos });
                return;
            }
            MouseMsg::Stop | MouseMsg::Enter { .. } => {
                // Its own mouse took the pointer: the driver stops, and so does this PC.
                app.mouse.relays.lock().unwrap().remove(&driver);
                send_msg(app, &driver, &MouseMsg::Stop);
                app.mouse.send(Cmd::Input { from: driver, msg: MouseMsg::Stop });
                if msg == MouseMsg::Stop {
                    return;
                }
            }
            _ => {}
        }
    }
    app.mouse.send(Cmd::Input { from: from.to_string(), msg });
}

/// A linked PC said whether it takes part (when connecting).
pub fn set_capable(app: &App, id: &str, on: bool) {
    let changed = {
        let mut capable = app.mouse.capable.lock().unwrap();
        if on { capable.insert(id.to_string()) } else { capable.remove(id) }
    };
    if changed {
        app.emit(Event::Devices);
    }
}

/// Tells the platform part which linked PCs are connected and where they are.
pub fn refresh(app: &App) {
    let pcs: Vec<(String, Option<ScreenSide>, bool, Option<f32>)> = {
        let state = app.state.lock().unwrap();
        state
            .peers
            .iter()
            .map(|p| (p.id.clone(), p.screen, true, p.pointer_speed))
            .chain(
                state
                    .devices
                    .iter()
                    .filter(|d| d.kind == DeviceKind::Pc)
                    .map(|d| (d.id.clone(), d.screen, false, d.pointer_speed)),
            )
            .collect()
    };
    *app.mouse.speeds.lock().unwrap() =
        pcs.iter().filter_map(|(id, _, _, speed)| Some((id.clone(), (*speed)?))).collect();
    let layout = pcs
        .into_iter()
        .filter(|(id, _, joined, _)| if *joined { app.peers.online(id) } else { app.pc_connected(id) })
        .filter(|(id, _, _, _)| app.mouse.capable(id))
        .map(|(id, side, _, _)| (id, side))
        .collect();
    app.mouse.send(Cmd::Layout(layout));
}

/// A control message from a linked PC: true when it was for the shared mouse.
pub fn handle(app: &App, from: &str, v: &Value) -> bool {
    let Some(t) = v["t"].as_str().filter(|t| t.starts_with("mouse_")) else { return false };
    if t == "mouse_layout" {
        let side = v["side"].as_str().and_then(ScreenSide::parse);
        if let Err(e) = place(app, from, side) {
            tracing::warn!("shared mouse: {e:#}");
        }
        return true;
    }
    match serde_json::from_value::<MouseMsg>(v.clone()) {
        Ok(msg) => route(app, from, msg),
        Err(e) => tracing::debug!("shared mouse: {t}: {e}"),
    }
    true
}

/// Places a linked PC (`key`: its name or id) on `side` of this PC's screen, or stops sharing
/// with it (None), and tells it so it places this PC on the opposite side. Returns its name.
pub fn set_screen(app: &App, key: &str, side: Option<ScreenSide>) -> Result<String> {
    let (id, name) = {
        let state = app.state.lock().unwrap();
        let found = state
            .peers
            .iter()
            .map(|p| (&p.id, &p.name))
            .chain(state.devices.iter().filter(|d| d.kind == DeviceKind::Pc).map(|d| (&d.id, &d.name)))
            .find(|(id, name)| *id == key || name.eq_ignore_ascii_case(key));
        let Some((id, name)) = found else { bail!("no linked PC \"{key}\"") };
        (id.clone(), name.clone())
    };
    place(app, &id, side)?;
    let msg = json!({ "t": "mouse_layout", "side": side.map(|s| s.opposite().as_str()) });
    app.send_to_pc(&id, msg.to_string());
    Ok(name)
}

/// How fast the pointer goes on a linked PC (`key`: its name or id), compared with this PC's
/// own: 1 is the same speed. Returns its name.
pub fn set_pointer_speed(app: &App, key: &str, speed: f32) -> Result<String> {
    if !(0.25..=4.).contains(&speed) {
        bail!("pointer speed goes from 0.25 to 4");
    }
    let speed = (speed != 1.).then_some(speed);
    let mut state = app.state.lock().unwrap();
    let name = if let Some(p) = state.peers.iter_mut().find(|p| p.id == key || p.name.eq_ignore_ascii_case(key)) {
        p.pointer_speed = speed;
        p.name.clone()
    } else if let Some(d) =
        state.devices.iter_mut().find(|d| d.kind == DeviceKind::Pc && (d.id == key || d.name.eq_ignore_ascii_case(key)))
    {
        d.pointer_speed = speed;
        d.name.clone()
    } else {
        bail!("no linked PC \"{key}\"");
    };
    state.save()?;
    drop(state);
    app.emit(Event::Devices);
    Ok(name)
}

/// Saves where a linked PC is (by id).
fn place(app: &App, id: &str, side: Option<ScreenSide>) -> Result<()> {
    let mut state = app.state.lock().unwrap();
    if let Some(p) = state.peers.iter_mut().find(|p| p.id == id) {
        p.screen = side;
    } else if let Some(d) = state.devices.iter_mut().find(|d| d.id == id && d.kind == DeviceKind::Pc) {
        d.screen = side;
    } else {
        bail!("no linked PC with id {id}");
    }
    state.save()?;
    drop(state);
    app.emit(Event::Devices);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip() {
        let msg = MouseMsg::Enter { edge: ScreenSide::Left, pos: 0.5 };
        let text = serde_json::to_string(&msg).unwrap();
        assert_eq!(text, r#"{"t":"mouse_enter","edge":"left","pos":0.5}"#);
        assert_eq!(serde_json::from_str::<MouseMsg>(&text).unwrap(), msg);
        let stop: MouseMsg = serde_json::from_str(r#"{"t":"mouse_stop"}"#).unwrap();
        assert_eq!(stop, MouseMsg::Stop);
        let key: MouseMsg = serde_json::from_str(r#"{"t":"mouse_key","sc":57373,"down":true}"#).unwrap();
        assert_eq!(key, MouseMsg::Key { sc: 0xE01D, down: true });
    }
}
