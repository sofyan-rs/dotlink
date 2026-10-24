//! Start at login: value `Dotlink` under HKCU\…\CurrentVersion\Run.
//!
//! Task Manager's Startup tab can turn the entry off without removing it; it records that
//! under `Explorer\StartupApproved\Run`. Off there counts as off here, and turning the setting
//! on clears it again.

use anyhow::Result;
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "Dotlink";

pub fn enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let present = hkcu.open_subkey_with_flags(RUN, KEY_READ).and_then(|k| k.get_value::<String, _>(NAME)).is_ok();
    // First byte 2 = enabled, 3 = disabled in Task Manager.
    let disabled = hkcu
        .open_subkey_with_flags(APPROVED, KEY_READ)
        .and_then(|k| k.get_raw_value(NAME))
        .is_ok_and(|v| v.bytes.first().is_some_and(|b| b & 1 == 1));
    present && !disabled
}

pub fn set(on: bool) -> Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu.create_subkey(RUN)?;
    if on {
        let exe = std::env::current_exe()?;
        run.set_value(NAME, &format!("\"{}\" --background", exe.display()))?;
    } else {
        match run.delete_value(NAME) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    if let Ok(approved) = hkcu.open_subkey_with_flags(APPROVED, KEY_SET_VALUE) {
        let _ = approved.delete_value(NAME);
    }
    Ok(())
}

/// Only for window managers without autostart (Linux).
pub fn wm_hint() -> Option<String> {
    None
}
