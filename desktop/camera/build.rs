//! `DOTLINK_CAMERA_SOURCE`: a hash of this crate's source, so Setup can tell whether the camera
//! DLL installed in Program Files is still this code. The DLL's bytes change with every release
//! (the version is in them); its source only when the camera itself changes, and only then
//! does it need installing again (a UAC prompt).

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect(&dir.join("src"), &mut files);
    files.sort();
    let mut hash = Fnv::new();
    for file in &files {
        let relative = file.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        hash.write(relative.as_bytes());
        hash.write(&normalized(&std::fs::read(file).unwrap()));
    }
    // The manifest without its version line: dependencies and features count, the release
    // number doesn't.
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
    for line in manifest.lines().filter(|l| !l.trim_start().starts_with("version")) {
        hash.write(line.trim_end().as_bytes());
        hash.write(b"\n");
    }
    println!("cargo:rustc-env=DOTLINK_CAMERA_SOURCE={:016x}", hash.0);
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// CRLF as LF: a Windows checkout and a Linux one hash alike.
fn normalized(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for (i, &b) in bytes.iter().enumerate() {
        if !(b == b'\r' && bytes.get(i + 1) == Some(&b'\n')) {
            out.push(b);
        }
    }
    out
}

/// FNV-1a, 64 bits: stable across Rust versions, unlike std's hasher.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf29ce484222325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }
}
