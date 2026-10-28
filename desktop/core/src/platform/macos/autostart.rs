//! Start at login: a LaunchAgent, `~/Library/LaunchAgents/dev.dotlink.Dotlink.plist`, running
//! `<exe> --background`. SMAppService.mainApp would list the app itself under Login Items, but
//! starts it without arguments, so it would open the window at every login.
//!
//! System Settings → General → Login Items → "Allow in the Background" can switch the agent off
//! without removing it; launchd records that, and off there counts as off here. Turning the
//! setting on again can't override the user's switch: it says where to turn it back on.

use anyhow::{Context, Result, bail};
use std::path::PathBuf;

const LABEL: &str = "dev.dotlink.Dotlink";

fn plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
}

pub fn enabled() -> bool {
    plist_path().is_some_and(|p| p.is_file()) && !switched_off()
}

pub fn set(on: bool) -> Result<()> {
    let path = plist_path().context("no home folder")?;
    if !on {
        // Not `launchctl bootout`: that would also stop the Dotlink it started at login.
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => return Ok(()),
        }
    }
    let exe = std::env::current_exe()?;
    std::fs::create_dir_all(path.parent().context("no LaunchAgents folder")?)?;
    std::fs::write(&path, plist(&exe.to_string_lossy()))?;
    if switched_off() {
        bail!("turn Dotlink on in System Settings → General → Login Items → Allow in the Background");
    }
    Ok(())
}

/// Only for window managers without autostart (Linux).
pub fn wm_hint() -> Option<String> {
    None
}

/// The user switched the agent off under "Allow in the Background".
fn switched_off() -> bool {
    // SAFETY: getuid can't fail.
    let uid = unsafe { libc::getuid() };
    let Ok(out) = std::process::Command::new("launchctl").args(["print-disabled", &format!("gui/{uid}")]).output()
    else {
        return false;
    };
    let needle = format!("\"{LABEL}\" => ");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().strip_prefix(&needle))
        .any(|state| state == "disabled" || state == "true")
}

fn plist(exe: &str) -> String {
    let exe = exe.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{exe}</string>
		<string>--background</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>ProcessType</key>
	<string>Interactive</string>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
</dict>
</plist>
"#
    )
}
