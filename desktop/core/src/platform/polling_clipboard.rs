//! Clipboard text through `arboard`, with a poll twice a second to notice changes.
//!
//! One thread owns the clipboard handle: on X11 pasted data is served by the owning process,
//! so the handle must live as long as the app. Works the same on Windows and macOS, where
//! arboard uses the native clipboard.

use crate::server::App;
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

// The clipboard API every platform module provides, for platforms where polling is all there is.

/// Pushes text copied on this PC to the phone.
pub async fn watch(app: Arc<App>) {
    let mut changes = shared().subscribe();
    while let Some(text) = changes.recv().await {
        app.clip_from_desktop(text);
    }
}

pub async fn get() -> Option<String> {
    tokio::task::spawn_blocking(|| shared().get()).await.ok().flatten()
}

pub async fn set(text: &str) -> anyhow::Result<()> {
    let text = text.to_string();
    tokio::task::spawn_blocking(move || shared().set(text)).await?
}

enum Cmd {
    Get(mpsc::Sender<Option<String>>),
    Set(String, mpsc::Sender<anyhow::Result<()>>),
}

pub struct PollingClipboard {
    cmds: mpsc::Sender<Cmd>,
    subscribers: std::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>,
}

static SHARED: OnceLock<PollingClipboard> = OnceLock::new();

pub fn shared() -> &'static PollingClipboard {
    SHARED.get_or_init(|| {
        let (cmds, rx) = mpsc::channel::<Cmd>();
        std::thread::Builder::new()
            .name("dotlink-clipboard".into())
            .spawn(move || run(rx))
            .expect("spawn clipboard thread");
        PollingClipboard { cmds, subscribers: std::sync::Mutex::new(Vec::new()) }
    })
}

impl PollingClipboard {
    /// Text copied by other apps from now on (not our own `set`).
    pub fn subscribe(&self) -> tokio::sync::mpsc::UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.subscribers.lock().unwrap().push(tx);
        rx
    }

    pub fn get(&self) -> Option<String> {
        let (tx, rx) = mpsc::channel();
        self.cmds.send(Cmd::Get(tx)).ok()?;
        rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
    }

    pub fn set(&self, text: String) -> anyhow::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.cmds.send(Cmd::Set(text, tx))?;
        rx.recv_timeout(Duration::from_secs(2))?
    }
}

fn run(cmds: mpsc::Receiver<Cmd>) {
    let mut clipboard = None;
    let mut last: Option<String> = None;
    loop {
        if clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(c) => clipboard = Some(c),
                Err(e) => {
                    tracing::debug!("clipboard not ready: {e}");
                    // Still answer requests while waiting for the display.
                    if let Ok(cmd) = cmds.recv_timeout(Duration::from_secs(2)) {
                        match cmd {
                            Cmd::Get(reply) => drop(reply.send(None)),
                            Cmd::Set(_, reply) => drop(reply.send(Err(anyhow::anyhow!("clipboard unavailable: {e}")))),
                        }
                    }
                    continue;
                }
            }
        }
        let cb = clipboard.as_mut().unwrap();
        match cmds.recv_timeout(Duration::from_millis(500)) {
            Ok(Cmd::Get(reply)) => drop(reply.send(cb.get_text().ok())),
            Ok(Cmd::Set(text, reply)) => {
                let result = cb.set_text(text.clone()).map_err(anyhow::Error::from);
                if result.is_ok() {
                    last = Some(text); // our own write is not a change to report
                }
                drop(reply.send(result));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Poll: other apps changed the clipboard?
                if let Ok(text) = cb.get_text() {
                    if last.as_deref() != Some(text.as_str()) {
                        let first = last.is_none();
                        last = Some(text.clone());
                        if !first {
                            if let Some(me) = SHARED.get() {
                                me.subscribers.lock().unwrap().retain(|s| s.send(text.clone()).is_ok());
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}
