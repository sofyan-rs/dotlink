//! What the features need from the system, and one-click fixes through pkexec (the desktop's
//! polkit agent asks for the admin password). Replaces most of what scripts/install.sh checks.

use super::super::Check;
use super::clipboard::which;
use anyhow::{Context, Result, bail};
use std::process::Command;

const UINPUT_RULE: &str = "/etc/udev/rules.d/70-dotlink-uinput.rules";

/// Blocking (runs a few small commands); call it off the async runtime.
pub fn checks(port: u16) -> Vec<Check> {
    let mut out = Vec::new();
    let mut check = |id: &str, feature: &str, title: &str, ok: bool, detail: String, fixable: bool| {
        out.push(Check { id: id.into(), feature: feature.into(), title: title.into(), ok, detail, fixable })
    };

    // Network: only firewalld can be checked without root.
    if firewalld_running() {
        let open = firewalld_allows(port);
        check(
            "firewall",
            "Network",
            &format!("Firewall allows port {port}/tcp and mDNS"),
            open,
            if open {
                String::new()
            } else {
                "firewalld blocks the phone. Opens the port and mDNS in the permanent config.".into()
            },
            !open,
        );
    } else if which("ufw") {
        check(
            "ufw",
            "Network",
            "Firewall (ufw)",
            true,
            format!("If ufw is active:  sudo ufw allow {port}/tcp && sudo ufw allow 5353/udp"),
            false,
        );
    }

    // Remote
    let uinput = std::fs::OpenOptions::new().write(true).open("/dev/uinput").is_ok();
    check(
        "uinput",
        "Remote",
        "Virtual mouse and keyboard (/dev/uinput)",
        uinput,
        if uinput {
            String::new()
        } else {
            "Adds a udev rule that lets you (not every user) create input devices.".into()
        },
        !uinput,
    );

    // Webcam
    let ffmpeg = which("ffmpeg");
    check(
        "ffmpeg",
        "Webcam",
        "ffmpeg",
        ffmpeg,
        if ffmpeg { String::new() } else { install_hint("ffmpeg") },
        !ffmpeg && can_install("ffmpeg"),
    );
    let has_cam = camera_exists();
    let module = has_cam || module_available("v4l2loopback");
    check(
        "v4l2loopback",
        "Webcam",
        "v4l2loopback kernel module",
        module,
        if module { String::new() } else { install_hint("v4l2loopback") },
        !module && can_install("v4l2loopback"),
    );
    check(
        "camera",
        "Webcam",
        "\"Dotlink Camera\" device",
        has_cam,
        if has_cam {
            format!("In your app, pick \"{}\" as the camera.", super::media::CAMERA_LABEL)
        } else {
            "Creates the device now and at every boot.".into()
        },
        !has_cam && module,
    );

    // Microphone
    let pw = which("pw-cat");
    check(
        "pw-cat",
        "Microphone",
        "PipeWire (pw-cat)",
        pw,
        if pw { "In your app, pick \"Dotlink Mic\" as the microphone.".into() } else { install_hint("pw-cat") },
        !pw && can_install("pw-cat"),
    );

    // Cast (scrcpy over adb). Fedora has adb, but scrcpy only from COPR.
    // Dotlink can download scrcpy (with adb) itself; Cast also does it on first use.
    let downloadable = crate::cast::can_install();
    let missing = |what: &str| {
        if downloadable {
            format!("Fix downloads scrcpy with adb (from GitHub). Or {}", install_hint(what).to_lowercase())
        } else {
            install_hint(what)
        }
    };
    let adb = crate::cast::find_tool("adb").is_some();
    check(
        "adb",
        "Cast",
        "adb (Android platform tools)",
        adb,
        if adb { String::new() } else { missing("adb") },
        !adb && downloadable,
    );
    let scrcpy = crate::cast::find_tool("scrcpy").is_some();
    check(
        "scrcpy",
        "Cast",
        "scrcpy",
        scrcpy,
        if scrcpy { String::new() } else { missing("scrcpy") },
        !scrcpy && downloadable,
    );

    // Clipboard: wl-clipboard only matters on compositors with data-control (not GNOME).
    let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_lowercase().contains("gnome"));
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && !gnome {
        let wl = which("wl-copy") && which("wl-paste");
        check(
            "wl-clipboard",
            "Clipboard",
            "wl-clipboard",
            wl,
            if wl { String::new() } else { install_hint("wl-copy") },
            !wl && can_install("wl-copy"),
        );
    }

    // Media: what plays here, on the phone's Media card (MPRIS).
    let playerctl = which("playerctl");
    check(
        "playerctl",
        "Media",
        "playerctl",
        playerctl,
        if playerctl { String::new() } else { install_hint("playerctl") },
        !playerctl && can_install("playerctl"),
    );

    // Notifications
    let notify = which("notify-send");
    check(
        "notify-send",
        "Notifications",
        "notify-send",
        notify,
        if notify { String::new() } else { install_hint("notify-send") },
        !notify && can_install("notify-send"),
    );

    out
}

/// Fixes the check `id`. The error says what to run by hand when it can't.
pub async fn fix(id: &str, port: u16) -> Result<()> {
    fix_many(&[id.to_string()], port).await
}

/// Fixes several checks with one password prompt: missing packages in a single install
/// command, then the system changes. scrcpy/adb are downloaded without root first.
pub async fn fix_many(ids: &[String], port: u16) -> Result<()> {
    if ids.iter().any(|id| id == "scrcpy" || id == "adb") {
        crate::cast::install().await?;
    }
    match root_script(package_manager(), ids, port, camera_exists()) {
        Some(script) => run_as_root(&script).await,
        None => Ok(()),
    }
}

/// The one root script for `ids`: install their packages, then each system change.
fn root_script(pm: Pm, ids: &[String], port: u16, have_camera: bool) -> Option<String> {
    let packages: Vec<&str> = ids.iter().filter_map(|id| package(pm, tool_of(id))).collect();
    let mut steps = Vec::new();
    if !packages.is_empty() {
        steps.push(install_command(pm, &packages));
    }
    for id in ids {
        steps.push(match id.as_str() {
            "uinput" => format!(
                "echo 'KERNEL==\"uinput\", TAG+=\"uaccess\"' > {UINPUT_RULE} && \
                 echo uinput > /etc/modules-load.d/dotlink-uinput.conf && \
                 modprobe uinput && udevadm control --reload && udevadm trigger --name-match=uinput"
            ),
            // Also right after installing v4l2loopback in the same run.
            "camera" | "v4l2loopback" if id == "camera" || !have_camera => format!(
                "cat > /etc/systemd/system/dotlink-camera.service <<'EOF'\n{CAMERA_SERVICE}EOF\n\
                 systemctl daemon-reload && systemctl enable --now dotlink-camera.service"
            ),
            "firewall" => {
                format!("firewall-cmd --permanent --add-port={port}/tcp --add-service=mdns && firewall-cmd --reload")
            }
            _ => continue,
        });
    }
    // "camera" and "v4l2loopback" both add the camera service.
    let mut seen = std::collections::HashSet::new();
    steps.retain(|step| seen.insert(step.clone()));
    // Stop at the first failure (a package that isn't in the repos, say).
    (!steps.is_empty()).then(|| format!("set -e\n{}", steps.join("\n")))
}

async fn run_as_root(script: &str) -> Result<()> {
    if !which("pkexec") {
        bail!("pkexec is missing. Run this as root:\n{script}");
    }
    let out =
        tokio::process::Command::new("pkexec").args(["sh", "-c", script]).output().await.context("starting pkexec")?;
    match out.status.code() {
        Some(0) => Ok(()),
        // 126: the password dialog was dismissed, 127: no polkit agent (or not authorized).
        Some(126) => bail!("cancelled"),
        Some(127) => {
            bail!("no polkit agent to ask for the password (on Hyprland: hyprpolkitagent). Run this as root:\n{script}")
        }
        _ => {
            let err = String::from_utf8_lossy(&out.stderr);
            // Package managers are chatty; the last lines say what went wrong.
            let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
            bail!("{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
        }
    }
}

/// The command a check's package provides (`wl-clipboard` → `wl-copy`).
fn tool_of(id: &str) -> &str {
    match id {
        "wl-clipboard" => "wl-copy",
        other => other,
    }
}

/// The package manager can install what provides `tool` (and pkexec can run it).
fn can_install(tool: &str) -> bool {
    which("pkexec") && package(package_manager(), tool).is_some()
}

/// Non-interactive install, run as root.
fn install_command(pm: Pm, packages: &[&str]) -> String {
    let list = packages.join(" ");
    match pm {
        Pm::Dnf => format!("dnf install -y {list}"),
        Pm::Apt => format!("DEBIAN_FRONTEND=noninteractive apt-get install -y {list}"),
        Pm::Pacman => format!("pacman -S --needed --noconfirm {list}"),
        Pm::Zypper => format!("zypper --non-interactive install {list}"),
        Pm::Other => String::new(),
    }
}

/// Recreates "Dotlink Camera" at every boot.
const CAMERA_SERVICE: &str = r#"[Unit]
Description=Dotlink Camera (v4l2loopback device)
After=systemd-modules-load.service

[Service]
Type=oneshot
ExecStartPre=/sbin/modprobe v4l2loopback
ExecStart=/bin/sh -c 'grep -qx "Dotlink Camera" /sys/class/video4linux/*/name 2>/dev/null || v4l2loopback-ctl add -n "Dotlink Camera" -x 1'
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
"#;

fn camera_exists() -> bool {
    super::find_camera("").is_ok()
}

fn module_available(name: &str) -> bool {
    Command::new("modinfo").arg(name).output().is_ok_and(|o| o.status.success())
}

fn firewalld_running() -> bool {
    which("firewall-cmd") && Command::new("firewall-cmd").arg("--state").output().is_ok_and(|o| o.status.success())
}

fn firewalld_allows(port: u16) -> bool {
    let query = Command::new("firewall-cmd").arg(format!("--query-port={port}/tcp")).output();
    if query.is_ok_and(|o| o.status.success()) {
        return true;
    }
    // Fedora Workstation's default zone opens every high port.
    Command::new("firewall-cmd")
        .arg("--list-ports")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("1025-65535/tcp"))
}

#[derive(Clone, Copy)]
enum Pm {
    Dnf,
    Apt,
    Pacman,
    Zypper,
    Other,
}

fn package_manager() -> Pm {
    if which("dnf") {
        Pm::Dnf
    } else if which("apt-get") {
        Pm::Apt
    } else if which("pacman") {
        Pm::Pacman
    } else if which("zypper") {
        Pm::Zypper
    } else {
        Pm::Other
    }
}

/// The package(s) that provide the command `what` on this distro; None for the ones we don't
/// install this way (scrcpy and adb are downloaded, the rest aren't packages).
fn package(pm: Pm, what: &str) -> Option<&'static str> {
    if matches!(pm, Pm::Other) {
        return None;
    }
    Some(match (pm, what) {
        (_, "wl-copy") => "wl-clipboard",
        (_, "playerctl") => "playerctl",
        (Pm::Apt, "notify-send") => "libnotify-bin",
        (_, "notify-send") => "libnotify",
        (Pm::Apt, "pw-cat") => "pipewire-bin",
        (Pm::Dnf, "pw-cat") => "pipewire-utils",
        (_, "pw-cat") => "pipewire",
        (Pm::Dnf, "ffmpeg") => "ffmpeg-free",
        (_, "ffmpeg") => "ffmpeg",
        // Fedora: from RPM Fusion; the install fails with a clear message without it.
        (Pm::Dnf, "v4l2loopback") => "v4l2loopback",
        (Pm::Apt | Pm::Pacman, "v4l2loopback") => "v4l2loopback-dkms v4l2loopback-utils",
        (Pm::Zypper, "v4l2loopback") => "v4l2loopback-kmp-default v4l2loopback-utils",
        _ => return None,
    })
}

/// "Install: sudo dnf install …" for the package that provides `what` on this distro.
fn install_hint(what: &str) -> String {
    let pm = package_manager();
    let pkg = match (pm, what) {
        (Pm::Dnf | Pm::Pacman | Pm::Zypper, "adb") => "android-tools",
        (Pm::Apt, "adb") => "adb",
        _ => package(pm, what).unwrap_or(what),
    };
    let cmd = match pm {
        Pm::Dnf => "sudo dnf install",
        Pm::Apt => "sudo apt install",
        Pm::Pacman => "sudo pacman -S --needed",
        Pm::Zypper => "sudo zypper install",
        Pm::Other => return format!("Install the {pkg} package."),
    };
    if let (Pm::Dnf, "scrcpy") = (pm, what) {
        return "Install: sudo dnf copr enable zeno/scrcpy && sudo dnf install scrcpy".into();
    }
    let note = match (pm, what) {
        (Pm::Dnf, "v4l2loopback") => " (from RPM Fusion)",
        _ => "",
    };
    format!("Install: {cmd} {pkg}{note}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn one_install_command_for_all_packages() {
        let script =
            root_script(Pm::Dnf, &ids(&["ffmpeg", "pw-cat", "wl-clipboard", "notify-send"]), 47390, true).unwrap();
        assert!(script.contains("dnf install -y ffmpeg-free pipewire-utils wl-clipboard libnotify"), "{script}");
        assert_eq!(script.matches("dnf install").count(), 1);
        let apt = root_script(Pm::Apt, &ids(&["pw-cat", "v4l2loopback"]), 47390, true).unwrap();
        assert!(apt.contains("apt-get install -y pipewire-bin v4l2loopback-dkms v4l2loopback-utils"), "{apt}");
        let pacman = root_script(Pm::Pacman, &ids(&["ffmpeg"]), 47390, true).unwrap();
        assert!(pacman.contains("pacman -S --needed --noconfirm ffmpeg"), "{pacman}");
        let zypper = root_script(Pm::Zypper, &ids(&["notify-send"]), 47390, true).unwrap();
        assert!(zypper.contains("zypper --non-interactive install libnotify"), "{zypper}");
    }

    #[test]
    fn camera_module_then_device_once() {
        let script = root_script(Pm::Dnf, &ids(&["v4l2loopback", "camera"]), 47390, false).unwrap();
        let install = script.find("dnf install -y v4l2loopback").unwrap();
        let service = script.find("dotlink-camera.service <<").unwrap();
        assert!(install < service);
        assert_eq!(script.matches("dotlink-camera.service <<").count(), 1);
    }

    #[test]
    fn downloads_need_no_root() {
        assert!(root_script(Pm::Dnf, &ids(&["scrcpy", "adb"]), 47390, true).is_none());
        let other = root_script(Pm::Other, &ids(&["ffmpeg"]), 47390, true);
        assert!(other.is_none());
    }

    #[test]
    fn script_is_valid_shell() {
        let script =
            root_script(Pm::Dnf, &ids(&["uinput", "ffmpeg", "v4l2loopback", "camera", "firewall"]), 47390, false)
                .unwrap();
        let ok = std::process::Command::new("sh").args(["-n", "-c", &script]).status().unwrap().success();
        assert!(ok, "{script}");
    }
}
