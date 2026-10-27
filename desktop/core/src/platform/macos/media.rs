//! Phone mic → BlackHole. macOS only adds a microphone through an audio driver, so the free
//! BlackHole driver provides the pair: we play the phone's PCM into "BlackHole 2ch", and apps
//! record from the same device as their microphone.
//!
//! TODO(macos): an own AudioServerPlugIn ("Dotlink Mic") would remove the extra install.

use crate::platform::cpal_sink::CpalSink;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};

pub const BLACKHOLE_URL: &str = "https://existential.audio/blackhole/";
pub const BLACKHOLE_BREW: &str = "brew install blackhole-2ch";

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
                "install BlackHole for the phone mic ({BLACKHOLE_BREW}, or {BLACKHOLE_URL}): Dotlink plays into it and apps use it as their microphone"
            )
        })?;
        CpalSink::start(device, "BlackHole", rate, channels).map(Self)
    }

    pub fn write(&mut self, pcm: axum::body::Bytes) {
        self.0.write(pcm)
    }
}
