//! Persistent shell sessions, one per paired device. Closing the app on the phone keeps the
//! shell running; reconnecting replays recent output so the screen comes back.

use anyhow::Result;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

const SCROLLBACK: usize = 256 * 1024;

pub struct Session {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    scrollback: Mutex<VecDeque<u8>>,
    tx: broadcast::Sender<Vec<u8>>,
    alive: Arc<AtomicBool>,
}

impl Session {
    pub fn spawn(shell: &str, cols: u16, rows: u16) -> Result<Arc<Self>> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-l");
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if let Some(home) = dirs::home_dir() {
            cmd.cwd(home);
        }
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let (tx, _) = broadcast::channel(512);
        let alive = Arc::new(AtomicBool::new(true));
        let session = Arc::new(Self {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            scrollback: Mutex::new(VecDeque::with_capacity(SCROLLBACK)),
            tx,
            alive: alive.clone(),
        });
        let weak = Arc::downgrade(&session);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let Some(s) = weak.upgrade() else { break };
                // Append and broadcast under the same lock, so `attach` sees no gap or overlap.
                let mut sb = s.scrollback.lock().unwrap();
                sb.extend(&buf[..n]);
                let excess = sb.len().saturating_sub(SCROLLBACK);
                sb.drain(..excess);
                let _ = s.tx.send(buf[..n].to_vec());
            }
            alive.store(false, Ordering::SeqCst);
            if let Some(s) = weak.upgrade() {
                let _ = s.tx.send(Vec::new()); // empty chunk = shell exited
            }
        });
        Ok(session)
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Returns the recent output plus a receiver for everything after it.
    pub fn attach(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let sb = self.scrollback.lock().unwrap();
        let rx = self.tx.subscribe();
        (sb.iter().copied().collect(), rx)
    }

    pub fn write(&self, data: &[u8]) {
        let mut w = self.writer.lock().unwrap();
        let _ = w.write_all(data);
        let _ = w.flush();
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.lock().unwrap().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }

    /// Name of the program in the foreground ("zsh", "htop", "nvim"…), for the tab title.
    pub fn title(&self) -> String {
        let pid = self.master.lock().unwrap().process_group_leader();
        pid.and_then(|p| std::fs::read_to_string(format!("/proc/{p}/comm")).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "shell".into())
    }

    pub fn kill(&self) {
        let _ = self.child.lock().unwrap().kill();
    }
}
