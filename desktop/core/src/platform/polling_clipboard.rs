//! Clipboard text through `arboard`. Changes are noticed by polling twice a second, or on
//! Linux by XFixes events (then only a slow safety poll remains).
//!
//! One thread owns the clipboard handle: on X11 pasted data is served by the owning process,
//! so the handle must live as long as the app. Works the same on Windows and macOS, where
//! arboard uses the native clipboard.

use crate::server::App;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::Duration;

/// XFixes tells us when the clipboard owner changes; polling is then only a fallback.
static EVENTS: AtomicBool = AtomicBool::new(false);

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

/// Files and folders copied on this PC from now on (Explorer, Finder), for `clip_files`.
pub fn file_changes() -> Option<tokio::sync::mpsc::UnboundedReceiver<Vec<PathBuf>>> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    shared().file_subscribers.lock().unwrap().push(tx);
    Some(rx)
}

/// Puts files on the clipboard, as copying them in Explorer or Finder does.
pub async fn set_files(paths: Vec<PathBuf>) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
        let (tx, rx) = mpsc::channel();
        shared().cmds.send(Cmd::SetFiles(paths, tx))?;
        rx.recv_timeout(Duration::from_secs(5))?
    })
    .await?
}

enum Cmd {
    Get(mpsc::Sender<Option<String>>),
    Set(String, mpsc::Sender<anyhow::Result<()>>),
    SetFiles(Vec<PathBuf>, mpsc::Sender<anyhow::Result<()>>),
    /// The clipboard got a new owner (XFixes): read it now.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Changed,
}

pub struct PollingClipboard {
    cmds: mpsc::Sender<Cmd>,
    subscribers: std::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<String>>>,
    file_subscribers: std::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<Vec<PathBuf>>>>,
}

static SHARED: OnceLock<PollingClipboard> = OnceLock::new();

pub fn shared() -> &'static PollingClipboard {
    SHARED.get_or_init(|| {
        let (cmds, rx) = mpsc::channel::<Cmd>();
        std::thread::Builder::new()
            .name("dotlink-clipboard".into())
            .spawn(move || run(rx))
            .expect("spawn clipboard thread");
        #[cfg(target_os = "linux")]
        {
            let changes = cmds.clone();
            let _ = std::thread::Builder::new().name("dotlink-xfixes".into()).spawn(move || xfixes::watch(changes));
        }
        PollingClipboard {
            cmds,
            subscribers: std::sync::Mutex::new(Vec::new()),
            file_subscribers: std::sync::Mutex::new(Vec::new()),
        }
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
    // The files on the clipboard as last seen; None before the first look.
    let mut last_files: Option<Vec<PathBuf>> = None;
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
                            Cmd::Set(_, reply) | Cmd::SetFiles(_, reply) => {
                                drop(reply.send(Err(anyhow::anyhow!("clipboard unavailable: {e}"))))
                            }
                            Cmd::Changed => {}
                        }
                    }
                    continue;
                }
            }
        }
        let cb = clipboard.as_mut().unwrap();
        // Reading the clipboard copies all of it from its owner, so with change events the
        // poll is only a slow safety net.
        let poll = if EVENTS.load(Ordering::Relaxed) { Duration::from_secs(5) } else { Duration::from_millis(500) };
        match cmds.recv_timeout(poll) {
            Ok(Cmd::Get(reply)) => drop(reply.send(cb.get_text().ok())),
            Ok(Cmd::Set(text, reply)) => {
                let result = cb.set_text(text.clone()).map_err(anyhow::Error::from);
                if result.is_ok() {
                    last = Some(text); // our own write is not a change to report
                }
                drop(reply.send(result));
            }
            Ok(Cmd::SetFiles(paths, reply)) => {
                let result = cb.set().file_list(&paths).map_err(anyhow::Error::from);
                if result.is_ok() {
                    last_files = Some(paths);
                }
                drop(reply.send(result));
            }
            Ok(Cmd::Changed) | Err(mpsc::RecvTimeoutError::Timeout) => {
                // Copied files: Finder puts their names on the clipboard as text too, which
                // isn't what was copied.
                let files = match cb.get().file_list() {
                    Ok(files) => files,
                    Err(arboard::Error::ContentNotAvailable) => Vec::new(),
                    // Another app has the clipboard open right now: look again next time,
                    // rather than take it for empty (and the files for copied again).
                    Err(_) => continue,
                };
                let first = last_files.is_none();
                if last_files.as_ref() != Some(&files) {
                    last_files = Some(files.clone());
                    if !first && !files.is_empty() && let Some(me) = SHARED.get() {
                        me.file_subscribers.lock().unwrap().retain(|s| s.send(files.clone()).is_ok());
                    }
                }
                if !files.is_empty() {
                    last = cb.get_text().ok().or(last);
                    continue;
                }
                // Poll: other apps changed the clipboard?
                if let Ok(text) = cb.get_text()
                    && last.as_deref() != Some(text.as_str())
                {
                    let first = last.is_none();
                    last = Some(text.clone());
                    if !first && let Some(me) = SHARED.get() {
                        me.subscribers.lock().unwrap().retain(|s| s.send(text.clone()).is_ok());
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Clipboard owner changes from the X server (XWayland on GNOME follows the Wayland clipboard).
#[cfg(target_os = "linux")]
mod xfixes {
    use super::{Cmd, EVENTS};
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;
    use std::time::Duration;
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
    use x11rb::protocol::xproto::ConnectionExt as _;

    /// Runs for the app's life, reconnecting when the display goes away.
    pub fn watch(changes: mpsc::Sender<Cmd>) {
        loop {
            match follow(&changes) {
                Ok(()) => return, // the clipboard thread is gone
                Err(e) => tracing::debug!("xfixes: {e}"),
            }
            EVENTS.store(false, Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(5));
        }
    }

    fn follow(changes: &mpsc::Sender<Cmd>) -> Result<(), Box<dyn std::error::Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        conn.xfixes_query_version(5, 0)?.reply()?;
        let root = conn.setup().roots[screen].root;
        let clipboard = conn.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
        let mask = SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE;
        conn.xfixes_select_selection_input(root, clipboard, mask)?.check()?;
        EVENTS.store(true, Ordering::Relaxed);
        tracing::debug!("clipboard: following XFixes selection events");
        loop {
            if let Event::XfixesSelectionNotify(_) = conn.wait_for_event()?
                && changes.send(Cmd::Changed).is_err()
            {
                return Ok(());
            }
        }
    }
}
