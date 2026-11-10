//! Copied files between linked PCs: copy files or folders in Explorer or Finder on one PC,
//! paste them on the other. Goes to the PCs with Clipboard sync on, like copied text.
//!
//! Copying only sends the list (`clip_files`: names, sizes, an id per file); nothing moves
//! until it's needed here:
//!
//! - small copies (up to [EAGER_LIMIT]) come right away, so any paste works, menus included;
//! - bigger ones when the shared mouse brings the pointer to this PC (up to [ARRIVAL_LIMIT]);
//! - otherwise on paste: the shared mouse holds Ctrl+V / ⌘V while they come, then pastes.
//!   Pressed again while waiting, it pastes what's on the clipboard right away.
//!
//! They land in a temporary folder and go on this PC's clipboard as files, so Explorer or
//! Finder copies them from there. Without the shared mouse (Linux, or no permission) they
//! always come right away.
//!
//! Files come from the PC this one joined with `GET /download/<id>`; from a PC that joined
//! this one, this PC asks (`clip_get`) and it uploads them (`POST /upload?req=…`), as a phone
//! does for Browse files. `clip_error` answers a `clip_get` that failed.

use crate::config::{DeviceKind, random_hex};
use crate::files::Get;
use crate::platform::{self, clipboard};
use crate::server::App;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

/// Copies up to this size come as soon as they're copied.
const EAGER_LIMIT: u64 = 64 << 20;
/// Copies up to this size come when the pointer comes over; bigger ones wait for a paste.
const ARRIVAL_LIMIT: u64 = 2 << 30;
/// Copies bigger than this say when they start coming and when they're here.
const NOTIFY_LIMIT: u64 = 16 << 20;
/// No copy lists more than this many files and folders (a folder like node_modules).
const MAX_ITEMS: usize = 10_000;
/// How long one file may take.
const FILE_TIMEOUT: Duration = Duration::from_secs(6 * 3600);

/// One file or folder of a copy, its path relative to where it was copied from.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Item {
    /// For files: what to ask the other PC for.
    #[serde(default)]
    id: String,
    /// `/`-separated, the copied file or folder first: `Photos/2024/a.jpg`.
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    dir: bool,
}

/// What this PC copied, offered to the linked PCs.
#[derive(Default)]
struct Outgoing {
    /// File id → its path, name and size.
    files: HashMap<String, (PathBuf, String, u64)>,
    /// The PCs it was offered to.
    to: HashSet<String>,
}

/// Files copied on another PC.
struct Incoming {
    set: String,
    from: String,
    from_name: String,
    items: Vec<Item>,
    total: u64,
    state: Fetch,
    /// A paste is waiting for them.
    paste: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Fetch {
    Waiting,
    Running,
    Done,
}

#[derive(Default)]
pub struct ClipFiles {
    outgoing: Mutex<Outgoing>,
    incoming: Mutex<Option<Incoming>>,
}

/// Follows files copied on this PC (service start), and clears what earlier runs left.
pub fn start(app: &Arc<App>) {
    let _ = std::fs::remove_dir_all(temp_root());
    let Some(mut changes) = clipboard::file_changes() else { return };
    let app = app.clone();
    tokio::spawn(async move {
        while let Some(paths) = changes.recv().await {
            local_copy(&app);
            let app = app.clone();
            tokio::spawn(async move { offer(&app, paths).await });
        }
    });
}

/// Something was copied on this PC: files copied on another PC are no longer what a paste
/// pastes.
pub fn local_copy(app: &App) {
    if app.clip_files.incoming.lock().unwrap().take().is_some() {
        app.mouse.arm_paste(false);
    }
}

/// The linked PCs that get this PC's clipboard and are connected: (id, name).
fn targets(app: &App) -> Vec<(String, String)> {
    let pcs: Vec<(String, String, bool)> = {
        let state = app.state.lock().unwrap();
        state
            .peers
            .iter()
            .filter(|p| p.clipboard_auto)
            .map(|p| (p.id.clone(), p.name.clone(), true))
            .chain(
                state
                    .devices
                    .iter()
                    .filter(|d| d.kind == DeviceKind::Pc && d.sends_clipboard())
                    .map(|d| (d.id.clone(), d.name.clone(), false)),
            )
            .collect()
    };
    pcs.into_iter()
        .filter(|(id, _, joined)| if *joined { app.peers.online(id) } else { app.pc_connected(id) })
        .map(|(id, name, _)| (id, name))
        .collect()
}

/// Tells the linked PCs about files copied here.
async fn offer(app: &Arc<App>, paths: Vec<PathBuf>) {
    tracing::debug!("copied files: {paths:?}");
    // Files that came from another PC (just put on the clipboard, or copied again from where
    // they wait): never offered back, else two PCs would pass them back and forth.
    let root = std::fs::canonicalize(temp_root()).unwrap_or_else(|_| temp_root());
    if paths.iter().all(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()).starts_with(&root)) {
        return;
    }
    let targets = targets(app);
    if targets.is_empty() {
        *app.clip_files.outgoing.lock().unwrap() = Outgoing::default();
        return;
    }
    let listed = tokio::task::spawn_blocking(move || list(&paths)).await;
    let Ok((items, files)) = listed else { return };
    if items.is_empty() {
        return;
    }
    let total: u64 = items.iter().map(|i| i.size).sum();
    *app.clip_files.outgoing.lock().unwrap() = Outgoing { files, to: targets.iter().map(|(id, _)| id.clone()).collect() };
    let msg = json!({ "t": "clip_files", "set": random_hex(8), "items": items, "total": total }).to_string();
    for (id, name) in &targets {
        if app.send_to_pc(id, msg.clone()) {
            tracing::info!("copied files: {} item(s), {total} bytes, offered to {name}", items.len());
        }
    }
}

/// The files and folders under `paths`, and the files' ids.
#[allow(clippy::type_complexity)]
fn list(paths: &[PathBuf]) -> (Vec<Item>, HashMap<String, (PathBuf, String, u64)>) {
    let mut items = Vec::new();
    let mut files = HashMap::new();
    let mut stack: Vec<(PathBuf, String)> = paths
        .iter()
        .filter_map(|p| Some((p.clone(), p.file_name()?.to_string_lossy().into_owned())))
        .rev()
        .collect();
    while let Some((path, rel)) = stack.pop() {
        if items.len() >= MAX_ITEMS {
            tracing::warn!("copied files: more than {MAX_ITEMS}, the rest left out");
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.is_dir() {
            items.push(Item { id: String::new(), path: rel.clone(), size: 0, dir: true });
            let Ok(entries) = std::fs::read_dir(&path) else { continue };
            let mut children: Vec<(PathBuf, String)> = entries
                .flatten()
                .map(|e| (e.path(), format!("{rel}/{}", e.file_name().to_string_lossy())))
                .collect();
            children.sort();
            stack.extend(children.into_iter().rev());
        } else if meta.is_file() {
            let id = random_hex(12);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            items.push(Item { id: id.clone(), path: rel, size: meta.len(), dir: false });
            files.insert(id, (path, name, meta.len()));
        }
    }
    (items, files)
}

/// A file copied here that `device` may download (`GET /download/<id>`).
pub fn offered(app: &App, id: &str, device: &str) -> Option<(PathBuf, String, u64)> {
    let outgoing = app.clip_files.outgoing.lock().unwrap();
    if !outgoing.to.contains(device) {
        return None;
    }
    outgoing.files.get(id).cloned()
}

/// A control message from a linked PC: true when it was about copied files.
pub fn handle(app: &Arc<App>, from: &str, v: &Value) -> bool {
    match v["t"].as_str() {
        Some("clip_files") => {
            received(app, from, v);
            true
        }
        Some("clip_get") => {
            let (Some(req), Some(id)) = (v["req"].as_str(), v["id"].as_str()) else { return true };
            let file = offered(app, id, from);
            let (app, from, req) = (app.clone(), from.to_string(), req.to_string());
            tokio::spawn(async move {
                let result = match file {
                    Some((path, _, _)) => crate::peer::send_requested(&app, &from, &path, &req).await,
                    None => Err(anyhow::anyhow!("it's no longer on the clipboard")),
                };
                if let Err(e) = result {
                    tracing::warn!("copied file: {e:#}");
                    app.send_to_pc(&from, json!({ "t": "clip_error", "req": req, "error": format!("{e:#}") }).to_string());
                }
            });
            true
        }
        Some("clip_error") => {
            app.files.answer(v.clone());
            true
        }
        _ => false,
    }
}

/// Files copied on another PC.
fn received(app: &Arc<App>, from: &str, v: &Value) {
    let Some(set) = v["set"].as_str().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())) else { return };
    let items: Vec<Item> = serde_json::from_value(v["items"].clone()).unwrap_or_default();
    // Only paths that stay inside the folder they go to.
    let items: Vec<Item> = items.into_iter().filter_map(|i| Some(Item { path: safe_path(&i.path)?, ..i })).collect();
    if items.is_empty() {
        return;
    }
    let total = items.iter().filter(|i| !i.dir).map(|i| i.size).sum();
    let from_name = pc_name(app, from);
    tracing::info!("{from_name} copied {} item(s), {total} bytes", items.len());
    *app.clip_files.incoming.lock().unwrap() =
        Some(Incoming { set: set.to_string(), from: from.to_string(), from_name, items, total, state: Fetch::Waiting, paste: false });
    // Earlier copies' files: their pastes are long done.
    let keep = temp_root().join(set);
    tokio::task::spawn_blocking(move || {
        for entry in std::fs::read_dir(temp_root()).into_iter().flatten().flatten() {
            if entry.path() != keep {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
    if total <= EAGER_LIMIT || !app.mouse.running() {
        fetch(app);
    } else {
        app.mouse.arm_paste(true);
    }
}

/// The shared mouse brought the pointer to this PC.
pub fn arrived(app: &Arc<App>) {
    let ready = app.clip_files.incoming.lock().unwrap().as_ref().is_some_and(|i| i.state == Fetch::Waiting && i.total <= ARRIVAL_LIMIT);
    if ready {
        fetch(app);
    }
}

/// Ctrl+V / ⌘V while files copied on another PC haven't come yet: they come, then it pastes.
pub fn paste_pressed(app: &Arc<App>) {
    let state = {
        let mut incoming = app.clip_files.incoming.lock().unwrap();
        let Some(i) = incoming.as_mut() else {
            drop(incoming);
            // Nothing to wait for (any more): paste what's there.
            app.mouse.paste();
            return;
        };
        // Pressed again while waiting: no more waiting, it pastes what's there.
        if i.paste && i.state == Fetch::Running {
            i.paste = false;
            drop(incoming);
            app.mouse.arm_paste(false);
            app.mouse.paste();
            return;
        }
        i.paste = true;
        // Waiting for a big copy says so: a paste that does nothing for a while looks broken.
        if i.state != Fetch::Done {
            let keys = if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" };
            platform::notify(
                "Pasting soon",
                &format!(
                    "Getting {} ({}) from {}; it pastes when it's here. {keys} again pastes without waiting.",
                    describe(&i.items),
                    megabytes(i.total),
                    i.from_name
                ),
            );
        }
        i.state
    };
    match state {
        Fetch::Waiting => fetch(app),
        Fetch::Running => {}
        Fetch::Done => app.mouse.paste(),
    }
}

/// Gets the files copied on another PC into a temporary folder and puts them on the clipboard.
fn fetch(app: &Arc<App>) {
    let (set, from, from_name, items, total) = {
        let mut incoming = app.clip_files.incoming.lock().unwrap();
        let Some(i) = incoming.as_mut().filter(|i| i.state == Fetch::Waiting) else { return };
        i.state = Fetch::Running;
        // A big copy takes a while: say it's coming (a paste waiting for it says so itself).
        if i.total > NOTIFY_LIMIT && !i.paste {
            platform::notify(
                &format!("Copying from {}", i.from_name),
                &format!("{} ({}): it can be pasted once it's here. Progress in Dotlink → Activity.", describe(&i.items), megabytes(i.total)),
            );
        }
        (i.set.clone(), i.from.clone(), i.from_name.clone(), i.items.clone(), i.total)
    };
    // A paste from now on waits for them.
    app.mouse.arm_paste(true);
    let app = app.clone();
    tokio::spawn(async move {
        let result = fetch_all(&app, &set, &from, &items).await;
        let mut incoming = app.clip_files.incoming.lock().unwrap();
        // Replaced meanwhile (copied again, here or there): this one is stale.
        let Some(i) = incoming.as_mut().filter(|i| i.set == set) else { return };
        let paste = std::mem::take(&mut i.paste);
        match result {
            Ok(()) => {
                i.state = Fetch::Done;
                drop(incoming);
                app.mouse.arm_paste(false);
                if paste {
                    app.mouse.paste();
                } else if total > NOTIFY_LIMIT {
                    let keys = if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" };
                    platform::notify("Ready to paste", &format!("{} from {from_name}: {keys} pastes it.", describe(&items)));
                }
            }
            Err(e) => {
                incoming.take();
                drop(incoming);
                app.mouse.arm_paste(false);
                tracing::warn!("files copied on {from_name}: {e:#}");
                platform::notify("Couldn't paste", &format!("The files copied on {from_name}: {e:#}"));
            }
        }
    });
}

async fn fetch_all(app: &Arc<App>, set: &str, from: &str, items: &[Item]) -> anyhow::Result<()> {
    let root = temp_root().join(set);
    tokio::fs::create_dir_all(&root).await?;
    // What goes on the clipboard: the copied files and folders themselves, as saved.
    let mut top: Vec<PathBuf> = Vec::new();
    for item in items {
        let path = root.join(&item.path);
        let is_top = !item.path.contains('/');
        if item.dir {
            tokio::fs::create_dir_all(&path).await?;
            if is_top {
                top.push(path);
            }
            continue;
        }
        let dir = path.parent().unwrap_or(&root).to_path_buf();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let saved = fetch_one(app, from, item, &name, &dir).await?;
        // Saved under another name if it had to be: rename it back.
        if saved != path {
            tokio::fs::rename(&saved, &path).await?;
        }
        if is_top {
            top.push(path);
        }
    }
    clipboard::set_files(top).await
}

async fn fetch_one(app: &Arc<App>, from: &str, item: &Item, name: &str, dir: &Path) -> anyhow::Result<PathBuf> {
    // The PC this one joined: download from it.
    if app.peers.online(from) {
        return crate::peer::fetch(app, from, &item.id, name, item.size, dir).await;
    }
    // A PC that joined this one: it uploads what this PC asks for.
    let req = random_hex(8);
    let (tx, rx) = oneshot::channel();
    app.files.expect(req.clone(), Get { device: from.to_string(), dir: dir.to_path_buf(), done: tx });
    if !app.send_to_pc(from, json!({ "t": "clip_get", "req": req, "id": item.id }).to_string()) {
        app.files.forget(&req);
        anyhow::bail!("{} is not connected", pc_name(app, from));
    }
    let result = tokio::time::timeout(FILE_TIMEOUT, rx).await;
    app.files.forget(&req);
    match result {
        Ok(Ok(Ok(path))) => Ok(path),
        Ok(Ok(Err(e))) => anyhow::bail!("{e}"),
        Ok(Err(_)) => anyhow::bail!("cancelled"),
        Err(_) => anyhow::bail!("{} didn't send {name}", pc_name(app, from)),
    }
}

/// "photo.jpg", or "3 files" / "Photos and 2 more".
fn describe(items: &[Item]) -> String {
    let top: Vec<&str> = items.iter().map(|i| i.path.as_str()).filter(|p| !p.contains('/')).collect();
    match top.as_slice() {
        [one] => one.to_string(),
        [first, rest @ ..] => format!("{first} and {} more", rest.len()),
        [] => "the files".into(),
    }
}

fn megabytes(bytes: u64) -> String {
    if bytes < 1 << 20 { format!("{} KB", bytes.div_ceil(1 << 10)) } else { format!("{:.0} MB", bytes as f64 / (1 << 20) as f64) }
}

/// `a/b/c` with each part a plain name; None for `..`, absolute paths and the like. Characters
/// Windows doesn't allow in names (a Mac allows them) become `_`.
fn safe_path(path: &str) -> Option<String> {
    let parts: Option<Vec<String>> = path.split('/').map(safe_name).collect();
    Some(parts?.join("/"))
}

fn safe_name(part: &str) -> Option<String> {
    if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
        return None;
    }
    let name: String =
        part.chars().map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    let name = name.trim_end_matches(['.', ' ']);
    (!name.is_empty() && name.len() <= 255).then(|| name.to_string())
}

fn pc_name(app: &App, id: &str) -> String {
    let state = app.state.lock().unwrap();
    state
        .peers
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .or_else(|| state.devices.iter().find(|d| d.id == id).map(|d| d.name.clone()))
        .unwrap_or_else(|| "the other PC".into())
}

/// Where files copied on other PCs wait to be pasted.
fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("dotlink-clipboard{}", crate::config::instance_suffix()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside() {
        assert_eq!(safe_path("Photos/2024/a.jpg").as_deref(), Some("Photos/2024/a.jpg"));
        assert_eq!(safe_path("a.txt").as_deref(), Some("a.txt"));
        assert_eq!(safe_path("../etc/passwd"), None);
        assert_eq!(safe_path("a/../../b"), None);
        assert_eq!(safe_path("/abs"), None);
        assert_eq!(safe_path(""), None);
        assert_eq!(safe_path("a\\b"), None);
        assert_eq!(safe_path(".gitignore").as_deref(), Some(".gitignore"));
        assert_eq!(safe_path("Notes: 1?.txt").as_deref(), Some("Notes_ 1_.txt"));
    }

    #[test]
    fn lists_folders_with_their_files() {
        let dir = std::env::temp_dir().join(format!("dotlink-test-{}", random_hex(6)));
        std::fs::create_dir_all(dir.join("Top/sub")).unwrap();
        std::fs::write(dir.join("Top/a.txt"), "aa").unwrap();
        std::fs::write(dir.join("Top/sub/b.txt"), "bbb").unwrap();
        std::fs::write(dir.join("c.txt"), "c").unwrap();
        let (items, files) = list(&[dir.join("Top"), dir.join("c.txt")]);
        let paths: Vec<(&str, bool, u64)> = items.iter().map(|i| (i.path.as_str(), i.dir, i.size)).collect();
        assert_eq!(paths, [("Top", true, 0), ("Top/a.txt", false, 2), ("Top/sub", true, 0), ("Top/sub/b.txt", false, 3), ("c.txt", false, 1)]);
        assert_eq!(files.len(), 3);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
