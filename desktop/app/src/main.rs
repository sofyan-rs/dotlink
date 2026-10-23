//! Dotlink: link an Android phone with this PC.
//! Files, clipboard, touchpad/keyboard, terminal, phone camera as webcam, phone mic.
//!
//! `dotlink` opens the app (window + tray, the service runs inside it). `dotlink daemon` runs
//! the service alone, without a window. The other subcommands talk to whichever is running.

#[cfg(feature = "gui")]
mod gui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use dotlink_core::ipc;
use serde_json::json;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dotlink", version, about = "Link your Android phone with this PC")]
struct Cli {
    /// Start in the tray, without opening the window (used when starting at login).
    #[arg(long)]
    background: bool,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the service without a window (for `exec-once`, servers, or no tray).
    Daemon,
    /// Show a QR code to pair a phone.
    Pair,
    /// Send files to the connected phones.
    Send {
        /// Only to this phone (name or id); every connected phone without it.
        #[arg(long)]
        to: Option<String>,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Send the current PC clipboard to the connected phones.
    Clip {
        /// Only to this phone (name or id).
        #[arg(long)]
        to: Option<String>,
    },
    /// List paired phones.
    Devices,
    /// Forget a paired phone (by name or id).
    Unpair { device: String },
    /// Turn the phone camera (Dotlink Camera) on or off.
    Camera {
        state: OnOff,
        /// The phone to use (name or id); needed when several are connected.
        #[arg(long)]
        to: Option<String>,
    },
    /// Turn the phone mic (Dotlink Mic) on or off.
    Mic {
        state: OnOff,
        /// The phone to use (name or id); needed when several are connected.
        #[arg(long)]
        to: Option<String>,
    },
    /// Show the phone's screen on the PC (scrcpy, over USB or Wi-Fi). `stop` ends it.
    Cast {
        #[command(subcommand)]
        action: Option<CastCmd>,
    },
    /// Stop the running app or daemon.
    Quit,
}

#[derive(Subcommand)]
enum CastCmd {
    /// End the cast.
    Stop,
    /// Pair with the phone's Wireless debugging once (the 6-digit code it shows).
    Pair { code: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging();
    let rt = tokio::runtime::Runtime::new()?;
    match cli.cmd {
        None => open_app(rt, cli.background),
        Some(Cmd::Daemon) => rt.block_on(dotlink_core::service::run(None, None)),
        Some(cmd) => rt.block_on(command(cmd)),
    }
}

#[cfg(feature = "gui")]
fn open_app(rt: tokio::runtime::Runtime, background: bool) -> Result<()> {
    gui::run(rt, background)
}

/// Built without the window: plain `dotlink` runs the service.
#[cfg(not(feature = "gui"))]
fn open_app(rt: tokio::runtime::Runtime, _background: bool) -> Result<()> {
    rt.block_on(dotlink_core::service::run(None, None))
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dotlink=info,dotlink_core=info".into()),
        )
        .init();
}

async fn command(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Daemon => unreachable!(),
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
        Cmd::Send { to, files } => {
            let paths: Vec<String> = files
                .iter()
                .map(|p| std::fs::canonicalize(p).map(|p| p.to_string_lossy().into_owned()))
                .collect::<std::io::Result<_>>()?;
            simple(json!({ "cmd": "send", "paths": paths, "to": to })).await
        }
        Cmd::Clip { to } => simple(json!({ "cmd": "clip", "to": to })).await,
        Cmd::Unpair { device } => simple(json!({ "cmd": "unpair", "id": device })).await,
        Cmd::Quit => simple(json!({ "cmd": "quit" })).await,
        Cmd::Cast { action: None } => simple(json!({ "cmd": "cast", "on": true })).await,
        Cmd::Cast { action: Some(CastCmd::Stop) } => simple(json!({ "cmd": "cast", "on": false })).await,
        Cmd::Cast { action: Some(CastCmd::Pair { code }) } => simple(json!({ "cmd": "cast_pair", "code": code })).await,
        Cmd::Camera { state, to } => {
            simple(json!({ "cmd": "media", "kind": "camera", "on": matches!(state, OnOff::On), "to": to })).await
        }
        Cmd::Mic { state, to } => {
            simple(json!({ "cmd": "media", "kind": "mic", "on": matches!(state, OnOff::On), "to": to })).await
        }
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
