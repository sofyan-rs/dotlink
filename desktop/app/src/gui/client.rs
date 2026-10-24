//! The window's line to the service, over the control socket, the same way the CLI talks to it.
//! This works whether the service runs inside this app or as a separate `dotlink daemon`.
//! Socket I/O runs on the tokio runtime; results come back as plain futures GPUI can await.

use anyhow::{Result, anyhow, bail};
use dotlink_core::cast::CastState;
use dotlink_core::config::Config;
use dotlink_core::events::{Event, Transfer};
use dotlink_core::ipc;
use futures::channel::{mpsc, oneshot};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct Status {
    pub version: String,
    /// What the phones call this PC (the configured name, or the hostname).
    pub name: String,
    pub port: u16,
    pub fingerprint: String,
    pub addresses: Vec<String>,
    pub devices: Vec<DeviceInfo>,
    pub transfers: Vec<Transfer>,
    pub config: Config,
    /// Phone streaming the camera / mic right now.
    pub camera: Option<String>,
    pub mic: Option<String>,
    /// Device id → its cast, for the phones that aren't Idle.
    #[serde(default)]
    pub casts: HashMap<String, CastState>,
}

impl Status {
    pub fn cast(&self, id: &str) -> CastState {
        self.casts.get(id).cloned().unwrap_or(CastState::Idle)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub paired_at: u64,
    /// Connected through the USB cable rather than Wi-Fi.
    #[serde(default)]
    pub usb: bool,
}

/// What the watch connection reports to the window.
pub enum Update {
    /// (Re)connected to the service.
    Connected,
    /// The service went away; retrying.
    Disconnected,
    Event(Event),
}

/// Progress of a pairing request.
pub enum PairUpdate {
    Uri(String),
    Paired(String),
    Failed(String),
}

#[derive(Clone)]
pub struct Client {
    rt: tokio::runtime::Handle,
}

impl Client {
    pub fn new(rt: tokio::runtime::Handle) -> Self {
        Self { rt }
    }

    /// Runs `fut` on the tokio runtime, detached.
    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) {
        self.rt.spawn(fut);
    }

    /// Runs `fut` on the tokio runtime; the returned future can be awaited anywhere.
    fn on_runtime<T, F>(&self, fut: F) -> impl Future<Output = Result<T>> + use<T, F>
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.rt.spawn(async move {
            let _ = tx.send(fut.await);
        });
        async move { rx.await.map_err(|_| anyhow!("Dotlink stopped"))? }
    }

    /// One request; an `error` reply becomes Err.
    pub fn call(&self, req: Value) -> impl Future<Output = Result<Value>> + use<> {
        self.on_runtime(async move {
            let v = ipc::call(req).await?;
            if let Some(e) = v["error"].as_str() {
                bail!("{e}");
            }
            Ok(v)
        })
    }

    pub fn status(&self) -> impl Future<Output = Result<Status>> + use<> {
        let call = self.call(json!({ "cmd": "status" }));
        async move { Ok(serde_json::from_value(call.await?)?) }
    }

    /// `to`: a device id, or None for every connected phone.
    pub fn send_files(&self, paths: Vec<PathBuf>, to: Option<String>) -> impl Future<Output = Result<String>> + use<> {
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        ok_text(self.call(json!({ "cmd": "send", "paths": paths, "to": to })))
    }

    pub fn send_clipboard(&self, to: Option<String>) -> impl Future<Output = Result<String>> + use<> {
        ok_text(self.call(json!({ "cmd": "clip", "to": to })))
    }

    /// Asks the phone to start or stop the camera ("camera") or mic ("mic").
    /// `to`: the device id to start it on (stopping goes to every phone).
    pub fn media(&self, kind: &str, on: bool, to: Option<String>) -> impl Future<Output = Result<String>> + use<> {
        ok_text(self.call(json!({ "cmd": "media", "kind": kind, "on": on, "to": to })))
    }

    /// Starts or stops casting a phone's screen (by device id; None: the first phone online, or
    /// every cast for stop).
    pub fn cast(&self, on: bool, id: Option<&str>) -> impl Future<Output = Result<String>> + use<> {
        ok_text(self.call(json!({ "cmd": "cast", "on": on, "id": id })))
    }

    pub fn cast_pair(&self, id: &str, code: &str) -> impl Future<Output = Result<String>> + use<> {
        ok_text(self.call(json!({ "cmd": "cast_pair", "id": id, "code": code })))
    }

    /// Opens the pairing link `uri` on the phones plugged in over USB.
    pub fn pair_usb(&self, uri: &str) -> impl Future<Output = Result<String>> + use<> {
        ok_text(self.call(json!({ "cmd": "pair_usb", "uri": uri })))
    }

    pub fn unpair(&self, id: &str) -> impl Future<Output = Result<String>> + use<> {
        ok_text(self.call(json!({ "cmd": "unpair", "id": id })))
    }

    /// Saves the config; Ok(true) when part of it applies after a restart.
    pub fn set_config(&self, config: &Config) -> impl Future<Output = Result<bool>> + use<> {
        let call = self.call(json!({ "cmd": "set_config", "config": config }));
        async move { Ok(call.await?["restart"].as_bool().unwrap_or(false)) }
    }

    pub fn quit_service(&self) -> impl Future<Output = Result<Value>> + use<> {
        self.call(json!({ "cmd": "quit" }))
    }

    /// Starts pairing. The receiver yields the URI, then the outcome. Dropping it doesn't
    /// cancel the pairing window on the service (it expires after 5 minutes).
    pub fn pair(&self) -> mpsc::UnboundedReceiver<PairUpdate> {
        let (tx, rx) = mpsc::unbounded();
        self.rt.spawn(async move {
            let result = ipc::request(json!({ "cmd": "pair" }), |v| {
                let update = if let Some(uri) = v["uri"].as_str() {
                    PairUpdate::Uri(uri.into())
                } else if let Some(name) = v["paired"].as_str() {
                    PairUpdate::Paired(name.into())
                } else {
                    PairUpdate::Failed(v["error"].as_str().unwrap_or("pairing failed").into())
                };
                let _ = tx.unbounded_send(update);
                Ok(())
            })
            .await;
            if let Err(e) = result {
                let _ = tx.unbounded_send(PairUpdate::Failed(format!("{e:#}")));
            }
        });
        rx
    }

    /// Follows the service's events, reconnecting when it restarts.
    pub fn watch(&self) -> mpsc::UnboundedReceiver<Update> {
        let (tx, rx) = mpsc::unbounded();
        self.rt.spawn(async move {
            let mut connected = false;
            while !tx.is_closed() {
                if ipc::running().await {
                    if !connected {
                        connected = true;
                        let _ = tx.unbounded_send(Update::Connected);
                    }
                    // Returns when the service goes away.
                    let _ = ipc::request(json!({ "cmd": "watch" }), |v| {
                        if let Ok(ev) = serde_json::from_value::<Event>(v) {
                            let _ = tx.unbounded_send(Update::Event(ev));
                        }
                        Ok(())
                    })
                    .await;
                }
                if connected && !ipc::running().await {
                    connected = false;
                    let _ = tx.unbounded_send(Update::Disconnected);
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        rx
    }
}

async fn ok_text(call: impl Future<Output = Result<Value>>) -> Result<String> {
    Ok(call.await?["ok"].as_str().unwrap_or("done").to_string())
}
