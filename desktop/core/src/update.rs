//! Updates from the GitHub releases. Windows runs the new setup .exe (it closes Dotlink, installs
//! and starts it again); an AppImage replaces itself. Other Linux installs (.deb, .rpm, built from
//! source) get the release page: their package manager or `install.sh` updates them.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::cmp::Ordering;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const RELEASES: &str = "https://api.github.com/repos/sofyan-rs/dotlink/releases?per_page=20";

/// The release workflow appends the downloads from this heading on; the app hides them.
const DOWNLOADS_HEADING: &str = "## Downloads";

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub prerelease: bool,
    pub page_url: String,
    /// The file this install updates itself with; None: open [Self::page_url] instead.
    pub asset: Option<Asset>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    html_url: String,
    assets: Vec<Asset>,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(concat!("dotlink/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// The newest release above this version, or None. Pre-releases count only when this version
/// is one too, so stable installs stay on stable releases. Blocking.
pub fn check() -> Result<Option<Release>> {
    let mut body = agent(Duration::from_secs(20))
        .get(RELEASES)
        .header("Accept", "application/vnd.github+json")
        .call()
        .context("couldn't reach GitHub")?
        .into_body();
    let releases: Vec<GhRelease> = serde_json::from_reader(body.as_reader())?;
    let current = Version::parse(CURRENT).context("bad version")?;
    let with_pre = !current.pre.is_empty();
    let best = releases
        .into_iter()
        .filter(|r| !r.draft && (with_pre || !r.prerelease))
        .filter_map(|r| Some((Version::parse(&r.tag_name)?, r)))
        .max_by(|a, b| a.0.cmp(&b.0));
    let Some((version, r)) = best.filter(|(v, _)| *v > current) else { return Ok(None) };
    let asset = r.assets.into_iter().find(|a| own_asset(&a.name));
    Ok(Some(Release {
        version: version.text,
        notes: r.body.unwrap_or_default().split(DOWNLOADS_HEADING).next().unwrap_or_default().trim().to_string(),
        prerelease: r.prerelease,
        page_url: r.html_url,
        asset,
    }))
}

/// The release file that updates this kind of install.
fn own_asset(name: &str) -> bool {
    if cfg!(windows) {
        name.ends_with("-setup.exe")
    } else if cfg!(target_os = "linux") {
        appimage().is_some() && name.ends_with(".AppImage")
    } else {
        false
    }
}

/// The AppImage this process runs from (its runtime sets `APPIMAGE`).
fn appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file())
}

/// Downloads `asset` and gets it ready: the installer's path (Windows) or the replaced AppImage.
/// `progress` gets 0.0–1.0. Blocking.
pub fn download(asset: &Asset, progress: impl Fn(f32)) -> Result<PathBuf> {
    let dest = if cfg!(windows) {
        std::env::temp_dir().join(&asset.name)
    } else {
        let current = appimage().context("not running from an AppImage")?;
        // Next to it: the rename over it stays on one filesystem.
        current.with_extension("AppImage.part")
    };
    let mut body = agent(Duration::from_secs(600)).get(&asset.url).call().context("download")?.into_body();
    let mut reader = body.as_reader();
    let mut file = std::fs::File::create(&dest).with_context(|| format!("can't write {}", dest.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])?;
        done += n as u64;
        if asset.size > 0 {
            progress(done as f32 / asset.size as f32);
        }
    }
    drop(file);
    if asset.size > 0 && done != asset.size {
        let _ = std::fs::remove_file(&dest);
        bail!("the download stopped early");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let current = appimage().context("not running from an AppImage")?;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&dest, &current).context("can't replace the AppImage")?;
        return Ok(current);
    }
    #[cfg(not(unix))]
    Ok(dest)
}

/// Starts the update downloaded by [download]. The caller quits right after: the installer
/// (Windows) or a small shell (AppImage) waits for this process to end, then starts Dotlink again.
pub fn launch(path: &std::path::Path) -> Result<()> {
    if cfg!(windows) {
        // Passive: a progress bar only. /R starts Dotlink when done; /NS keeps the shortcuts as
        // they are (the first install made them).
        std::process::Command::new(path).args(["/P", "/R", "/NS"]).spawn().context("can't start the installer")?;
    } else {
        let pid = std::process::id().to_string();
        std::process::Command::new("sh")
            .args(["-c", r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec "$2" >/dev/null 2>&1"#, "sh", &pid])
            .arg(path)
            .stdin(std::process::Stdio::null())
            .spawn()
            .context("can't restart Dotlink")?;
    }
    Ok(())
}

/// A semantic version: 1.2.3, or 1.2.3-beta.1 (a pre-release, older than 1.2.3).
#[derive(Debug)]
struct Version {
    text: String,
    core: [u64; 3],
    pre: Vec<String>,
}

impl Version {
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let main = text.split('+').next()?;
        let (core, pre) = match main.split_once('-') {
            Some((core, pre)) => (core, pre.split('.').map(str::to_string).collect()),
            None => (main, Vec::new()),
        };
        let mut parts = core.split('.').map(|p| p.parse().ok());
        let core = [parts.next()??, parts.next()??, parts.next()??];
        if parts.next().is_some() {
            return None;
        }
        Some(Self { text: main.to_string(), core, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(a), Ok(b)) => a.cmp(&b),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if c != Ordering::Equal {
                        return c;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

#[cfg(test)]
mod tests {
    use super::Version;

    #[test]
    fn order() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("v0.2.0") > v("0.1.9"));
        assert!(v("0.2.0") > v("0.2.0-beta.2"));
        assert!(v("0.2.0-beta.10") > v("0.2.0-beta.2"));
        assert!(v("0.2.0-rc.1") > v("0.2.0-beta.3"));
        assert!(v("0.2.0-beta.1") > v("0.1.0"));
        assert!(v("1.0.0+5") == v("1.0.0"));
        assert!(Version::parse("1.0").is_none());
    }
}
