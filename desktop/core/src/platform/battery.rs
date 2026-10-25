//! Reading a laptop's battery from what Linux and macOS print, kept apart from the OS calls so
//! both parsers are tested on every OS. `charging` means "on power", as on the phone: a full
//! battery on its charger counts.

#![allow(dead_code)]

use std::path::Path;

/// Linux: `/sys/class/power_supply`. Batteries of `scope` "Device" (a mouse, a headset) aren't
/// the laptop's. With two batteries (some ThinkPads) the level is their average.
pub(crate) fn from_sysfs(root: &Path) -> Option<(u8, bool)> {
    let read = |dir: &Path, file: &str| std::fs::read_to_string(dir.join(file)).ok().map(|s| s.trim().to_string());
    let mut levels = Vec::new();
    let mut on_power = false;
    for entry in std::fs::read_dir(root).ok()?.flatten() {
        let dir = entry.path();
        match read(&dir, "type").as_deref() {
            Some("Battery") if read(&dir, "scope").as_deref() != Some("Device") => {
                if let Some(level) = read(&dir, "capacity").and_then(|c| c.parse::<u8>().ok()) {
                    levels.push(level.min(100));
                }
                if matches!(read(&dir, "status").as_deref(), Some("Charging" | "Full")) {
                    on_power = true;
                }
            }
            Some("Mains" | "USB") if read(&dir, "online").as_deref() == Some("1") => on_power = true,
            _ => {}
        }
    }
    if levels.is_empty() {
        return None;
    }
    let level = levels.iter().map(|&l| l as u32).sum::<u32>() / levels.len() as u32;
    Some((level as u8, on_power))
}

/// macOS: `pmset -g batt`, for example
///
/// ```text
/// Now drawing from 'AC Power'
///  -InternalBattery-0 (id=4653155)	85%; charging; 1:02 remaining present: true
/// ```
///
/// A Mac without a battery (mini, Studio, iMac) has no `InternalBattery` line.
pub(crate) fn from_pmset(text: &str) -> Option<(u8, bool)> {
    let line = text.lines().find(|l| l.contains("InternalBattery"))?;
    let percent = line.split('%').next()?;
    let digits: String = percent.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    let level: u8 = digits.chars().rev().collect::<String>().parse().ok()?;
    let on_power = text.lines().next().is_some_and(|l| l.contains("'AC Power'"));
    Some((level.min(100), on_power))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pmset() {
        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=4653155)\t85%; charging; 1:02 remaining present: true\n";
        assert_eq!(from_pmset(ac), Some((85, true)));
        let battery = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=4653155)\t7%; discharging; 0:20 remaining present: true\n";
        assert_eq!(from_pmset(battery), Some((7, false)));
        let full = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=1)\t100%; charged; 0:00 remaining present: true\n";
        assert_eq!(from_pmset(full), Some((100, true)));
        assert_eq!(from_pmset("Now drawing from 'AC Power'\n"), None);
    }

    #[test]
    fn reads_sysfs() {
        let root = std::env::temp_dir().join(format!("dotlink-battery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let supply = |name: &str, files: &[(&str, &str)]| {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            for (file, value) in files {
                std::fs::write(dir.join(file), format!("{value}\n")).unwrap();
            }
        };
        supply("AC", &[("type", "Mains"), ("online", "0")]);
        supply("BAT0", &[("type", "Battery"), ("capacity", "80"), ("status", "Discharging")]);
        supply("BAT1", &[("type", "Battery"), ("capacity", "60"), ("status", "Discharging")]);
        supply("hidpp_battery_0", &[("type", "Battery"), ("scope", "Device"), ("capacity", "5")]);
        assert_eq!(from_sysfs(&root), Some((70, false)));
        supply("AC", &[("type", "Mains"), ("online", "1")]);
        assert_eq!(from_sysfs(&root), Some((70, true)));
        std::fs::remove_dir_all(root.join("BAT0")).unwrap();
        std::fs::remove_dir_all(root.join("BAT1")).unwrap();
        assert_eq!(from_sysfs(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
