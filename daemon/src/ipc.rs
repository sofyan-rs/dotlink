//! Local control socket used by the `dotlink` CLI (pair, send, devices…).
//! One JSON request line in, one or more JSON reply lines out.

use crate::config::socket_path;
use crate::server::App;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

pub async fn serve(app: Arc<App>) -> Result<()> {
    let path = socket_path();
    if path.exists() {
        if UnixStream::connect(&path).await.is_ok() {
            bail!("another dotlink daemon is already running");
        }
        std::fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let app = app.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(app, stream).await {
                tracing::debug!("ipc: {e}");
            }
        });
    }
}

async fn handle(app: Arc<App>, stream: UnixStream) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await?;
    let req: Value = serde_json::from_str(&line)?;
    let reply = |v: Value| {
        let mut s = v.to_string();
        s.push('\n');
        s
    };
    match req["cmd"].as_str().unwrap_or("") {
        "pair" => {
            let (uri, done) = app.start_pairing();
            w.write_all(reply(json!({ "uri": uri })).as_bytes()).await?;
            let msg = match tokio::time::timeout(std::time::Duration::from_secs(300), done).await {
                Ok(Ok(name)) => json!({ "paired": name }),
                _ => json!({ "error": "pairing timed out" }),
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "send" => {
            let paths: Vec<PathBuf> =
                req["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(PathBuf::from)).collect();
            let msg = match app.offer_files(&paths) {
                Ok(n) => json!({ "ok": format!("sent {n} file(s) to {}", app.online_devices().join(", ")) }),
                Err(e) => json!({ "error": e.to_string() }),
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "clip" => {
            let msg = match crate::clipboard::get().await {
                Some(text) if !app.online_devices().is_empty() => {
                    app.remember_clip(&text);
                    app.send_control(json!({ "t": "clip", "text": text }));
                    json!({ "ok": "clipboard sent" })
                }
                Some(_) => json!({ "error": "no phone connected" }),
                None => json!({ "error": "clipboard is empty or not text" }),
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        "devices" => {
            let online = app.online_devices();
            let devices: Vec<Value> = app
                .state
                .lock()
                .unwrap()
                .devices
                .iter()
                .map(|d| json!({ "id": d.id, "name": d.name, "online": online.contains(&d.name) }))
                .collect();
            w.write_all(reply(json!({ "devices": devices })).as_bytes()).await?;
        }
        "unpair" => {
            let msg = match app.unpair(req["id"].as_str().unwrap_or("")) {
                Ok(name) => json!({ "ok": format!("unpaired {name}") }),
                Err(e) => json!({ "error": e.to_string() }),
            };
            w.write_all(reply(msg).as_bytes()).await?;
        }
        _ => w.write_all(reply(json!({ "error": "unknown command" })).as_bytes()).await?,
    }
    Ok(())
}

/// Sends `req` to the running daemon and calls `on_reply` for each reply line.
pub async fn request(req: Value, mut on_reply: impl FnMut(Value) -> Result<()>) -> Result<()> {
    let stream = UnixStream::connect(socket_path())
        .await
        .context("dotlink daemon is not running (start it with `dotlink daemon`)")?;
    let (r, mut w) = stream.into_split();
    w.write_all(format!("{req}\n").as_bytes()).await?;
    let mut lines = BufReader::new(r).lines();
    while let Some(line) = lines.next_line().await? {
        on_reply(serde_json::from_str(&line)?)?;
    }
    Ok(())
}
