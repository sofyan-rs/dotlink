//! Phone camera → OBS → "OBS Virtual Camera". A camera every app sees is a CoreMediaIO Camera
//! Extension, which macOS only installs from an app signed with a Developer ID and an Apple
//! entitlement. OBS ships one, so Dotlink serves the phone's frames to it instead: an MJPEG
//! stream on 127.0.0.1, which OBS reads as a Media Source and passes on through its virtual
//! camera.
//!
//! The stream's address holds a random token kept in the config folder, so it stays the same
//! across restarts (OBS keeps the source) and other users and programs on the Mac can't guess
//! it. Only the newest frame matters: a reader that falls behind skips to it.
//!
//! TODO(macos): a Camera Extension of our own once there's a Developer ID (MACOS_PLAN.md).

use anyhow::Result;
use axum::body::Bytes;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// Shown while the phone camera is off (scripts/make-camera-placeholder.swift).
static PLACEHOLDER: &[u8] = include_bytes!("../../../assets/camera-off.jpg");
/// The stream's port is Dotlink's plus this, so a second instance (another port) gets its own.
const PORT_OFFSET: u16 = 1000;
const OBS_URL: &str = "https://obsproject.com/download";

/// The address to give OBS. `port`: Dotlink's own (the phone's) port.
pub fn camera_url(port: u16) -> String {
    format!("http://127.0.0.1:{}/camera/{}.mjpg", port.wrapping_add(PORT_OFFSET), token())
}

pub fn obs_download_url() -> &'static str {
    OBS_URL
}

/// Whether OBS is installed where Finder puts apps.
pub fn obs_installed() -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    [PathBuf::from("/Applications/OBS.app"), home.join("Applications/OBS.app")].iter().any(|p| p.is_dir())
}

/// The secret part of the stream's address, made once.
fn token() -> String {
    let path = crate::config::config_dir().join("camera-token");
    if let Ok(t) = std::fs::read_to_string(&path)
        && t.trim().len() == 32
    {
        return t.trim().to_string();
    }
    let mut bytes = [0u8; 16];
    let _ = getrandom::fill(&mut bytes);
    let t = hex::encode(bytes);
    let _ = std::fs::create_dir_all(crate::config::config_dir());
    if std::fs::write(&path, &t).is_ok() {
        crate::config::set_private(&path);
    }
    t
}

/// Always there: OBS reads the stream whenever it likes.
pub fn find_camera(_configured: &str) -> Result<PathBuf> {
    Ok(PathBuf::new())
}

#[derive(Clone)]
pub struct CameraHub {
    /// The newest phone frame; empty until the phone sends one.
    frames: Arc<watch::Sender<Bytes>>,
}

impl CameraHub {
    pub fn start(_configured_device: String, _size: (u32, u32)) -> Self {
        let (frames, _) = watch::channel(Bytes::new());
        let frames = Arc::new(frames);
        let port = crate::config::Config::load().map_or(crate::config::DEFAULT_PORT, |c| c.port);
        tokio::spawn(serve(frames.clone(), port.wrapping_add(PORT_OFFSET), token()));
        Self { frames }
    }

    /// The phone's newest frame, replacing one no reader has taken yet.
    pub fn send(&self, jpeg: Bytes) {
        self.frames.send_replace(jpeg);
    }
}

async fn serve(frames: Arc<watch::Sender<Bytes>>, port: u16, token: String) {
    use axum::extract::{Path, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Response};

    let path = format!("/camera/{token}.mjpg");
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("camera: can't serve the OBS stream on port {port}: {e}");
            return;
        }
    };
    let app = axum::Router::new()
        .route(
            "/camera/{file}",
            axum::routing::get(
                move |State(frames): State<Arc<watch::Sender<Bytes>>>, Path(file): Path<String>| {
                    let ok = format!("/camera/{file}") == path;
                    async move {
                        if !ok {
                            return StatusCode::NOT_FOUND.into_response();
                        }
                        tracing::info!("camera: a reader connected (OBS)");
                        Response::builder()
                            .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=dotlink")
                            .header(header::CACHE_CONTROL, "no-store")
                            .body(axum::body::Body::from_stream(mjpeg(frames.subscribe())))
                            .unwrap()
                    }
                },
            ),
        )
        .with_state(frames);
    if let Err(e) = axum::serve(listener, app).await {
        tracing::warn!("camera: the OBS stream stopped: {e}");
    }
}

/// The frames as multipart JPEG: each phone frame once, and the placeholder at 5 fps while the
/// phone has sent nothing for a second.
fn mjpeg(rx: watch::Receiver<Bytes>) -> impl futures::Stream<Item = Result<Bytes, std::io::Error>> {
    futures::stream::unfold((rx, None::<Instant>), |(mut rx, mut last)| async move {
        let jpeg = loop {
            tokio::select! {
                changed = rx.changed() => {
                    changed.ok()?;
                    let frame = rx.borrow_and_update().clone();
                    if !frame.is_empty() {
                        last = Some(Instant::now());
                        break frame;
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(200)) => {
                    if last.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
                        break Bytes::from_static(PLACEHOLDER);
                    }
                }
            }
        };
        let mut part = format!("--dotlink\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", jpeg.len())
            .into_bytes();
        part.extend_from_slice(&jpeg);
        part.extend_from_slice(b"\r\n");
        Some((Ok(Bytes::from(part)), (rx, last)))
    })
}
