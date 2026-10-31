//! Local control socket for the `dotlink` CLI and the desktop window.
//! One JSON request line in, one or more JSON reply lines out.
//!
//! Linux/macOS: a Unix socket in the user's runtime dir. Windows: a named pipe.

use crate::config::Config;
use crate::events::UiRequest;
use crate::server::App;
use anyhow::{Context, Result, bail};
use interprocess::local_socket::tokio::{Stream, prelude::*};
use interprocess::local_socket::{ListenerOptions, Name};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[cfg(unix)]
fn socket_name() -> std::io::Result<Name<'static>> {
    use interprocess::local_socket::GenericFilePath;
    // Inside $XDG_RUNTIME_DIR, which only this user can open.
    socket_path().to_fs_name::<GenericFilePath>()
}

#[cfg(unix)]
fn socket_path() -> PathBuf {
    crate::config::runtime_dir().join(format!("dotlink{}.sock", crate::config::instance_suffix()))
}

/// `\\.\pipe\dotlink-<user SID>`: one per user, also when two users share a name across domains.
#[cfg(windows)]
fn socket_name() -> std::io::Result<Name<'static>> {
    use interprocess::local_socket::GenericNamespaced;
    format!("dotlink-{}{}", crate::platform::user_sid()?, crate::config::instance_suffix())
        .to_ns_name::<GenericNamespaced>()
}

#[cfg(unix)]
fn listener_options() -> std::io::Result<ListenerOptions<'static>> {
    Ok(ListenerOptions::new().name(socket_name()?))
}

/// Named pipes are open to every user on the machine by default; through this one they could
/// pair a phone or read the clipboard. Only this user (and nobody else, not even admins) gets in.
#[cfg(windows)]
fn listener_options() -> std::io::Result<ListenerOptions<'static>> {
    use interprocess::os::windows::local_socket::ListenerOptionsExt;
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    let sddl = format!("D:P(A;;GA;;;{})", crate::platform::user_sid()?);
    let sd = SecurityDescriptor::deserialize(&widestring::U16CString::from_str_truncate(sddl))?;
    Ok(ListenerOptions::new().name(socket_name()?).security_descriptor(sd))
}

#[cfg(unix)]
fn remove_stale_socket() {
    let _ = std::fs::remove_file(socket_path());
}

#[cfg(windows)]
fn remove_stale_socket() {}

/// Called when the service stops.
pub fn cleanup() {
    remove_stale_socket();
}

/// Whether a Dotlink service is already answering on the socket.
pub async fn running() -> bool {
    match socket_name() {
        Ok(name) => Stream::connect(name).await.is_ok(),
        Err(_) => false,
    }
}

pub async fn serve(app: Arc<App>) -> Result<()> {
    if running().await {
        bail!("Dotlink is already running");
    }
    remove_stale_socket();
    let listener = listener_options()?.create_tokio()?;
    loop {
        let stream = listener.accept().await?;
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(app, stream).await {
                tracing::debug!("ipc: {e}");
            }
        });
    }
}

async fn handle(app: Arc<App>, stream: Stream) -> Result<()> {
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).await?;
    let req: Value = serde_json::from_str(&line)?;
    let mut w = &stream;
    let reply = |v: Value| {
        let mut s = v.to_string();
        s.push('\n');
        s
    };
    let result = |r: Result<String>| match r {
        Ok(ok) => json!({ "ok": ok }),
        Err(e) => json!({ "error": format!("{e:#}") }),
    };
    match req["cmd"].as_str().unwrap_or("") {
        "pair" => {
            let (uri, code, done) = app.start_pairing();
            w.write_all(reply(json!({ "uri": uri, "code": code })).as_bytes()).await?;
            let msg = match tokio::time::timeout(std::time::Duration::from_secs(300), done).await {
                Ok(Ok(name)) => json!({ "paired": name }),
                _ => json!({ "error": "pairing timed out" }),
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "pair_usb" => {
            let uri = req["uri"].as_str().unwrap_or("");
            let msg = result(
                crate::usb::open_pairing(uri, false)
                    .await
                    .map(|phones| format!("Tap Pair on {} to finish", phones.join(" and "))),
            );
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "send" => {
            let paths: Vec<PathBuf> =
                req["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(PathBuf::from)).collect();
            let msg = result(
                app.offer_files(&paths, req["to"].as_str())
                    .map(|(n, names)| format!("sending {n} file(s) to {names}")),
            );
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "clip" => {
            let msg = result(app.send_clipboard(req["to"].as_str()).await.map(|()| "clipboard sent".into()));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "open" => {
            let msg = result(app.send_url(req["url"].as_str(), req["to"].as_str()).await.map(|names| format!("opened on {names}")));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "ring" => {
            let on = req["on"].as_bool().unwrap_or(true);
            let msg = result(
                app.ring(req["to"].as_str(), on)
                    .map(|names| if on { format!("ringing {names}") } else { format!("stopped ringing {names}") }),
            );
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "answer_incoming" => {
            let (id, accept) = (req["id"].as_str().unwrap_or(""), req["accept"].as_bool().unwrap_or(false));
            let msg = if app.answer_incoming(id, accept) {
                json!({ "ok": if accept { "accepted" } else { "declined" } })
            } else {
                json!({ "error": "that file isn't waiting any more" })
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "clear_transfers" => {
            app.clear_transfers();
            w.write_all(reply(json!({ "ok": "cleared" })).as_bytes()).await?;
        }
        "nearby" => {
            let pcs = crate::peer::nearby(&app).await;
            w.write_all(reply(json!({ "pcs": pcs })).as_bytes()).await?;
        }
        "join_code" => {
            let (id, code) = (req["id"].as_str().unwrap_or(""), req["code"].as_str().unwrap_or(""));
            let msg = result(crate::peer::join_code(&app, id, code).await.map(|name| format!("linked to {name}")));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "join" => {
            let msg = result(crate::peer::join(&app, req["uri"].as_str().unwrap_or("")).await.map(|name| format!("linked to {name}")));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "clipboard_auto" => {
            let on = req["on"].as_bool().unwrap_or(false);
            let msg = result(app.set_clipboard_auto(req["id"].as_str().unwrap_or(""), on).map(|name| {
                format!("{name} {} this PC's clipboard changes", if on { "gets" } else { "no longer gets" })
            }));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "devices" => {
            w.write_all(reply(json!({ "devices": app.devices_json() })).as_bytes()).await?;
        }
        "media" => {
            let kind = req["kind"].as_str().unwrap_or("");
            let on = req["on"].as_bool().unwrap_or(false);
            let msg = result(app.request_stream(kind, on, req["to"].as_str()).map(|names| {
                let what = if kind == "mic" { "microphone" } else { "webcam" };
                format!("asked {names} to turn the {what} {}", if on { "on" } else { "off" })
            }));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "cast" => {
            let id = req["id"].as_str();
            let msg = if req["on"].as_bool().unwrap_or(true) {
                result(crate::cast::start(&app, id).await.map(String::from))
            } else {
                crate::cast::stop(&app, id);
                json!({ "ok": "stopped" })
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "cast_pair" => {
            let code = req["code"].as_str().unwrap_or("");
            let msg = result(crate::cast::pair(&app, req["id"].as_str(), code).await.map(|()| "paired".into()));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "status" => {
            w.write_all(reply(app.status()).as_bytes()).await?;
        }
        "rename" => {
            let name = req["name"].as_str().unwrap_or("");
            let msg = result(app.set_name(req["id"].as_str().unwrap_or(""), name).map(|old| format!("{old} is now {}", name.trim())));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "unpair" => {
            let msg = result(app.unpair(req["id"].as_str().unwrap_or("")).map(|name| format!("unpaired {name}")));
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "set_config" => {
            let msg = match serde_json::from_value::<Config>(req["config"].clone()) {
                Ok(config) => match app.set_config(config) {
                    Ok(restart) => json!({ "ok": "saved", "restart": restart }),
                    Err(e) => json!({ "error": format!("{e:#}") }),
                },
                Err(e) => json!({ "error": format!("bad config: {e}") }),
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        // One line per event until the client hangs up.
        "watch" => {
            let mut events = app.events.subscribe();
            loop {
                match events.recv().await {
                    Ok(ev) => w.write_all(reply(serde_json::to_value(ev)?).as_bytes()).await?,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Missed some: a full refresh covers it.
                        w.write_all(reply(json!({ "t": "devices" })).as_bytes()).await?
                    }
                    Err(_) => break,
                }
            }
        }
        // A second launch of the app: show the running one's window.
        "show" => {
            tracing::info!("another launch of Dotlink asked for the window");
            let msg = if app.request_ui(UiRequest::Show) {
                json!({ "ok": "shown" })
            } else {
                json!({ "error": "running without a window (dotlink daemon)" })
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "quit" => {
            w.write_all(reply(json!({ "ok": "stopping" })).as_bytes()).await?;
            app.shutdown.notify_waiters();
        }
        _ => w.write_all(reply(json!({ "error": "unknown command" })).as_bytes()).await?,
    }
    Ok(())
}

/// Sends `req` to the running service and calls `on_reply` for each reply line.
pub async fn request(req: Value, mut on_reply: impl FnMut(Value) -> Result<()>) -> Result<()> {
    let stream = Stream::connect(socket_name()?)
        .await
        .context("Dotlink is not running (open the Dotlink app, or run `dotlink daemon`)")?;
    (&stream).write_all(format!("{req}\n").as_bytes()).await?;
    let mut lines = BufReader::new(&stream).lines();
    while let Some(line) = lines.next_line().await? {
        on_reply(serde_json::from_str(&line)?)?;
    }
    Ok(())
}

/// One request, its first reply.
pub async fn call(req: Value) -> Result<Value> {
    let mut first = None;
    request(req, |v| {
        if first.is_none() {
            first = Some(v);
        }
        Ok(())
    })
    .await?;
    first.context("no reply from Dotlink")
}
