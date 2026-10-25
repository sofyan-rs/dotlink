//! Phone camera → "Dotlink Camera", a Windows 11 virtual camera (MFCreateVirtualCamera).
//!
//! The camera's media source is `dotlink_camera.dll` (the `camera` crate), which the Windows
//! Camera Frame Server loads; Setup installs and registers it once (administrator). While
//! Dotlink runs it keeps the camera registered for this user and writes frames into the file
//! the DLL reads: the phone's newest JPEG, decoded and fitted into the fixed camera size as
//! NV12, or a dark frame while the phone camera is off. Like on Linux, only the newest frame
//! matters and the size never changes while apps read.

use anyhow::{Context, Result, bail};
use dotlink_camera::shared::{self, FrameWriter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// The virtual camera is up (apps can see it).
static STARTED: AtomicBool = AtomicBool::new(false);
/// Why it isn't, for the phone and the log.
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());
/// Setup reinstalled the DLL: register the camera again.
static RESTART: AtomicBool = AtomicBool::new(false);

const REGISTRY_KEY: &str = r"SOFTWARE\Classes\CLSID";
pub const DLL_NAME: &str = "dotlink_camera.dll";

/// Where Setup installs the DLL: readable by the frame server, unlike a user folder.
/// Named after its content: a DLL that is loaded (by Dotlink itself, which activates the
/// camera in-process, by the frame server, or by an app using the camera) can't be overwritten,
/// so a new build is installed next to it under a new name and the registration moved over.
pub fn installed_dll(bundled: &std::path::Path) -> std::io::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let programs = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let hash = hex::encode(Sha256::digest(std::fs::read(bundled)?));
    Ok(programs.join("Dotlink").join(format!("dotlink_camera-{}.dll", &hash[..12])))
}

/// The DLL this build ships, next to dotlink.exe.
pub fn bundled_dll() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    // A development build leaves it in deps/ (cargo only copies what was asked for by name).
    [dir.join(DLL_NAME), dir.join("deps").join(DLL_NAME)].into_iter().find(|p| p.is_file())
}

/// The DLL path registered for the camera's class, if any.
pub fn registered_dll() -> Option<PathBuf> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(format!(r"{REGISTRY_KEY}\{}\InprocServer32", shared::CLSID_STR))
        .ok()?;
    key.get_value::<String, _>("").ok().map(PathBuf::from)
}

/// [shared::SOURCE] of the installed DLL, as Setup recorded it (builds from before it did: None).
fn registered_source() -> Option<String> {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(format!(r"{REGISTRY_KEY}\{}", shared::CLSID_STR))
        .ok()?;
    key.get_value::<String, _>(shared::SOURCE_VALUE).ok()
}

/// Windows 11 (build 22000) or newer: the first with MFCreateVirtualCamera.
pub fn os_supported() -> bool {
    let key = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    key.and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|b| b.parse::<u32>().ok())
        .is_some_and(|b| b >= 22000)
}

/// Whether the camera is installed, current and usable; the reason otherwise (for Setup).
pub fn installed_ok() -> std::result::Result<(), String> {
    if !os_supported() {
        return Err("Needs Windows 11: earlier versions have no virtual cameras.".into());
    }
    let Some(bundled) = bundled_dll() else {
        return Err(format!("{DLL_NAME} is missing next to dotlink.exe; reinstall Dotlink."));
    };
    // The registered DLL is this build's camera: the same source (its bytes differ with every
    // release), or, installed by a build that didn't record that, the same bytes.
    let installed = registered_dll().filter(|p| p.is_file());
    let current = installed.is_some_and(|p| {
        registered_source().as_deref() == Some(shared::SOURCE)
            || std::fs::read(p).ok().is_some_and(|a| std::fs::read(&bundled).ok().is_some_and(|b| a == b))
    });
    if !current {
        return Err(
            "Installs the virtual camera into Program Files, so apps see \"Dotlink Camera\".".into(),
        );
    }
    if !STARTED.load(Ordering::SeqCst) {
        let why = LAST_ERROR.lock().unwrap().clone();
        if !why.is_empty() {
            return Err(format!("Installed, but Windows refused it: {why}"));
        }
    }
    Ok(())
}

/// Setup reinstalled the DLL; the running camera picks it up.
pub fn reload() {
    RESTART.store(true, Ordering::SeqCst);
}

/// Ok while "Dotlink Camera" exists; the phone is told why otherwise.
pub fn find_camera(_configured: &str) -> Result<PathBuf> {
    if STARTED.load(Ordering::SeqCst) {
        return Ok(PathBuf::from(shared::FRIENDLY_NAME));
    }
    let why = LAST_ERROR.lock().unwrap().clone();
    if why.is_empty() {
        bail!("\"Dotlink Camera\" isn't set up yet: Dotlink → Setup → Webcam");
    }
    bail!("\"Dotlink Camera\" isn't available: {why}")
}

/// The newest phone frame, for the camera thread. A frame still waiting when the next one
/// arrives is replaced.
#[derive(Default)]
struct Latest {
    frame: Mutex<Option<axum::body::Bytes>>,
    ready: Condvar,
}

#[derive(Clone)]
pub struct CameraHub {
    latest: Arc<Latest>,
}

impl CameraHub {
    pub fn start(_configured_device: String, size: (u32, u32)) -> Self {
        let latest = Arc::new(Latest::default());
        let feed = latest.clone();
        let spawned = std::thread::Builder::new().name("dotlink-camera".into()).spawn(move || run(feed, size));
        if let Err(e) = spawned {
            tracing::warn!("camera: {e}");
        }
        Self { latest }
    }

    pub fn send(&self, jpeg: axum::body::Bytes) {
        *self.latest.frame.lock().unwrap() = Some(jpeg);
        self.latest.ready.notify_one();
    }
}

/// The camera thread: keeps the virtual camera registered and the frame file fed.
fn run(latest: Arc<Latest>, (width, height): (u32, u32)) {
    // NV12 needs even sizes.
    let (width, height) = (width & !1, height & !1);
    let mut camera: Option<windows::Win32::Media::MediaFoundation::IMFVirtualCamera> = None;
    let mut writer: Option<FrameWriter> = None;
    let mut last_try = Instant::now() - Duration::from_secs(60);
    let mut last_phone: Option<Instant> = None;
    let mut blank_shown = false;
    let mut warned = String::new();
    let mut frame = vec![0u8; shared::frame_len(width, height)];
    let mut decoded = Vec::new();
    // Debug aid: DOTLINK_DUMP_FRAME=1 keeps the latest phone frame in %TEMP%.
    let dump = std::env::var_os("DOTLINK_DUMP_FRAME").is_some();
    let mut dumped = Instant::now();
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        let _ = windows::Win32::Media::MediaFoundation::MFStartup(
            windows::Win32::Media::MediaFoundation::MF_VERSION,
            windows::Win32::Media::MediaFoundation::MFSTARTUP_FULL,
        );
    }
    loop {
        if RESTART.swap(false, Ordering::SeqCst) {
            if let Some(c) = camera.take() {
                unsafe {
                    let _ = c.Remove();
                }
            }
            STARTED.store(false, Ordering::SeqCst);
            last_try = Instant::now() - Duration::from_secs(60);
        }
        // Set up what's missing, every few seconds until it works (Setup may fix it meanwhile).
        if (writer.is_none() || camera.is_none()) && last_try.elapsed() > Duration::from_secs(5) {
            last_try = Instant::now();
            let result = (|| -> Result<()> {
                if writer.is_none() {
                    let mut w = FrameWriter::create(width, height).context("the frame file")?;
                    w.write(|f| shared::fill_blank(f, width, height));
                    writer = Some(w);
                }
                if camera.is_none() {
                    camera = Some(start_camera()?);
                    STARTED.store(true, Ordering::SeqCst);
                    tracing::info!("camera: \"{}\" is up ({width}x{height})", shared::FRIENDLY_NAME);
                }
                Ok(())
            })();
            let error = result.err().map(|e| format!("{e:#}")).unwrap_or_default();
            if !error.is_empty() && error != warned {
                tracing::info!("camera: {error}");
            }
            *LAST_ERROR.lock().unwrap() = error.clone();
            warned = error;
        }

        let jpeg = {
            let mut slot = latest.frame.lock().unwrap();
            if slot.is_none() {
                slot = latest.ready.wait_timeout(slot, Duration::from_millis(200)).unwrap().0;
            }
            slot.take()
        };
        if let Some(jpeg) = &jpeg
            && dump
            && dumped.elapsed() > Duration::from_secs(1)
        {
            dumped = Instant::now();
            let _ = std::fs::write(std::env::temp_dir().join("dotlink-last-frame.jpg"), jpeg);
        }
        let Some(writer) = writer.as_mut() else { continue };
        match jpeg {
            Some(jpeg) => match decode(&jpeg, &mut decoded) {
                Ok((w, h)) => {
                    fit_nv12(&decoded, w, h, &mut frame, width, height);
                    writer.write(|f| f.copy_from_slice(&frame));
                    last_phone = Some(Instant::now());
                    blank_shown = false;
                }
                Err(e) => tracing::debug!("camera: bad frame: {e:#}"),
            },
            None => {
                // The phone stopped: a dark frame instead of its last picture frozen.
                if !blank_shown && last_phone.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
                    writer.write(|f| shared::fill_blank(f, width, height));
                    blank_shown = true;
                }
            }
        }
    }
}

fn start_camera() -> Result<windows::Win32::Media::MediaFoundation::IMFVirtualCamera> {
    use windows::Win32::Media::MediaFoundation::*;
    if !os_supported() {
        bail!("needs Windows 11");
    }
    if registered_dll().is_none_or(|p| !p.is_file()) {
        bail!("not set up yet: Dotlink → Setup → Webcam");
    }
    unsafe {
        let camera = MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            MFVirtualCameraLifetime_Session,
            MFVirtualCameraAccess_CurrentUser,
            &windows::core::HSTRING::from(shared::FRIENDLY_NAME),
            &windows::core::HSTRING::from(shared::CLSID_STR),
            None,
        )
        .context("creating the virtual camera")?;
        camera.Start(None).context("starting the virtual camera")?;
        Ok(camera)
    }
}

/// The JPEG as full-range YCbCr (3 bytes per pixel); returns its size.
fn decode(jpeg: &[u8], out: &mut Vec<u8>) -> Result<(u32, u32)> {
    use zune_jpeg::zune_core::colorspace::ColorSpace;
    use zune_jpeg::zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::YCbCr);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(std::io::Cursor::new(jpeg), options);
    decoder.decode_headers().map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let info = decoder.info().context("no image info")?;
    let (w, h) = (info.width as u32, info.height as u32);
    out.resize(w as usize * h as usize * 3, 0);
    decoder.decode_into(out).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok((w, h))
}

/// Fits a full-range YCbCr picture into an NV12 frame (limited range, as video apps expect),
/// keeping its aspect ratio with black bars. Nearest-neighbour scaling: the phone already
/// sends about the camera size.
fn fit_nv12(src: &[u8], sw: u32, sh: u32, dst: &mut [u8], dw: u32, dh: u32) {
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw as usize, dh as usize);
    let luma = |v: u8| (16 + (v as u32 * 219 + 127) / 255) as u8;
    let chroma = |v: u8| (16 + (v as u32 * 224 + 127) / 255) as u8;
    // Black: Y 16, neutral chroma.
    dst[..dw * dh].fill(16);
    dst[dw * dh..].fill(128);
    if sw == 0 || sh == 0 {
        return;
    }
    let (fw, fh) = if sw * dh > sh * dw { (dw, sh * dw / sw) } else { (sw * dh / sh, dh) };
    let (fw, fh) = (fw & !1, fh & !1);
    let (ox, oy) = (((dw - fw) / 2) & !1, ((dh - fh) / 2) & !1);
    let (y_plane, uv_plane) = dst.split_at_mut(dw * dh);
    for y in 0..fh {
        let sy = y * sh / fh;
        let row = &mut y_plane[(oy + y) * dw + ox..][..fw];
        for (x, out) in row.iter_mut().enumerate() {
            let sx = x * sw / fw;
            *out = luma(src[(sy * sw + sx) * 3]);
        }
    }
    for y in 0..fh / 2 {
        let sy = (y * 2) * sh / fh;
        let row = &mut uv_plane[(oy / 2 + y) * dw + ox..][..fw];
        for x in 0..fw / 2 {
            let sx = (x * 2) * sw / fw;
            let p = (sy * sw + sx) * 3;
            row[x * 2] = chroma(src[p + 1]);
            row[x * 2 + 1] = chroma(src[p + 2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_with_bars() {
        // An 8x4 white picture into 8x8: two rows of bar above and below (offsets stay even
        // for NV12's 2x2 chroma).
        let src = [255u8, 128, 128].repeat(32);
        let mut dst = vec![0u8; shared::frame_len(8, 8)];
        fit_nv12(&src, 8, 4, &mut dst, 8, 8);
        let y = &dst[..64];
        assert_eq!(&y[..16], &[16; 16]);
        assert_eq!(&y[16..48], &[235; 32]);
        assert_eq!(&y[48..], &[16; 16]);
        assert!(dst[64..].iter().all(|&c| c == 128));
    }
}
