//! What Dotlink and the camera DLL share: the COM class id, and the frame file.
//!
//! The DLL runs inside the Windows Camera Frame Server service (Local Service account, session
//! 0), so the two can't share a session's named objects. They map the same file instead:
//! `%ProgramData%\Dotlink\camera.bin`, created by Dotlink with read access for Local Service.
//! Views of one local file are coherent across processes.
//!
//! Layout: a 64-byte header (magic, version, width, height, sequence) and one NV12 frame. The
//! sequence is a seqlock: odd while Dotlink writes, even when the frame is whole.

use std::path::PathBuf;

/// The media source's COM class, registered under HKLM for the frame server.
pub const CLSID: u128 = 0x882edc27_ac20_4c63_935d_aba37092eb60;
pub const CLSID_STR: &str = "{882EDC27-AC20-4C63-935D-ABA37092EB60}";
pub const FRIENDLY_NAME: &str = "Dotlink Camera";

/// A hash of this crate's source (`build.rs`). Setup stores it with the installed DLL and
/// asks to install again only when it changes, not on every release.
pub const SOURCE: &str = env!("DOTLINK_CAMERA_SOURCE");

/// The registry value, next to the camera's class, that holds [SOURCE] of the installed DLL.
pub const SOURCE_VALUE: &str = "DotlinkSource";

pub const MAGIC: u32 = u32::from_le_bytes(*b"DLCM");
pub const VERSION: u32 = 1;
pub const HEADER: usize = 64;
const OFF_MAGIC: usize = 0;
const OFF_VERSION: usize = 4;
const OFF_WIDTH: usize = 8;
const OFF_HEIGHT: usize = 12;
const OFF_SEQ: usize = 16;

/// Bytes of one NV12 frame: a full-size Y plane and a half-size interleaved UV plane.
pub fn frame_len(width: u32, height: u32) -> usize {
    width as usize * height as usize * 3 / 2
}

/// `DOTLINK_CAMERA_FILE` overrides it (tests: never the file a running Dotlink uses).
pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("DOTLINK_CAMERA_FILE") {
        return PathBuf::from(p);
    }
    let data = std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    data.join("Dotlink").join("camera.bin")
}

/// A dark grey NV12 frame: what the camera shows before Dotlink sends anything.
pub fn fill_blank(frame: &mut [u8], width: u32, height: u32) {
    let y = width as usize * height as usize;
    frame[..y].fill(0x20);
    frame[y..].fill(0x80);
}

#[cfg(windows)]
pub use mapped::{FrameReader, FrameWriter};

#[cfg(windows)]
mod mapped {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering, fence};
    use windows::Win32::Foundation::{
        CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        OPEN_ALWAYS, OPEN_EXISTING,
    };
    use windows::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS,
        MapViewOfFile, PAGE_READONLY, PAGE_READWRITE, UnmapViewOfFile,
    };
    use windows::core::{HSTRING, PCWSTR};

    struct Mapping {
        file: HANDLE,
        section: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        len: usize,
    }

    // The view is plain memory shared with another process; access goes through the seqlock.
    unsafe impl Send for Mapping {}
    unsafe impl Sync for Mapping {}

    impl Mapping {
        fn ptr(&self) -> *mut u8 {
            self.view.Value as *mut u8
        }

        fn u32_at(&self, off: usize) -> u32 {
            unsafe { std::ptr::read_volatile(self.ptr().add(off) as *const u32) }
        }

        fn seq(&self) -> &AtomicU64 {
            unsafe { &*(self.ptr().add(OFF_SEQ) as *const AtomicU64) }
        }
    }

    impl Drop for Mapping {
        fn drop(&mut self) {
            unsafe {
                let _ = UnmapViewOfFile(self.view);
                let _ = CloseHandle(self.section);
                let _ = CloseHandle(self.file);
            }
        }
    }

    /// Dotlink's side: writes frames of one size.
    pub struct FrameWriter {
        map: Mapping,
        pub width: u32,
        pub height: u32,
    }

    impl FrameWriter {
        pub fn create(width: u32, height: u32) -> windows::core::Result<Self> {
            let path = path();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // Owner and administrators: full; Local Service (the frame server) and SYSTEM: read.
            let sddl = HSTRING::from("D:P(A;;FA;;;OW)(A;;FA;;;BA)(A;;FR;;;LS)(A;;FR;;;SY)");
            let mut sd = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    &sddl,
                    SDDL_REVISION_1,
                    &mut sd,
                    None,
                )?;
            }
            let attrs = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            };
            let file = unsafe {
                CreateFileW(
                    &HSTRING::from(path.as_os_str()),
                    (GENERIC_READ | GENERIC_WRITE).0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    Some(&attrs),
                    OPEN_ALWAYS,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            };
            unsafe { LocalFree(Some(HLOCAL(sd.0))) };
            let file = file?;
            let len = HEADER + frame_len(width, height);
            let map = map(file, len, true)?;
            let writer = Self { map, width, height };
            writer.map.seq().store(1, Ordering::Release);
            writer.write_header();
            writer.map.seq().store(2, Ordering::Release);
            Ok(writer)
        }

        fn write_header(&self) {
            unsafe {
                let p = self.map.ptr();
                std::ptr::write_volatile(p.add(OFF_WIDTH) as *mut u32, self.width);
                std::ptr::write_volatile(p.add(OFF_HEIGHT) as *mut u32, self.height);
                std::ptr::write_volatile(p.add(OFF_VERSION) as *mut u32, VERSION);
                std::ptr::write_volatile(p.add(OFF_MAGIC) as *mut u32, MAGIC);
            }
        }

        /// Writes one NV12 frame (`fill` gets the frame's bytes).
        pub fn write(&mut self, fill: impl FnOnce(&mut [u8])) {
            let seq = self.map.seq();
            let start = seq.load(Ordering::Relaxed) | 1;
            seq.store(start, Ordering::Relaxed);
            fence(Ordering::Release);
            // The header again with every frame: whatever else wrote to the file (an older
            // Dotlink, another size), readers see this writer's size once the frame is whole.
            self.write_header();
            let frame = unsafe {
                std::slice::from_raw_parts_mut(self.map.ptr().add(HEADER), self.map.len - HEADER)
            };
            fill(frame);
            seq.store(start + 1, Ordering::Release);
        }
    }

    /// The DLL's side: reads the newest whole frame.
    pub struct FrameReader {
        map: Mapping,
    }

    impl FrameReader {
        pub fn open() -> windows::core::Result<Self> {
            let file = unsafe {
                CreateFileW(
                    &HSTRING::from(path().as_os_str()),
                    GENERIC_READ.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )?
            };
            let len = std::fs::metadata(path())
                .map(|m| m.len() as usize)
                .unwrap_or(0);
            if len < HEADER {
                unsafe { CloseHandle(file)? };
                return Err(windows::core::Error::from(
                    windows::Win32::Foundation::E_FAIL,
                ));
            }
            Ok(Self {
                map: map(file, len, false)?,
            })
        }

        /// (width, height) the writer uses, when the header is valid.
        pub fn size(&self) -> Option<(u32, u32)> {
            let ok = self.map.u32_at(OFF_MAGIC) == MAGIC && self.map.u32_at(OFF_VERSION) == VERSION;
            let size = (self.map.u32_at(OFF_WIDTH), self.map.u32_at(OFF_HEIGHT));
            (ok && size.0 > 0 && size.1 > 0 && HEADER + frame_len(size.0, size.1) <= self.map.len)
                .then_some(size)
        }

        /// Copies the current frame into `dst` (sized for `size()`); returns its sequence number,
        /// or None while Dotlink is mid-write or the size differs.
        pub fn read(&self, dst: &mut [u8], size: (u32, u32)) -> Option<u64> {
            if self.size() != Some(size) || dst.len() != frame_len(size.0, size.1) {
                return None;
            }
            let seq = self.map.seq();
            for _ in 0..3 {
                let before = seq.load(Ordering::Acquire);
                if before & 1 == 1 {
                    std::thread::yield_now();
                    continue;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        self.map.ptr().add(HEADER),
                        dst.as_mut_ptr(),
                        dst.len(),
                    )
                };
                fence(Ordering::Acquire);
                if seq.load(Ordering::Relaxed) == before {
                    return Some(before);
                }
            }
            None
        }
    }

    fn map(file: HANDLE, len: usize, write: bool) -> windows::core::Result<Mapping> {
        let protect = if write { PAGE_READWRITE } else { PAGE_READONLY };
        let section = match unsafe {
            CreateFileMappingW(
                file,
                None,
                protect,
                (len as u64 >> 32) as u32,
                len as u32,
                PCWSTR::null(),
            )
        } {
            Ok(s) => s,
            Err(e) => {
                unsafe { CloseHandle(file)? };
                return Err(e);
            }
        };
        let access = if write { FILE_MAP_WRITE } else { FILE_MAP_READ };
        let view = unsafe { MapViewOfFile(section, access, 0, 0, len) };
        if view.Value.is_null() {
            let e = windows::core::Error::from_thread();
            unsafe {
                let _ = CloseHandle(section);
                let _ = CloseHandle(file);
            }
            return Err(e);
        }
        Ok(Mapping {
            file,
            section,
            view,
            len,
        })
    }
}
