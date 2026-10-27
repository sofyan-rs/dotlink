//! Phone mic → VB-CABLE. Windows has no user-space way to add a microphone (that's a kernel
//! audio driver), so the free VB-CABLE driver provides the pair: we play the phone's PCM into
//! "CABLE Input", and apps record from "CABLE Output" as their microphone.

use crate::platform::cpal_sink::CpalSink;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// The playback side of VB-CABLE.
const CABLE_INPUT: &str = "CABLE Input";
/// What the user picks as the microphone in their app.
pub const CABLE_OUTPUT: &str = "CABLE Output";
pub const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";
/// VB-CABLE's playback device, when the driver is installed.
pub fn find_cable() -> Option<cpal::Device> {
    let host = cpal::default_host();
    host.output_devices()
        .ok()?
        .find(|d| d.description().is_ok_and(|desc| desc.name().contains(CABLE_INPUT)))
}

/// Plays the phone's 16-bit PCM into "CABLE Input" for as long as it lives.
pub struct MicSink(CpalSink);

impl MicSink {
    pub fn start(rate: u32, channels: u32) -> Result<Self> {
        let device = find_cable().with_context(|| {
            format!("install VB-CABLE ({VB_CABLE_URL}) for the phone mic: Dotlink plays into it and apps use \"{CABLE_OUTPUT}\"")
        })?;
        CpalSink::start(device, "VB-CABLE", rate, channels).map(Self)
    }

    pub fn write(&mut self, pcm: axum::body::Bytes) {
        self.0.write(pcm)
    }
}
