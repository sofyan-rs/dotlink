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
//! | `mouse_move` | driver | pointer motion in the driver's pixels |
//! | `mouse_btn` | driver | button `b` (0 left, 1 right, 2 middle, 3 back, 4 forward) down or up |
//! | `mouse_wheel` | driver | wheel, 120 per notch; positive `y` away from the user, positive `x` right |
//! | `mouse_key` | driver | key `sc` down or up: a PC/AT set 1 scan code, `0xE0xx` for the extended ones |
//! | `mouse_leave` | receiver | the pointer went back out the edge it came in, at `pos` |
//! | `mouse_stop` | either | that PC's own mouse moved (receiver), or Ctrl+Alt+Esc (driver): it's over |
//! | `mouse_layout` | either | the user placed this PC on `side` of the sender's screen (None: not shared) |
//!
//! Scan codes rather than key names, so the receiver's own keyboard layout applies, like a
//! keyboard plugged into it.

use crate::config::{DeviceKind, ScreenSide};
use crate::events::Event;
use crate::platform;
use crate::server::App;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum MouseMsg {
    #[serde(rename = "mouse_enter")]
    Enter { edge: ScreenSide, pos: f32 },
    #[serde(rename = "mouse_move")]
    Move { x: i32, y: i32 },
    #[serde(rename = "mouse_btn")]
    Btn { b: u8, down: bool },
    #[serde(rename = "mouse_wheel")]
    Wheel { x: i32, y: i32 },
    #[serde(rename = "mouse_key")]
    Key { sc: u16, down: bool },
    #[serde(rename = "mouse_leave")]
    Leave { pos: f32 },
    #[serde(rename = "mouse_stop")]
    Stop,
}

/// What the platform part tells.
#[derive(Debug)]
pub enum Out {
    /// For that PC.
    Mouse(String, MouseMsg),
    /// Ctrl+V / ⌘V while armed ([Cmd::ArmPaste]): held back until the files are here.
    Paste,
    /// The pointer came to this PC: another PC brought it, or it came back.
    Arrived,
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
}

/// The platform part, once started.
#[derive(Default)]
pub struct SharedMouse {
    inner: Mutex<Option<platform::shared_mouse::SharedMouse>>,
    /// The linked PCs whose Dotlink said it takes part (`mouse` in their `hello` / `info`):
    /// only those get the pointer, else it would vanish into a PC that ignores it.
    capable: Mutex<HashSet<String>>,
}

impl SharedMouse {
    /// Whether it runs on this PC.
    pub fn running(&self) -> bool {
        self.inner.lock().unwrap().is_some()
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
        };
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
        }
        let Ok(text) = serde_json::to_string(&msg) else { continue };
        if !app.send_to_pc(&id, text) {
            tracing::debug!("shared mouse: {id} isn't connected");
        }
    }
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
    let pcs: Vec<(String, Option<ScreenSide>, bool)> = {
        let state = app.state.lock().unwrap();
        state
            .peers
            .iter()
            .map(|p| (p.id.clone(), p.screen, true))
            .chain(state.devices.iter().filter(|d| d.kind == DeviceKind::Pc).map(|d| (d.id.clone(), d.screen, false)))
            .collect()
    };
    let layout = pcs
        .into_iter()
        .filter(|(id, _, joined)| if *joined { app.peers.online(id) } else { app.pc_connected(id) })
        .filter(|(id, _, _)| app.mouse.capable(id))
        .map(|(id, side, _)| (id, side))
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
        Ok(msg) => app.mouse.send(Cmd::Input { from: from.to_string(), msg }),
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
