//! Phone camera → v4l2loopback ("Dotlink Camera"), phone mic → PipeWire source ("Dotlink Mic").

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

pub const CAMERA_LABEL: &str = "Dotlink Camera";
const MIC_SOURCE: &str = "dotlink_mic";

/// Finds the loopback device: the configured path, or the one whose name is [CAMERA_LABEL].
pub fn find_camera(configured: &str) -> Result<PathBuf> {
    if !configured.is_empty() {
        return Ok(PathBuf::from(configured));
    }
    for entry in std::fs::read_dir("/sys/class/video4linux")?.flatten() {
        let name = std::fs::read_to_string(entry.path().join("name")).unwrap_or_default();
        if name.trim() == CAMERA_LABEL {
            return Ok(PathBuf::from("/dev").join(entry.file_name()));
        }
    }
    bail!("no v4l2loopback device named \"{CAMERA_LABEL}\"; run scripts/install.sh --camera")
}

/// An ffmpeg process that decodes MJPEG frames from stdin into the loopback device.
pub struct CameraSink {
    child: Child,
    stdin: ChildStdin,
}

impl CameraSink {
    pub fn start(device: &PathBuf, width: u32, height: u32) -> Result<Self> {
        let filter = format!(
            "scale={width}:{height}:force_original_aspect_ratio=decrease,\
             pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,format=yuv420p"
        );
        let mut child = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-nostdin"])
            .args(["-fflags", "nobuffer", "-flags", "low_delay", "-probesize", "32", "-analyzeduration", "0"])
            .args(["-use_wallclock_as_timestamps", "1", "-f", "jpeg_pipe", "-i", "pipe:0"])
            .args(["-vf", &filter, "-fps_mode", "passthrough", "-f", "v4l2"])
            .arg(device)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("starting ffmpeg")?;
        let stdin = child.stdin.take().unwrap();
        Ok(Self { child, stdin })
    }

    pub async fn frame(&mut self, jpeg: &[u8]) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            bail!("ffmpeg exited: {status}");
        }
        self.stdin.write_all(jpeg).await?;
        Ok(())
    }
}


/// Keeps "Dotlink Camera" fed for the daemon's whole life: phone frames while the phone streams,
/// a "camera off" placeholder at 5 fps otherwise. With `exclusive_caps=1` the device is only
/// listed as a camera (in OBS, browsers) while something writes to it, and apps keep their
/// source when the phone stops. The size is fixed for the same reason: a reader that opened
/// the device at one size shows garbage if the writer switches to another.
#[derive(Clone)]
pub struct CameraHub {
    frames: tokio::sync::mpsc::Sender<axum::body::Bytes>,
}

impl CameraHub {
    pub fn start(configured_device: String, size: (u32, u32)) -> Self {
        let (frames, mut rx) = tokio::sync::mpsc::channel::<axum::body::Bytes>(2);
        tokio::spawn(async move {
            let mut sink: Option<CameraSink> = None;
            let mut current = size;
            let mut placeholder = placeholder_jpeg(current).await;
            let mut last_phone: Option<std::time::Instant> = None;
            let mut warned = false;
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(200));
            // Debug aid: DOTLINK_DUMP_FRAME=1 keeps the latest phone frame in /tmp.
            let dump = std::env::var_os("DOTLINK_DUMP_FRAME").is_some();
            let mut dumped = std::time::Instant::now();
            loop {
                let jpeg = tokio::select! {
                    frame = rx.recv() => {
                        let Some(jpeg) = frame else { break };
                        last_phone = Some(std::time::Instant::now());
                        if dump && dumped.elapsed() > std::time::Duration::from_secs(1) {
                            dumped = std::time::Instant::now();
                            let _ = std::fs::write("/tmp/dotlink-last-frame.jpg", &jpeg);
                        }
                        jpeg
                    }
                    _ = tick.tick() => {
                        if last_phone.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)) {
                            continue;
                        }
                        placeholder.clone()
                    }
                };
                // The placeholder may have been made for another size before the sink started.
                let jpeg = if jpeg.is_empty() { continue } else { jpeg };
                if sink.is_none() {
                    let started = find_camera(&configured_device).and_then(|dev| {
                        // An app that already holds the device keeps the size it opened it with;
                        // writing any other size would show up there as garbage.
                        let want = active_format(&dev).unwrap_or(size);
                        if want != size {
                            tracing::info!("camera: device is held at {}x{}, using that size", want.0, want.1);
                        }
                        Ok((want, CameraSink::start(&dev, want.0, want.1)?))
                    });
                    match started {
                        Ok((want, s)) => {
                            if want != current {
                                current = want;
                                placeholder = placeholder_jpeg(current).await;
                            }
                            sink = Some(s);
                            warned = false;
                        }
                        Err(e) => {
                            if !warned {
                                tracing::warn!("camera: {e:#}");
                                warned = true;
                            }
                            continue;
                        }
                    }
                }
                if dump {
                    let _ = std::fs::write("/tmp/dotlink-written.jpg", &jpeg);
                }
                if let Err(e) = sink.as_mut().unwrap().frame(&jpeg).await {
                    tracing::warn!("camera: {e:#}");
                    sink = None;
                }
            }
        });
        Self { frames }
    }

    /// Queues a phone frame; dropped when ffmpeg is behind (keeps latency low).
    pub fn send(&self, jpeg: axum::body::Bytes) {
        let _ = self.frames.try_send(jpeg);
    }
}

/// The size the loopback device is currently set to (`/sys/.../format` = "YU12:1280x720@30"),
/// or None when no format is set.
fn active_format(dev: &std::path::Path) -> Option<(u32, u32)> {
    let name = dev.file_name()?.to_str()?;
    let text = std::fs::read_to_string(format!("/sys/class/video4linux/{name}/format")).ok()?;
    let size = text.trim().split(':').nth(1)?.split('@').next()?;
    let (w, h) = size.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?)).filter(|&(w, h): &(u32, u32)| w > 0 && h > 0)
}

/// A dark "Dotlink · phone camera off" frame, as JPEG. Plain dark grey if drawtext is missing.
async fn placeholder_jpeg((w, h): (u32, u32)) -> axum::body::Bytes {
    let text = format!(
        "color=c=0x111111:s={w}x{h},drawtext=text='Dotlink · phone camera off':fontcolor=white@0.55:fontsize={}:x=(w-tw)/2:y=(h-th)/2",
        h / 22
    );
    for source in [text, format!("color=c=0x111111:s={w}x{h}")] {
        let out = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", &source])
            .args(["-frames:v", "1", "-f", "mjpeg", "pipe:1"])
            .stdin(Stdio::null())
            .output()
            .await;
        if let Ok(out) = out {
            if out.status.success() && !out.stdout.is_empty() {
                return out.stdout.into();
            }
        }
    }
    axum::body::Bytes::new()
}

/// "Dotlink Mic": a pw-cat process that is itself a PipeWire source node, fed PCM on stdin.
/// No modules or FIFOs, so nothing is left behind when it stops.
pub struct MicSink {
    _child: Child,
    queue: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl MicSink {
    pub fn start(rate: u32, channels: u32) -> Result<Self> {
        let props = format!(
            "{{ media.class=Audio/Source node.name={MIC_SOURCE} node.description=\"Dotlink Mic\" node.autoconnect=false }}"
        );
        let mut child = Command::new("pw-cat")
            .args(["--playback", "--raw", "--format", "s16", "--latency", "40ms"])
            .args(["--rate", &rate.to_string(), "--channels", &channels.to_string()])
            .args(["-P", &props, "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("starting pw-cat")?;
        let mut stdin = child.stdin.take().unwrap();
        // ~200 ms of 20 ms chunks. When PipeWire falls behind, whole chunks are dropped
        // (never partial ones, which would shift every following sample).
        let (queue, mut rx) = tokio::sync::mpsc::channel::<Vec<u8>>(10);
        tokio::spawn(async move {
            while let Some(chunk) = rx.recv().await {
                if stdin.write_all(&chunk).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self { _child: child, queue })
    }

    pub fn write(&mut self, pcm: &[u8]) {
        let _ = self.queue.try_send(pcm.to_vec());
    }
}
