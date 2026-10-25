//! What the features need from Windows: an inbound firewall rule for the port, on a Private
//! network (its fix asks for administrator rights once), the virtual camera, VB-CABLE, adb +
//! scrcpy for Cast (downloaded without admin rights), and notifications left on. Sections come
//! in the same order as on Linux.

use super::super::Check;
use super::hidden_command;
use anyhow::{Result, bail};

/// The port is in the name, so finding the rule by name also checks the port.
fn rule_name(port: u16) -> String {
    format!("Dotlink (TCP {port})")
}

/// Blocking (runs netsh and PowerShell); call it off the async runtime.
pub fn checks(port: u16) -> Vec<Check> {
    let mut out = Vec::new();

    let rule = hidden_command("netsh")
        .args(["advfirewall", "firewall", "show", "rule"])
        .arg(format!("name={}", rule_name(port)))
        .output()
        .is_ok_and(|o| o.status.success());
    out.push(Check {
        id: "firewall".into(),
        feature: "Network".into(),
        title: format!("Firewall allows port {port}/tcp"),
        ok: rule,
        detail: if rule {
            String::new()
        } else {
            "Windows Firewall blocks the phone. Adds an inbound rule for Private and Domain networks.".into()
        },
        fixable: !rule,
    });

    // Allowing Dotlink when Windows asked (on a Public network) adds a rule for the exe that
    // covers Public too; then the profile doesn't matter.
    let public = if exe_allowed_on_public() { Vec::new() } else { public_networks() };
    out.push(Check {
        id: "network_profile".into(),
        feature: "Network".into(),
        title: "Network is Private".into(),
        ok: public.is_empty(),
        detail: if public.is_empty() {
            String::new()
        } else {
            format!(
                "{} is set to Public, where Windows blocks the phone. Switch it to Private in \
                 Settings → Network & internet → (your network) → Network profile type.",
                public.join(", ")
            )
        },
        fixable: false,
    });
    // Remote: SendInput needs no setup.
    out.push(Check {
        id: "input".into(),
        feature: "Remote".into(),
        title: "Virtual mouse and keyboard (SendInput)".into(),
        ok: true,
        detail: "Windows keeps administrator windows (UAC prompts, Task Manager) out of reach.".into(),
        fixable: false,
    });

    // Webcam: the virtual camera's media source, installed where the frame server can load it.
    let camera = super::camera::installed_ok();
    out.push(Check {
        id: "camera".into(),
        feature: "Webcam".into(),
        title: "\"Dotlink Camera\" device".into(),
        ok: camera.is_ok(),
        detail: match &camera {
            Ok(()) => format!("In your app, pick \"{}\" as the camera.", dotlink_camera::shared::FRIENDLY_NAME),
            Err(e) => e.clone(),
        },
        fixable: camera.is_err() && super::camera::os_supported() && super::camera::bundled_dll().is_some(),
    });

    // Microphone: VB-CABLE provides the device apps record from.
    let cable = super::media::find_cable().is_some();
    out.push(Check {
        id: "vb_cable".into(),
        feature: "Microphone".into(),
        title: "VB-CABLE audio driver".into(),
        ok: cable,
        detail: if cable {
            format!("In your app, pick \"{}\" as the microphone.", super::media::CABLE_OUTPUT)
        } else {
            "The phone mic plays into it, and apps use its \"CABLE Output\" as their microphone. \
             Fix opens its download page: unzip, run VBCABLE_Setup_x64.exe as administrator, restart Dotlink."
                .into()
        },
        fixable: !cable,
    });

    // Cast. The scrcpy download includes adb; Cast also downloads it on first use.
    let downloadable = crate::cast::can_install();
    for (id, title) in [("adb", "adb (Android platform tools)"), ("scrcpy", "scrcpy")] {
        let ok = crate::cast::find_tool(id).is_some();
        out.push(Check {
            id: id.into(),
            feature: "Cast".into(),
            title: title.into(),
            ok,
            detail: if ok {
                String::new()
            } else if downloadable {
                "Fix downloads scrcpy with adb (from GitHub). Or install: winget install Genymobile.scrcpy".into()
            } else {
                "Install: winget install Genymobile.scrcpy".into()
            },
            fixable: !ok && downloadable,
        });
    }

    // Notifications: toasts are built in, but can be turned off for everyone or for Dotlink.
    let toasts = toasts_enabled();
    out.push(Check {
        id: "notifications".into(),
        feature: "Notifications".into(),
        title: "Windows notifications".into(),
        ok: toasts,
        detail: if toasts {
            String::new()
        } else {
            "Windows has notifications off for Dotlink. Turn them on in Settings → System → Notifications.".into()
        },
        fixable: false,
    });
    out
}

/// Neither notifications as a whole nor Dotlink's are switched off (a missing value means on).
fn toasts_enabled() -> bool {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let off = |key: &str, value: &str| {
        hkcu.open_subkey(key).and_then(|k| k.get_value::<u32, _>(value)).is_ok_and(|v| v == 0)
    };
    !off(r"Software\Microsoft\Windows\CurrentVersion\PushNotifications", "ToastEnabled")
        && !off(&format!(r"Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\{}", super::APP_ID), "Enabled")
}

/// Names of the connected networks whose profile is Public.
fn public_networks() -> Vec<String> {
    let out = hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-NetConnectionProfile | Where-Object NetworkCategory -eq 'Public' | ForEach-Object Name",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
        }
        _ => Vec::new(),
    }
}

/// An enabled inbound Allow rule for this exe that applies on Public networks.
fn exe_allowed_on_public() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let script = format!(
        "$exe = '{}'; \
         $r = Get-NetFirewallApplicationFilter | Where-Object Program -eq $exe | Get-NetFirewallRule | \
         Where-Object {{ $_.Enabled -eq 'True' -and $_.Direction -eq 'Inbound' -and $_.Action -eq 'Allow' -and \
         ($_.Profile -eq 'Any' -or $_.Profile -match 'Public') }}; if ($r) {{ 'yes' }}",
        exe.display().to_string().replace('\'', "''")
    );
    hidden_command("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
}

pub async fn fix(id: &str, port: u16) -> Result<()> {
    match id {
        "firewall" => add_firewall_rule(port).await,
        "camera" => install_camera().await,
        "adb" | "scrcpy" => crate::cast::install().await,
        // A driver only its own installer can add (and only VB-Audio may hand out).
        "vb_cable" => {
            super::open_path(std::path::Path::new(super::media::VB_CABLE_URL));
            Ok(())
        }
        _ => bail!("nothing to fix for \"{id}\""),
    }
}

pub async fn fix_many(ids: &[String], port: u16) -> Result<()> {
    // One download covers both.
    if ids.iter().any(|id| id == "adb" || id == "scrcpy") {
        crate::cast::install().await?;
    }
    for id in ids.iter().filter(|id| *id != "adb" && *id != "scrcpy") {
        fix(id, port).await?;
    }
    Ok(())
}

/// Installs the camera DLL into Program Files and registers its class under HKLM, as
/// administrator (one UAC prompt). The frame server is then stopped so it lets go of an older
/// DLL; Windows starts it again on the next camera use, with the new one. Apps using the
/// camera lose it for a moment.
async fn install_camera() -> Result<()> {
    let source = super::camera::bundled_dll().ok_or_else(|| anyhow::anyhow!("dotlink_camera.dll is missing next to Dotlink"))?;
    let target = super::camera::installed_dll(&source)?;
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let key = format!(r"HKLM:\SOFTWARE\Classes\CLSID\{}", dotlink_camera::shared::CLSID_STR);
    let script = format!(
        "$ErrorActionPreference = 'Stop'\n\
         $dll = {target}\n\
         New-Item -ItemType Directory -Force (Split-Path $dll) | Out-Null\n\
         Copy-Item -Force {source} $dll\n\
         New-Item -Force {key} | Out-Null\n\
         Set-ItemProperty {key} '(default)' 'Dotlink Camera'\n\
         # Which camera code this is: updates that don't change it need no new install.\n\
         Set-ItemProperty {key} {source_value} {source_hash}\n\
         New-Item -Force {key_inproc} | Out-Null\n\
         Set-ItemProperty {key_inproc} '(default)' $dll\n\
         Set-ItemProperty {key_inproc} 'ThreadingModel' 'Both'\n\
         # The frame server keeps the old build loaded (and its formats) until it restarts.\n\
         Stop-Service FrameServer -Force -ErrorAction SilentlyContinue\n\
         # Older builds: removed when nothing has them loaded any more.\n\
         Get-ChildItem (Split-Path $dll) -Filter 'dotlink_camera*.dll' | Where-Object FullName -ne $dll | \
         Remove-Item -Force -ErrorAction SilentlyContinue\n",
        target = quote(&target.to_string_lossy()),
        source = quote(&source.to_string_lossy()),
        key = quote(&key),
        key_inproc = quote(&format!(r"{key}\InprocServer32")),
        source_value = quote(dotlink_camera::shared::SOURCE_VALUE),
        source_hash = quote(dotlink_camera::shared::SOURCE),
    );
    run_elevated_powershell(&script).await.map_err(|e| anyhow::anyhow!("Dotlink Camera wasn't installed: {e:#}"))?;
    super::camera::reload();
    Ok(())
}

/// Runs `script` in an elevated PowerShell and waits for it (one UAC prompt). Its error, if
/// any, comes back through a file: the elevated process has no console to read.
async fn run_elevated_powershell(script: &str) -> Result<()> {
    // -EncodedCommand: UTF-16LE base64, so the script needs no quoting through two shells.
    use base64::Engine;
    let log = std::env::temp_dir().join(format!("dotlink-setup-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let quoted_log = format!("'{}'", log.to_string_lossy().replace('\'', "''"));
    // `exit 0` on success: a non-terminating error the script silenced (-ErrorAction
    // SilentlyContinue) still leaves $? false, and PowerShell would exit 1 with no message.
    let script = format!(
        "try {{\n{script}\n}} catch {{ $_.Exception.Message | Out-File -Encoding utf8 {quoted_log}; exit 1 }}\nexit 0\n"
    );
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64::engine::general_purpose::STANDARD.encode(utf16);
    let outer = format!(
        "$p = Start-Process powershell.exe -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{encoded}' \
         -Verb RunAs -Wait -WindowStyle Hidden -PassThru; exit $p.ExitCode"
    );
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &outer]).creation_flags(super::CREATE_NO_WINDOW);
    let out = cmd.output().await?;
    if !out.status.success() {
        let reason = std::fs::read_to_string(&log).map(|s| s.trim().trim_start_matches('\u{feff}').to_string());
        let _ = std::fs::remove_file(&log);
        match reason {
            Ok(reason) if !reason.is_empty() => bail!("{reason}"),
            // No message: the UAC prompt was declined (Start-Process failed before running it).
            _ => bail!("administrator rights were refused"),
        }
    }
    Ok(())
}

/// netsh through an elevated PowerShell `Start-Process -Verb RunAs`: one UAC prompt, and
/// `-Wait` so we know when it's done.
async fn add_firewall_rule(port: u16) -> Result<()> {
    let name = rule_name(port);
    let netsh = format!(
        "advfirewall firewall delete rule name=\"{name}\" & netsh advfirewall firewall add rule name=\"{name}\" \
         dir=in action=allow protocol=TCP localport={port} profile=private,domain"
    );
    // cmd runs both netsh calls under the one elevation; PowerShell quotes with ''.
    let script = format!(
        "$p = Start-Process cmd.exe -ArgumentList '/c netsh {}' -Verb RunAs -Wait -WindowStyle Hidden -PassThru; exit $p.ExitCode",
        netsh.replace('\'', "''")
    );
    let mut cmd = tokio::process::Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]).creation_flags(super::CREATE_NO_WINDOW);
    let out = cmd.output().await?;
    if !out.status.success() {
        bail!("the firewall rule wasn't added (administrator rights refused?)");
    }
    Ok(())
}
