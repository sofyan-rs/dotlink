//! Phone camera → v4l2loopback ("Dotlink Camera"), phone mic → PipeWire source ("Dotlink Mic").

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

pub const CAMERA_LABEL: &str = "Dotlink Camera";
const MIC_SOURCE: &str = "dotlink_mic";
/// Phone mic chunks (10 ms each) allowed to wait for pw-cat before older ones are dropped.
const MIC_BACKLOG: usize = 3;

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
    bail!("no v4l2loopback device named \"{CAMERA_LABEL}\"; set it up in Dotlink → Setup")
}

/// An ffmpeg process that decodes MJPEG frames from stdin into the loopback device.
///
/// Frames go in as multipart JPEG with a Content-Length each: `jpeg_pipe` only knows where a
/// frame ends once the next one starts, and with ffmpeg's default threading the device ran
/// three frames behind the phone (100 ms at 30 fps). This way it is one.
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
            .args(["-use_wallclock_as_timestamps", "1", "-threads", "1", "-f", "mpjpeg", "-i", "pipe:0"])
            .args(["-vf", &filter, "-fps_mode", "passthrough", "-threads", "1", "-f", "v4l2"])
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
        let header = format!("--dotlink\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", jpeg.len());
        self.stdin.write_all(header.as_bytes()).await?;
        self.stdin.write_all(jpeg).await?;
        self.stdin.write_all(b"\r\n").await?;
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
    /// The newest phone frame. A frame still waiting when the next arrives is replaced:
    /// writing it late would only delay everything after it.
    frames: std::sync::Arc<tokio::sync::watch::Sender<axum::body::Bytes>>,
}

impl CameraHub {
    pub fn start(configured_device: String, size: (u32, u32)) -> Self {
        let (frames, mut rx) = tokio::sync::watch::channel(axum::body::Bytes::new());
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
                    changed = rx.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        let jpeg = rx.borrow_and_update().clone();
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
        Self { frames: std::sync::Arc::new(frames) }
    }

    /// Hands a phone frame to ffmpeg, replacing one that is still waiting (keeps latency low).
    pub fn send(&self, jpeg: axum::body::Bytes) {
        self.frames.send_replace(jpeg);
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
    queue: tokio::sync::mpsc::Sender<axum::body::Bytes>,
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
        // A pipe holds 64 KB by default: ~680 ms of 48 kHz mono, and once a Wi-Fi hiccup fills
        // it the delay never shrinks (we write as fast as pw-cat plays). One page keeps it short.
        {
            use std::os::fd::AsRawFd;
            // SAFETY: plain fcntl on a pipe fd we own; failing just keeps the default size.
            unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_SETPIPE_SZ, 4096) };
        }
        // Chunks that pile up after a stall are dropped down to a few (whole chunks only:
        // partial ones would shift every following sample), so the mic catches up again.
        let (queue, mut rx) = tokio::sync::mpsc::channel::<axum::body::Bytes>(16);
        tokio::spawn(async move {
            while let Some(mut chunk) = rx.recv().await {
                while rx.len() > MIC_BACKLOG {
                    match rx.try_recv() {
                        Ok(newer) => chunk = newer,
                        Err(_) => break,
                    }
                }
                if stdin.write_all(&chunk).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self { _child: child, queue })
    }

    pub fn write(&mut self, pcm: axum::body::Bytes) {
        let _ = self.queue.try_send(pcm);
    }
}
