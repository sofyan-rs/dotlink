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
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(3);

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
