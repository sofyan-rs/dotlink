//! What the features need from Windows: an inbound firewall rule for the port, on a Private
//! network. The fix asks for administrator rights once (UAC).

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
        title: format!("Windows Firewall allows port {port}"),
        ok: rule,
        detail: if rule {
            String::new()
        } else {
            "Adds an inbound rule for Private and Domain networks, so the phone can connect. \
             Windows asks for administrator rights once."
                .into()
        },
        fixable: !rule,
    });

    let public = public_networks();
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
    out
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

pub async fn fix(id: &str, port: u16) -> Result<()> {
    match id {
        "firewall" => add_firewall_rule(port).await,
        _ => bail!("nothing to fix for \"{id}\""),
    }
}

pub async fn fix_many(ids: &[String], port: u16) -> Result<()> {
    for id in ids {
        fix(id, port).await?;
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
