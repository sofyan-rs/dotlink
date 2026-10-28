//! What the desktop UI follows live: device changes, transfers, clipboard. Sent to `watch`
//! clients on the control socket as one JSON line each.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Event {
    /// A phone paired, was forgotten, came online or went offline.
    Devices,
    /// Pairing finished with this phone.
    Paired { name: String },
    /// A transfer started, progressed or ended (the whole transfer, not a delta).
    Transfer(Transfer),
    /// Clipboard text went to (`to_phone`) or came from the phone.
    Clipboard { to_phone: bool, device: String, chars: usize },
    /// config.toml changed.
    Config,
    /// The phone's camera or mic stream started or stopped.
    Streams,
    /// Casting the phone screen started, stopped or needs something (see `cast::CastState`).
    Cast,
    /// The phone needs a tap on its notification to start `kind` ("camera" | "mic").
    MediaPrompt { device: String, kind: String },
    /// A file waits for Accept or Decline ("Ask before receiving files"): `answer_incoming`.
    Incoming(Incoming),
    /// That file was answered, or nobody did in time.
    IncomingDone { id: String },
}

/// A file someone wants to send here, waiting for an answer. For several files sent together,
/// one answer for all of them: `name` is the first, `size` their total.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incoming {
    pub id: String,
    /// The phone's or PC's name.
    pub from: String,
    pub name: String,
    pub size: Option<u64>,
    #[serde(default = "one")]
    pub count: u32,
}

fn one() -> u32 {
    1
}

impl Incoming {
    /// "X wants to send a file" / "… 3 files", for the notification.
    pub fn title(&self) -> String {
        match self.count {
            0 | 1 => format!("{} wants to send a file", self.from),
            n => format!("{} wants to send {n} files", self.from),
        }
    }

    /// The file's name, or "a.jpg and 2 more".
    pub fn names(&self) -> String {
        match self.count {
            0 | 1 => self.name.clone(),
            n => format!("{} and {} more", self.name, n - 1),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub id: String,
    /// true: PC → phone, false: phone → PC.
    pub outgoing: bool,
    /// Phone name; for outgoing files, whichever phone downloads it.
    pub device: String,
    pub name: String,
    pub size: Option<u64>,
    pub done: u64,
    pub state: TransferState,
    /// Saved file (incoming) or the file being sent (outgoing).
    pub path: String,
    pub started_at: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", content = "error", rename_all = "snake_case")]
pub enum TransferState {
    /// Offered to the phone, not downloaded yet.
    Waiting,
    Active,
    Done,
    Failed(String),
}

/// Requests from the core to the window of the app it runs in.
#[derive(Debug, Clone)]
pub enum UiRequest {
    /// Another `dotlink` launch: bring the window up instead of starting twice.
    Show,
}
