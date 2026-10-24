//! The USB link: the phone's Dotlink connection over a USB cable instead of Wi-Fi.
//!
//! For every phone plugged in with USB debugging allowed, `adb reverse tcp:<port> tcp:<port>`
//! makes `127.0.0.1:<port>` on the phone reach this PC's server through the cable. The phone
//! tries that address first (the pinned certificate still decides which PC it is), so the
//! same HTTPS/WebSocket link carries everything: files, clipboard, shell, remote, camera, mic.
//! Unplugged, the connection drops and the phone reconnects over Wi-Fi.
//!
//! adb forgets reversals when the phone is unplugged or adb's server restarts (Android
//! Studio, another scrcpy), so each phone's list is checked on every poll and set again when
//! it's missing.

use crate::server::App;
use anyhow::{Result, bail};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(3);

/// The phone app's package, which handles `dotlink://pair` links.
const PACKAGE: &str = "com.dotlink.android";

/// Sends the pairing link `uri` to Dotlink on every phone plugged in with USB debugging
/// allowed, so it pairs without scanning the QR code. Allowing USB debugging for this PC is
/// the trust step. Returns the phones' models.
///
/// `quiet`: the phone asked (`POST /pair/usb`). The link goes as a broadcast that only the
/// asking phone acts on (its nonce is in the link); the others, which can't be told apart
/// here, stay as they are. Otherwise (Pair over USB on the PC) it opens the pairing screen
/// with the link filled in, and the user taps Pair.
pub async fn open_pairing(uri: &str, quiet: bool) -> Result<Vec<String>> {
    if crate::cast::find_tool("adb").is_none() {
        bail!("adb isn't installed: get it in Setup (Cast), or scan the code");
    }
    let phones = crate::cast::usb_phones().await?;
    if phones.is_empty() {
        bail!("No phone on the cable. Plug it in with USB debugging on and allow this PC");
    }
    // `adb shell` hands its arguments to the phone's sh as one line: quote the link's `&`s.
    let uri = uri.replace('\'', "%27");
    let start = if quiet {
        format!("am broadcast -a com.dotlink.android.USB_PAIR -n {PACKAGE}/com.dotlink.link.UsbPairReceiver --es link '{uri}'")
    } else {
        format!("am start -a android.intent.action.VIEW -d '{uri}' {PACKAGE}")
    };
    let mut opened = Vec::new();
    for (serial, model) in phones {
        let installed =
            crate::cast::adb(&["-s", &serial, "shell", "pm", "path", PACKAGE]).await.is_ok_and(|o| o.contains("package:"));
        if !installed {
            tracing::info!("usb: {model} ({serial}) has no Dotlink app");
            continue;
        }
        match crate::cast::adb(&["-s", &serial, "shell", &start]).await {
            Ok(out) if !out.contains("Error") => opened.push(model),
            Ok(out) => tracing::warn!("usb: opening the pairing link on {serial}: {}", out.trim()),
            Err(e) => tracing::warn!("usb: opening the pairing link on {serial}: {e:#}"),
        }
    }
    if opened.is_empty() {
        bail!("Install Dotlink on the phone first");
    }
    Ok(opened)
}

pub async fn watch(app: Arc<App>) {
    // Phones already announced, so the log says it once per plug-in.
    let mut linked: HashSet<String> = HashSet::new();
    loop {
        tokio::time::sleep(POLL).await;
        if !app.config().usb_link || crate::cast::find_tool("adb").is_none() {
            linked.clear();
            continue;
        }
        let Ok(serials) = crate::cast::usb_serials().await else { continue };
        linked.retain(|s| serials.contains(s));
        let spec = format!("tcp:{}", app.port);
        for serial in serials {
            let present = crate::cast::adb(&["-s", &serial, "reverse", "--list"])
                .await
                .is_ok_and(|list| list.lines().any(|l| l.split_whitespace().skip(1).eq([spec.as_str(), spec.as_str()])));
            if present {
                continue;
            }
            match crate::cast::adb(&["-s", &serial, "reverse", &spec, &spec]).await {
                Ok(out) if !out.to_lowercase().contains("error") => {
                    if linked.insert(serial.clone()) {
                        tracing::info!("usb: {serial} can link over the cable");
                    }
                }
                Ok(out) => tracing::debug!("usb: adb reverse on {serial}: {}", out.trim()),
                Err(e) => tracing::debug!("usb: adb reverse on {serial}: {e:#}"),
            }
        }
    }
}
