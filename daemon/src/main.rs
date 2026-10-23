//! Dotlink: link an Android phone with a Hyprland PC.
//! Files, clipboard, touchpad/keyboard, terminal, phone camera as webcam, phone mic.

mod clipboard;
mod config;
mod input;
mod ipc;
mod media;
mod server;
mod term;
mod tls;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "dotlink", version, about = "Link your Android phone with Hyprland")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the background service (put `dotlink daemon` in exec-once).
    Daemon,
    /// Show a QR code to pair a phone.
    Pair,
    /// Send files to the connected phone.
    Send {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Send the current PC clipboard to the phone.
    Clip,
    /// List paired phones.
    Devices,
    /// Forget a paired phone (by name or id).
    Unpair { device: String },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Daemon => run_daemon().await,
        Cmd::Pair => {
            ipc::request(json!({ "cmd": "pair" }), |v| {
                if let Some(uri) = v["uri"].as_str() {
                    print_qr(uri);
                    println!("\nScan this with the Dotlink app (valid for 5 minutes).");
                    println!("Or paste this link in the app:\n{uri}\n\nWaiting for the phone…");
                } else if let Some(name) = v["paired"].as_str() {
                    println!("Paired with {name}.");
                } else if let Some(e) = v["error"].as_str() {
                    bail!("{e}");
                }
                Ok(())
            })
            .await
        }
        Cmd::Send { files } => {
            let paths: Vec<String> = files
                .iter()
                .map(|p| std::fs::canonicalize(p).map(|p| p.to_string_lossy().into_owned()))
                .collect::<std::io::Result<_>>()?;
            simple(json!({ "cmd": "send", "paths": paths })).await
        }
        Cmd::Clip => simple(json!({ "cmd": "clip" })).await,
        Cmd::Unpair { device } => simple(json!({ "cmd": "unpair", "id": device })).await,
        Cmd::Devices => {
            ipc::request(json!({ "cmd": "devices" }), |v| {
                let devices = v["devices"].as_array().cloned().unwrap_or_default();
                if devices.is_empty() {
                    println!("No paired phones. Run `dotlink pair`.");
                }
                for d in devices {
                    let state = if d["online"].as_bool().unwrap_or(false) { "online" } else { "offline" };
                    println!("{}  {}  ({state})", d["id"].as_str().unwrap_or(""), d["name"].as_str().unwrap_or(""));
                }
                Ok(())
            })
            .await
        }
    }
}

/// One request, one `ok`/`error` reply.
async fn simple(req: serde_json::Value) -> Result<()> {
    ipc::request(req, |v| {
        if let Some(e) = v["error"].as_str() {
            bail!("{e}");
        }
        println!("{}", v["ok"].as_str().unwrap_or("ok"));
        Ok(())
    })
    .await
}

async fn run_daemon() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "dotlink=info".into()),
        )
        .init();
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let config = config::Config::load()?;
    let state = config::State::load()?;
    let identity = tls::load_or_create()?;
    let app = server::App::new(config.clone(), state, identity.fingerprint.clone());

    // Bind the control socket first: it also guards against two daemons.
    let ipc_task = tokio::spawn(ipc::serve(app.clone()));
    tokio::task::yield_now().await;
    if ipc_task.is_finished() {
        ipc_task.await??;
    }

    if config.clipboard_sync {
        tokio::spawn(clipboard::watch(app.clone()));
    }
    let _mdns = advertise(&app).map_err(|e| tracing::warn!("mDNS disabled: {e}")).ok();

    let tls = axum_server::tls_rustls::RustlsConfig::from_pem(identity.cert_pem, identity.key_pem).await?;
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("listening on {addr} (fingerprint {}…)", &identity.fingerprint[..16]);
    let handle = axum_server::Handle::new();
    let shutdown = handle.clone();
    tokio::spawn(async move {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        shutdown.shutdown();
    });
    axum_server::bind_rustls(addr, tls)
        .handle(handle)
        .serve(server::router(app).into_make_service())
        .await?;
    let _ = std::fs::remove_file(config::socket_path());
    Ok(())
}

/// Announces `_dotlink._tcp` so the phone can find the PC after its IP changes.
fn advertise(app: &Arc<server::App>) -> Result<mdns_sd::ServiceDaemon> {
    let mdns = mdns_sd::ServiceDaemon::new()?;
    let id = app.server_id();
    let host = format!("dotlink-{id}.local.");
    let props = [("id", id.as_str()), ("fp", &app.fingerprint[..16])];
    let info = mdns_sd::ServiceInfo::new(
        "_dotlink._tcp.local.",
        &app.hostname,
        &host,
        "",
        app.config.port,
        &props[..],
    )?
    .enable_addr_auto();
    mdns.register(info)?;
    Ok(mdns)
}

/// Desktop notification through notify-send (mako, dunst, swaync…).
pub fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send").args(["-a", "Dotlink", title, body]).spawn();
}

fn print_qr(data: &str) {
    match qrcode::QrCode::new(data.as_bytes()) {
        Ok(code) => {
            let img = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .dark_color(qrcode::render::unicode::Dense1x2::Light)
                .light_color(qrcode::render::unicode::Dense1x2::Dark)
                .quiet_zone(true)
                .build();
            println!("{img}");
        }
        Err(e) => eprintln!("could not draw QR code: {e}"),
    }
}
