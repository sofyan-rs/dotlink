//! Phone mic → BlackHole. macOS only adds a microphone through an audio driver, so the free
//! BlackHole driver provides the pair: we play the phone's PCM into "BlackHole 2ch", and apps
//! record from the same device as their microphone.
//!
//! TODO(macos): an own AudioServerPlugIn ("Dotlink Mic") would remove the extra install.

use crate::platform::cpal_sink::CpalSink;
use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait};
use std::time::Duration;

pub const BLACKHOLE_URL: &str = "https://existential.audio/blackhole/";
/// The installer Setup's Fix downloads: the same file and checksum as Homebrew's
/// `blackhole-2ch` cask.
const PKG_VERSION: &str = "0.7.1";
const PKG_SHA256: &str = "57b540f27a3e29c37e310e01bee0fdfab76733087e47f997ef9dccf851400dcf";

/// BlackHole's device (2ch, 16ch or 64ch, whichever is installed) and its name.
pub fn find_blackhole() -> Option<(cpal::Device, String)> {
    cpal::default_host().output_devices().ok()?.find_map(|d| {
        let name = d.description().ok()?.name().to_string();
        name.starts_with("BlackHole").then_some((d, name))
    })
}

/// Plays the phone's 16-bit PCM into BlackHole for as long as it lives.
pub struct MicSink(CpalSink);

impl MicSink {
    pub fn start(rate: u32, channels: u32) -> Result<Self> {
        let (device, _) = find_blackhole().with_context(|| {
            format!(
                "install BlackHole for the phone mic (Setup → Fix, or {BLACKHOLE_URL}): Dotlink plays into it and apps use it as their microphone"
            )
        })?;
        CpalSink::start(device, "BlackHole", rate, channels).map(Self)
    }

    pub fn write(&mut self, pcm: axum::body::Bytes) {
        self.0.write(pcm)
    }
}

/// Downloads BlackHole 2ch's installer, checks it, and runs it with the administrator password
/// macOS asks for (an audio driver goes into /Library), then restarts the audio service
/// (launchd starts it again at once), so the device shows up without a reboot.
pub async fn install_blackhole() -> Result<()> {
    let url = format!("https://existential.audio/downloads/BlackHole2ch-{PKG_VERSION}.pkg");
    tracing::info!("downloading {url}");
    let pkg = tokio::task::spawn_blocking(move || -> Result<std::path::PathBuf> {
        use std::io::Read;
        let agent: ureq::Agent =
            ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(300))).build().into();
        let mut data = Vec::new();
        agent.get(&url).call().context("download")?.into_body().as_reader().take(50 << 20).read_to_end(&mut data)?;
        let got = crate::config::sha256_hex(&data);
        if got != PKG_SHA256 {
            bail!("the download doesn't match its checksum (got {got})");
        }
        let path = std::env::temp_dir().join(format!("BlackHole2ch-{PKG_VERSION}.pkg"));
        std::fs::write(&path, &data)?;
        Ok(path)
    })
    .await??;
    // The path goes in as an argument, quoted by AppleScript, never into the script's text.
    let script = r#"on run argv
do shell script "/usr/sbin/installer -pkg " & quoted form of item 1 of argv & " -target / && (/usr/bin/killall coreaudiod; true)" with prompt "Dotlink wants to install BlackHole, the audio driver the phone mic plays into." with administrator privileges
end run"#;
    let out = tokio::process::Command::new("osascript").arg("-e").arg(script).arg(&pkg).output().await?;
    let _ = std::fs::remove_file(&pkg);
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // -128: the user cancelled the password prompt.
        if err.contains("(-128)") {
            bail!("administrator rights were refused");
        }
        bail!("BlackHole's installer failed: {}", err.trim());
    }
    // The audio service is starting again: the device appears a few seconds later, and
    // Setup checks again as soon as this returns.
    for _ in 0..30 {
        if tokio::task::spawn_blocking(|| find_blackhole().is_some()).await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    bail!("BlackHole is installed; restart Dotlink to use it")
}
