//! Dotlink: link an Android phone with this PC.
//! Files, clipboard, touchpad/keyboard, terminal, phone camera as webcam, phone mic.
//!
//! `dotlink` opens the app (window + tray, the service runs inside it). `dotlink daemon` runs
//! the service alone, without a window. The other subcommands talk to whichever is running.

// Windows: a GUI program, so opening the app shows no console window. The CLI subcommands
// borrow the console of the terminal they were started from (`attach_console`).
#![cfg_attr(all(windows, feature = "gui"), windows_subsystem = "windows")]

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
    Pair {
        /// Also open the link on the phones plugged in over USB (USB debugging on): no scanning.
        #[arg(long)]
        usb: bool,
    },
    /// Send files to the connected phones and linked PCs.
    Send {
        /// Only to this device (name or id); every connected one without it.
        #[arg(long)]
        to: Option<String>,
        /// Report a failure as a desktop notification (Explorer's "Share with Dotlink" has no
        /// console to print it to).
        #[arg(long, hide = true)]
        notify: bool,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Send the current PC clipboard to the connected phones and linked PCs.
    Clip {
        /// Only to this device (name or id).
        #[arg(long)]
        to: Option<String>,
    },
    /// Link this PC to another one: paste the link its `dotlink pair` (or Pair page) shows, or
    /// give its name from `dotlink nearby` and the 6-digit code it shows. The two PCs then share
    /// files and the clipboard.
    Join {
        /// The other PC's `dotlink://pair` link, or its name or id.
        target: String,
        /// The 6-digit code the other PC shows (with a name or id).
        #[arg(long)]
        code: Option<String>,
    },
    /// List the Dotlink PCs on this network (to join one with a code).
    Nearby,
    /// Send this PC's clipboard changes to a linked PC as they happen (`on`), or only on
    /// `dotlink clip` (`off`, the default).
    ClipAuto { device: String, state: OnOff },
    /// List paired phones and linked PCs.
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
    /// Show a phone's screen on the PC (scrcpy, over USB or Wi-Fi). `stop` ends it.
    Cast {
        /// The phone (name or id); without it, the first one connected (`stop`: every cast).
        #[arg(long, global = true)]
        to: Option<String>,
        #[command(subcommand)]
        action: Option<CastCmd>,
    },
    /// Stop the running app or daemon.
    Quit,
    /// Windows: what a notification's `dotlink:` link runs (Open, Show in folder).
    #[command(hide = true)]
    Url { link: String },
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
    attach_console();
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

/// A `windows_subsystem = "windows"` program starts without stdout; started from a terminal,
/// print there (`dotlink devices`, `--help`, logs). Started from the Start menu there is no
/// parent console and nothing changes.
#[cfg(all(windows, feature = "gui"))]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(all(windows, feature = "gui")))]
fn attach_console() {}

/// To the terminal (when there is one) and to `config::log_path()`, so what happened at login,
/// from a notification or the tray can be read afterwards.
fn init_logging() {
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "dotlink=info,dotlink_core=info".into())
    };
    match open_log() {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter())
            .with_ansi(false)
            .with_writer(std::io::stdout.and(std::sync::Mutex::new(file)))
            .init(),
        None => tracing_subscriber::fmt().with_env_filter(filter()).init(),
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parent = dotlink_core::platform::parent_process().unwrap_or_else(|| "?".into());
    tracing::info!("dotlink {} started (pid {}) by {parent} with {args:?}", env!("CARGO_PKG_VERSION"), std::process::id());
}

fn open_log() -> Option<std::fs::File> {
    let path = dotlink_core::config::log_path();
    std::fs::create_dir_all(path.parent()?).ok()?;
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1024 * 1024) {
        let _ = std::fs::rename(&path, path.with_file_name("dotlink.old.log"));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
}

async fn command(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Daemon => unreachable!(),
        Cmd::Pair { usb } => {
            ipc::request(json!({ "cmd": "pair" }), |v| {
                if let Some(uri) = v["uri"].as_str() {
                    print_qr(uri);
                    println!("\nScan this with the Dotlink app (valid for 5 minutes).");
                    println!("Or paste this link in the app:\n{uri}\n");
                    if let Some(code) = v["code"].as_str() {
                        println!(
                            "Another PC can join with this code: {}\n(`dotlink join <this PC> --code {code}`)\n",
                            dotlink_core::pair_code::spaced(code)
                        );
                    }
                    println!("Waiting…");
                    if usb {
                        // A second request while this one waits for the phone to pair.
                        let req = json!({ "cmd": "pair_usb", "uri": uri });
                        tokio::spawn(async move {
                            if let Err(e) = simple(req).await {
                                eprintln!("USB: {e:#}");
                            }
                        });
                    }
                } else if let Some(name) = v["paired"].as_str() {
                    println!("Paired with {name}.");
                } else if let Some(e) = v["error"].as_str() {
                    bail!("{e}");
                }
                Ok(())
            })
            .await
        }
        Cmd::Send { to, notify, files } => {
            let result = async {
                let paths: Vec<String> = files
                    .iter()
                    .map(|p| std::fs::canonicalize(p).map(|p| p.to_string_lossy().into_owned()))
                    .collect::<std::io::Result<_>>()?;
                simple(json!({ "cmd": "send", "paths": paths, "to": to })).await
            }
            .await;
            if notify && let Err(e) = &result {
                dotlink_core::platform::notify("Could not share", &format!("{e:#}"));
            }
            result
        }
        Cmd::Clip { to } => simple(json!({ "cmd": "clip", "to": to })).await,
        Cmd::Join { target, code: None } if target.starts_with("dotlink://") => {
            simple(json!({ "cmd": "join", "uri": target })).await
        }
        Cmd::Join { code: None, .. } => bail!("give the code the other PC shows: --code 123456 (or paste its link)"),
        Cmd::Join { target, code: Some(code) } => {
            // A name: find its id among the PCs on the network.
            let mut id = target.clone();
            ipc::request(json!({ "cmd": "nearby" }), |v| {
                for pc in v["pcs"].as_array().into_iter().flatten() {
                    if pc["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&target)) {
                        id = pc["id"].as_str().unwrap_or_default().to_string();
                    }
                }
                Ok(())
            })
            .await?;
            simple(json!({ "cmd": "join_code", "id": id, "code": code })).await
        }
        Cmd::Nearby => {
            ipc::request(json!({ "cmd": "nearby" }), |v| {
                let pcs = v["pcs"].as_array().cloned().unwrap_or_default();
                if pcs.is_empty() {
                    println!("No other Dotlink PC found on this network.");
                }
                for pc in pcs {
                    let linked = if pc["linked"].as_bool() == Some(true) { "  (linked)" } else { "" };
                    println!("{}  {}{linked}", pc["id"].as_str().unwrap_or(""), pc["name"].as_str().unwrap_or(""));
                }
                Ok(())
            })
            .await
        }
        Cmd::ClipAuto { device, state } => {
            simple(json!({ "cmd": "clipboard_auto", "id": device, "on": matches!(state, OnOff::On) })).await
        }
        Cmd::Unpair { device } => simple(json!({ "cmd": "unpair", "id": device })).await,
        Cmd::Quit => simple(json!({ "cmd": "quit" })).await,
        Cmd::Url { link } => open_link(&link).await,
        Cmd::Cast { to, action: None } => simple(json!({ "cmd": "cast", "on": true, "id": to })).await,
        Cmd::Cast { to, action: Some(CastCmd::Stop) } => {
            simple(json!({ "cmd": "cast", "on": false, "id": to })).await
        }
        Cmd::Cast { to, action: Some(CastCmd::Pair { code }) } => {
            simple(json!({ "cmd": "cast_pair", "code": code, "id": to })).await
        }
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
                    println!("No paired phones or linked PCs. Run `dotlink pair`.");
                }
                for d in devices {
                    let state = if d["online"].as_bool().unwrap_or(false) { "online" } else { "offline" };
                    let kind = d["kind"].as_str().unwrap_or("phone");
                    let battery = match d["battery"]["level"].as_u64() {
                        Some(level) if d["battery"]["charging"].as_bool() == Some(true) => format!("  {level}% charging"),
                        Some(level) => format!("  {level}%"),
                        None => String::new(),
                    };
                    println!(
                        "{}  {}  {kind}  ({state}){battery}",
                        d["id"].as_str().unwrap_or(""),
                        d["name"].as_str().unwrap_or("")
                    );
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

/// Started by Windows, without a console: errors go to a notification. Accept / Decline on an
/// incoming file go to the running service.
#[cfg(windows)]
async fn open_link(link: &str) -> Result<()> {
    let answer = link.strip_prefix("dotlink:accept/").map(|id| (id, true)).or(link.strip_prefix("dotlink:decline/").map(|id| (id, false)));
    let result = match answer {
        Some((id, accept)) => simple(json!({ "cmd": "answer_incoming", "id": id.trim_end_matches('/'), "accept": accept })).await,
        None => dotlink_core::platform::open_link(link),
    };
    if let Err(e) = result {
        dotlink_core::platform::notify("Dotlink", &format!("{e:#}"));
    }
    Ok(())
}

#[cfg(not(windows))]
async fn open_link(_link: &str) -> Result<()> {
    bail!("dotlink: links are only used on Windows")
}
