//! Start with the desktop session.
//!
//! - An XDG autostart entry: GNOME, KDE, XFCE, Cinnamon, MATE, LXQt and Budgie read it.
//! - Hyprland ignores it, so there we also hook into Hyprland's own config: a file of ours
//!   (`dotlink.lua` or `dotlink.conf`) plus one marked line in the main config that loads it.
//!   Turning the setting off removes both.
//! - Sway, niri and X11 window managers get a hint (`wm_hint`) with the line to add.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Ends every line we add to someone else's config, so we can find and remove it.
const MARK: &str = "Dotlink: start at login";

fn entry_path() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("autostart/dotlink.desktop")
}

fn hypr_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("hypr")
}

pub fn enabled() -> bool {
    entry_path().exists() || hypr_hooked()
}

pub fn set(on: bool) -> Result<()> {
    let path = entry_path();
    if on {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(
            &path,
            format!(
                "[Desktop Entry]\nType=Application\nName=Dotlink\nComment=Link your Android phone\n\
                 Exec={} --background\nIcon=dotlink\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
                exe_path()
            ),
        )?;
    } else if path.exists() {
        std::fs::remove_file(path)?;
    }
    if is_hyprland() || hypr_hooked() {
        set_hyprland(on).context("Hyprland config")?;
    }
    Ok(())
}

/// The command that starts this app: the AppImage itself when this binary runs from one (the
/// binary inside lives in a temporary mount), otherwise this executable. `$APPIMAGE` alone
/// isn't proof: it is inherited by everything started from another AppImage.
pub fn exe_path() -> String {
    let exe = std::env::current_exe().ok();
    let in_appimage = match (std::env::var_os("APPDIR"), &exe) {
        (Some(dir), Some(exe)) => exe.starts_with(dir),
        _ => false,
    };
    if in_appimage && let Ok(appimage) = std::env::var("APPIMAGE") {
        return appimage;
    }
    exe.map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "dotlink".into())
}

/// For desktops we can't set up ourselves: the line to add to their config.
pub fn wm_hint() -> Option<String> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    let exe = exe_path();
    Some(if desktop.contains("hyprland") {
        let file = if hypr_dir().join("hyprland.lua").exists() { "dotlink.lua" } else { "dotlink.conf" };
        format!("Added to your Hyprland config (~/.config/hypr/{file}).")
    } else if desktop.contains("sway") || desktop.contains("i3") {
        format!("Add to your sway/i3 config:  exec {exe} --background")
    } else if desktop.contains("niri") {
        format!("Add to niri's config:  spawn-at-startup \"{exe}\" \"--background\"")
    } else if ["river", "labwc", "wayfire", "bspwm", "awesome", "qtile"].iter().any(|d| desktop.contains(d)) {
        format!("Your desktop may ignore XDG autostart. Add `{exe} --background` to its startup commands.")
    } else {
        return None;
    })
}

fn is_hyprland() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_lowercase().contains("hyprland"))
}

/// Our line in the main Hyprland config, if it's there.
fn hypr_hooked() -> bool {
    ["hyprland.lua", "hyprland.conf"]
        .iter()
        .any(|f| std::fs::read_to_string(hypr_dir().join(f)).is_ok_and(|s| s.contains(MARK)))
}

/// Lua config (Hyprland 0.5x): `dotlink.lua` + `pcall(require, "dotlink")` (pcall, so a
/// deleted file never breaks the config). Classic config: one `exec-once` line.
fn set_hyprland(on: bool) -> Result<()> {
    let dir = hypr_dir();
    let lua = dir.join("hyprland.lua");
    let conf = dir.join("hyprland.conf");
    let command = format!("{} --background", exe_path());
    if lua.exists() {
        let ours = dir.join("dotlink.lua");
        if on {
            std::fs::write(
                &ours,
                format!(
                    "-- Written by Dotlink (Settings → Start at login); turning that off removes it.\n\
                     hl.on(\"hyprland.start\", function()\n  hl.exec_cmd({})\nend)\n",
                    lua_string(&command)
                ),
            )?;
            add_line(&lua, &format!("pcall(require, \"dotlink\") -- {MARK}"))?;
        } else {
            remove_lines(&lua)?;
            if ours.exists() {
                std::fs::remove_file(ours)?;
            }
        }
    } else if conf.exists() {
        if on {
            remove_lines(&conf)?;
            add_line(&conf, &format!("exec-once = {command} # {MARK}"))?;
        } else {
            remove_lines(&conf)?;
        }
    }
    Ok(())
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Appends `line` unless a marked line is already there.
fn add_line(path: &Path, line: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    if text.contains(MARK) {
        return Ok(());
    }
    let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    std::fs::write(path, format!("{text}{sep}{line}\n"))?;
    Ok(())
}

fn remove_lines(path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path)?;
    if !text.contains(MARK) {
        return Ok(());
    }
    let kept: String = text.lines().filter(|l| !l.contains(MARK)).map(|l| format!("{l}\n")).collect();
    std::fs::write(path, kept)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test (they share the process environment): on, on again, off, on a copy of a Lua config.
    #[test]
    fn hyprland_lua_config_round_trip() {
        let root = std::env::temp_dir().join(format!("dotlink-autostart-{}", std::process::id()));
        let hypr = root.join("hypr");
        std::fs::create_dir_all(&hypr).unwrap();
        let original = "require(\"config.autostart\")\n";
        std::fs::write(hypr.join("hyprland.lua"), original).unwrap();
        // SAFETY: the only test touching the environment.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &root);
            std::env::set_var("XDG_CURRENT_DESKTOP", "Hyprland");
        }

        set(true).unwrap();
        set(true).unwrap();
        let main = std::fs::read_to_string(hypr.join("hyprland.lua")).unwrap();
        assert_eq!(main.matches(MARK).count(), 1, "{main}");
        assert!(main.starts_with(original));
        let ours = std::fs::read_to_string(hypr.join("dotlink.lua")).unwrap();
        assert!(ours.contains("hl.exec_cmd(") && ours.contains("--background"), "{ours}");
        assert!(enabled());

        set(false).unwrap();
        assert_eq!(std::fs::read_to_string(hypr.join("hyprland.lua")).unwrap(), original);
        assert!(!hypr.join("dotlink.lua").exists());
        assert!(!enabled());
        std::fs::remove_dir_all(root).unwrap();
    }
}
