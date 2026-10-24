//! Phone mic → VB-CABLE. Windows has no user-space way to add a microphone (that's a kernel
//! audio driver), so the free VB-CABLE driver provides the pair: we play the phone's PCM into
//! "CABLE Input", and apps record from "CABLE Output" as their microphone.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The playback side of VB-CABLE.
const CABLE_INPUT: &str = "CABLE Input";
/// What the user picks as the microphone in their app.
pub const CABLE_OUTPUT: &str = "CABLE Output";
pub const VB_CABLE_URL: &str = "https://vb-audio.com/Cable/";
/// Audio allowed to wait for the device before the oldest is dropped, like the Linux backlog
/// of a few 10 ms chunks: after a Wi-Fi stall the mic catches up instead of staying late.
const MAX_BACKLOG_MS: usize = 60;

/// VB-CABLE's playback device, when the driver is installed.
pub fn find_cable() -> Option<cpal::Device> {
    let host = cpal::default_host();
    host.output_devices()
        .ok()?
        .find(|d| d.description().is_ok_and(|desc| desc.name().contains(CABLE_INPUT)))
}

/// Plays the phone's 16-bit PCM into "CABLE Input" for as long as it lives.
pub struct MicSink {
    shared: Arc<Mutex<Shared>>,
    /// Dropping it ends the thread that owns the audio stream (cpal streams can't move threads).
    _stop: std::sync::mpsc::Sender<()>,
}

struct Shared {
    /// Interleaved samples in the device's channel count and rate.
    queue: VecDeque<f32>,
    max: usize,
    convert: Converter,
}

impl MicSink {
    pub fn start(rate: u32, channels: u32) -> Result<Self> {
        let device = find_cable().with_context(|| {
            format!("install VB-CABLE ({VB_CABLE_URL}) for the phone mic: Dotlink plays into it and apps use \"{CABLE_OUTPUT}\"")
        })?;
        let supported = device.default_output_config().context("VB-CABLE's audio format")?;
        let format = supported.sample_format();
        let config = supported.config();
        let out_channels = config.channels as usize;
        let shared = Arc::new(Mutex::new(Shared {
            queue: VecDeque::new(),
            max: config.sample_rate as usize * out_channels * MAX_BACKLOG_MS / 1000,
            convert: Converter::new(rate, channels.max(1) as usize, config.sample_rate, out_channels),
        }));

        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let (ready_tx, ready) = std::sync::mpsc::channel::<Result<()>>();
        let feed = shared.clone();
        std::thread::Builder::new().name("dotlink-mic".into()).spawn(move || {
            let stream = match build_stream(&device, &config, format, feed) {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(stream.play().map_err(|e| anyhow!("starting VB-CABLE playback: {e}")));
            // Until MicSink is dropped (the sender goes away).
            let _ = stopped.recv();
            drop(stream);
        })?;
        ready.recv().context("the mic thread stopped")??;
        tracing::info!("mic: playing into {CABLE_INPUT} ({} Hz, {out_channels} ch, {format:?})", config.sample_rate);
        Ok(Self { shared, _stop: stop })
    }

    pub fn write(&mut self, pcm: axum::body::Bytes) {
        let mut shared = self.shared.lock().unwrap();
        let Shared { queue, max, convert } = &mut *shared;
        convert.push(&pcm, queue);
        // Whole frames only: dropping part of one would swap the channels from then on.
        let frame = convert.out_channels;
        let excess = queue.len().saturating_sub(*max);
        queue.drain(..excess.div_ceil(frame) * frame);
    }
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    shared: Arc<Mutex<Shared>>,
) -> Result<cpal::Stream> {
    fn build<T: cpal::SizedSample + cpal::FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        shared: Arc<Mutex<Shared>>,
    ) -> Result<cpal::Stream, cpal::Error> {
        device.build_output_stream(
            *config,
            move |out: &mut [T], _| {
                let mut shared = shared.lock().unwrap();
                for sample in out.iter_mut() {
                    // Silence when the phone is behind; never wait in the audio callback.
                    *sample = T::from_sample(shared.queue.pop_front().unwrap_or(0.0));
                }
            },
            |e| tracing::warn!("mic: {e}"),
            None,
        )
    }
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(device, config, shared),
        cpal::SampleFormat::I16 => build::<i16>(device, config, shared),
        cpal::SampleFormat::I32 => build::<i32>(device, config, shared),
        other => return Err(anyhow!("VB-CABLE uses {other:?} samples, which Dotlink can't play")),
    };
    stream.map_err(|e| anyhow!("opening VB-CABLE: {e}"))
}

/// 16-bit PCM at the phone's rate and channel count → f32 at the device's, by linear
/// interpolation. Mono is copied to every channel; more channels than the device has are mixed
/// down.
struct Converter {
    in_channels: usize,
    out_channels: usize,
    /// Input frames per output frame.
    step: f64,
    /// Position of the next output frame, in input frames after `prev`.
    pos: f64,
    /// The last input frame of the previous chunk (interpolation spans chunks).
    prev: Vec<f32>,
}

impl Converter {
    fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Self {
        Self {
            in_channels,
            out_channels,
            step: in_rate as f64 / out_rate as f64,
            pos: 0.0,
            prev: vec![0.0; out_channels],
        }
    }

    /// One input frame, already in the output's channel layout.
    fn frame(&self, samples: &[i16]) -> Vec<f32> {
        let x: Vec<f32> = samples.iter().map(|&s| s as f32 / 32768.0).collect();
        match (self.in_channels, self.out_channels) {
            (i, o) if i == o => x,
            (1, o) => vec![x[0]; o],
            (_, 1) => vec![x.iter().sum::<f32>() / x.len() as f32],
            // Stereo and up: the first channels as they are, the rest silent.
            (_, o) => (0..o).map(|c| x.get(c).copied().unwrap_or(0.0)).collect(),
        }
    }

    fn push(&mut self, pcm: &[u8], out: &mut VecDeque<f32>) {
        let samples: Vec<i16> = pcm.as_chunks::<2>().0.iter().map(|&b| i16::from_le_bytes(b)).collect();
        for chunk in samples.chunks_exact(self.in_channels) {
            let next = self.frame(chunk);
            // Output frames that fall between `prev` (0.0) and `next` (1.0).
            while self.pos < 1.0 {
                let t = self.pos as f32;
                out.extend(self.prev.iter().zip(&next).map(|(a, b)| a + (b - a) * t));
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn same_rate_mono_to_stereo() {
        let mut c = Converter::new(48000, 1, 48000, 2);
        let mut out = VecDeque::new();
        c.push(&pcm(&[16384, -16384]), &mut out);
        // One frame behind: each output frame interpolates up to the newest input frame.
        assert_eq!(Vec::from(out), vec![0.0, 0.0, 0.5, 0.5]);
    }

    #[test]
    fn upsampling_keeps_the_rate() {
        let mut c = Converter::new(16000, 1, 48000, 1);
        let mut out = VecDeque::new();
        c.push(&pcm(&vec![1000; 160]), &mut out); // 10 ms
        assert_eq!(out.len(), 480);
        c.push(&pcm(&vec![1000; 160]), &mut out);
        assert_eq!(out.len(), 960);
    }

    #[test]
    fn downsampling_keeps_the_rate() {
        let mut c = Converter::new(48000, 2, 44100, 2);
        let mut out = VecDeque::new();
        for _ in 0..100 {
            c.push(&pcm(&vec![0; 960]), &mut out); // 10 ms of stereo
        }
        // One second in, one second (± a frame) out.
        assert!((out.len() as i64 / 2 - 44100).abs() <= 1, "{}", out.len());
    }
}
