//! Browsing a phone's storage from this PC: `fs_*` control messages, each with a `req` id the
//! phone answers with. The phone only answers with Browse from PC on in its app; a linked PC
//! has no such thing to ask.
//!
//! Downloads (`fs_get`): the phone uploads the file (`POST /upload?req=…`), and it's saved
//! where this PC asked ([PhoneFiles::take_get]). Uploads go as the usual `file` offer with a
//! `dir`, saved there instead of Download/Dotlink.

use crate::config::random_hex;
use crate::server::App;
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// How long a list, rename or delete may take on the phone.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a download may take, the transfer included.
const GET_TIMEOUT: Duration = Duration::from_secs(6 * 3600);

/// One entry of a folder on the phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    #[serde(default)]
    pub dir: bool,
    #[serde(default)]
    pub size: u64,
    /// Unix time, seconds.
    #[serde(default)]
    pub mtime: u64,
}

/// A download waiting for the phone's upload.
pub(crate) struct Get {
    pub device: String,
    /// The folder it goes into.
    pub dir: PathBuf,
    pub done: oneshot::Sender<std::result::Result<PathBuf, String>>,
}

#[derive(Default)]
pub struct PhoneFiles {
    /// Requests waiting for the phone's answer, by `req`.
    answers: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    gets: Mutex<HashMap<String, Get>>,
}

impl PhoneFiles {
    /// The phone's answer (`fs_list`, `fs_ok`) to request `req`. A failed `fs_get` ends its
    /// download too.
    pub fn answer(&self, v: Value) {
        let Some(req) = v["req"].as_str().map(String::from) else { return };
        if let Some(e) = v["error"].as_str()
            && let Some(get) = self.gets.lock().unwrap().remove(&req)
        {
            let _ = get.done.send(Err(e.to_string()));
            return;
        }
        if let Some(tx) = self.answers.lock().unwrap().remove(&req) {
            let _ = tx.send(v);
        }
    }

    /// The download `req` from `device`, for the upload carrying it; None when nobody asked.
    pub(crate) fn take_get(&self, req: &str, device: &str) -> Option<Get> {
        let mut gets = self.gets.lock().unwrap();
        if gets.get(req).is_some_and(|g| g.device == device) { gets.remove(req) } else { None }
    }
}

/// The phone's id and name: `key` is its name or id, and it must be a connected phone.
fn phone(app: &App, key: &str) -> Result<(String, String)> {
    let target = app.target_of(Some(key), true)?;
    target.phones.into_iter().next().ok_or_else(|| anyhow!("no connected phone \"{key}\""))
}

/// Sends `msg` with a new `req` and waits for the phone's answer; its `error` becomes Err.
async fn ask(app: &App, device: &str, mut msg: Value) -> Result<Value> {
    let req = random_hex(8);
    msg["req"] = json!(req);
    let (tx, rx) = oneshot::channel();
    app.files.answers.lock().unwrap().insert(req.clone(), tx);
    app.send_control_to(Some(device), msg);
    let answer = tokio::time::timeout(ANSWER_TIMEOUT, rx).await;
    app.files.answers.lock().unwrap().remove(&req);
    let v = answer.map_err(|_| anyhow!("the phone didn't answer (update Dotlink on it)"))?.map_err(|_| anyhow!("cancelled"))?;
    if let Some(e) = v["error"].as_str() {
        bail!("{e}");
    }
    Ok(v)
}

/// The folder `path` on the phone (`/` is its internal storage): folders first, then by name.
pub async fn list(app: &App, key: &str, path: &str) -> Result<Vec<Entry>> {
    let (id, _) = phone(app, key)?;
    let v = ask(app, &id, json!({ "t": "fs_list", "path": path })).await?;
    let mut entries: Vec<Entry> = serde_json::from_value(v["entries"].clone()).unwrap_or_default();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

/// Downloads the file `path` from the phone into `dir`. Returns where it was saved.
pub async fn get(app: &App, key: &str, path: &str, dir: &Path) -> Result<PathBuf> {
    let (id, _) = phone(app, key)?;
    let req = random_hex(8);
    let (tx, rx) = oneshot::channel();
    app.files.gets.lock().unwrap().insert(req.clone(), Get { device: id.clone(), dir: dir.to_path_buf(), done: tx });
    app.send_control_to(Some(&id), json!({ "t": "fs_get", "req": req, "path": path }));
    let result = tokio::time::timeout(GET_TIMEOUT, rx).await;
    app.files.gets.lock().unwrap().remove(&req);
    match result {
        Ok(Ok(Ok(path))) => Ok(path),
        Ok(Ok(Err(e))) => bail!("{e}"),
        Ok(Err(_)) => bail!("cancelled"),
        Err(_) => bail!("the phone didn't send it"),
    }
}

/// Sends files into the folder `dir` on the phone.
pub fn put(app: &Arc<App>, key: &str, dir: &str, paths: &[PathBuf]) -> Result<(usize, String)> {
    let (id, _) = phone(app, key)?;
    app.offer_files_into(paths, &id, Some(dir))
}

/// Deletes a file or folder (with what's in it) on the phone.
pub async fn remove(app: &App, key: &str, path: &str) -> Result<()> {
    let (id, _) = phone(app, key)?;
    ask(app, &id, json!({ "t": "fs_rm", "path": path })).await.map(drop)
}

/// Renames or moves `path` to `to`, both on the phone.
pub async fn rename(app: &App, key: &str, path: &str, to: &str) -> Result<()> {
    let (id, _) = phone(app, key)?;
    ask(app, &id, json!({ "t": "fs_mv", "path": path, "to": to })).await.map(drop)
}

pub async fn mkdir(app: &App, key: &str, path: &str) -> Result<()> {
    let (id, _) = phone(app, key)?;
    ask(app, &id, json!({ "t": "fs_mkdir", "path": path })).await.map(drop)
}
