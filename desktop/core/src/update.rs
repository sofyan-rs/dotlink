//! Updates from the GitHub releases. Windows runs the new setup .exe (it closes Dotlink, installs
//! and starts it again); an AppImage replaces itself. Other Linux installs (.deb, .rpm, built from
//! source) get the release page: their package manager or `install.sh` updates them.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::cmp::Ordering;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const REPO: &str = "https://github.com/sofyan-rs/dotlink";
const RELEASES: &str = "https://api.github.com/repos/sofyan-rs/dotlink/releases?per_page=20";
const FEED: &str = "https://github.com/sofyan-rs/dotlink/releases.atom";

/// The release workflow appends the downloads from this heading on; the app hides them.
const DOWNLOADS_HEADING: &str = "## Downloads";
/// The same heading in the feed's HTML.
const DOWNLOADS_HTML: &str = "<h2>Downloads</h2>";

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
    let releases = releases()?;
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

/// The releases from the API. It allows 60 requests an hour per IP address without a token
/// (every phone and PC behind one router shares them); past that, from the releases feed, which
/// has no such limit.
fn releases() -> Result<Vec<GhRelease>> {
    let response = agent(Duration::from_secs(20))
        .get(RELEASES)
        .header("Accept", "application/vnd.github+json")
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .context("couldn't reach GitHub")?;
    let status = response.status().as_u16();
    if response.status().is_success() {
        return Ok(serde_json::from_reader(response.into_body().as_reader())?);
    }
    if status != 403 && status != 429 {
        bail!("GitHub answered {status}");
    }
    let minutes = response.headers().get("x-ratelimit-reset").and_then(|v| v.to_str().ok()?.parse::<u64>().ok()).and_then(|reset| {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(reset.saturating_sub(now).div_ceil(60).max(1))
    });
    from_feed().map_err(|_| match minutes {
        Some(m) => anyhow!("GitHub limits update checks. Try again in {m} min."),
        None => anyhow!("GitHub limits update checks. Try again later."),
    })
}

fn from_feed() -> Result<Vec<GhRelease>> {
    let xml = agent(Duration::from_secs(20)).get(FEED).call().context("couldn't reach GitHub")?.into_body().read_to_string()?;
    Ok(parse_feed(&xml))
}

/// The releases in the Atom feed: no drafts, no pre-release flag (a version with a suffix is one,
/// as for the release workflow), the notes as HTML, and the files named in the download table the
/// workflow appends (sizes unknown).
fn parse_feed(xml: &str) -> Vec<GhRelease> {
    xml.split("<entry>")
        .skip(1)
        .filter_map(|entry| {
            let tag = between(entry, "/releases/tag/", "\"")?;
            let html = unescape(between(entry, "<content type=\"html\">", "</content>").unwrap_or_default());
            let (notes, downloads) = html.split_once(DOWNLOADS_HTML).unwrap_or((&html, ""));
            let assets = downloads
                .split("<code>")
                .skip(1)
                .filter_map(|s| s.split_once("</code>"))
                .map(|(name, _)| unescape(name))
                .map(|name| Asset { url: format!("{REPO}/releases/download/{tag}/{name}"), name, size: 0 })
                .collect();
            Some(GhRelease {
                tag_name: tag.to_string(),
                body: Some(html_to_text(notes)),
                draft: false,
                prerelease: tag.split('+').next().unwrap_or_default().contains('-'),
                html_url: format!("{REPO}/releases/tag/{tag}"),
                assets,
            })
        })
        .collect()
}

/// The text between the first `start` and the `end` after it.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let rest = &text[text.find(start)? + start.len()..];
    Some(&rest[..rest.find(end)?])
}

/// Release notes HTML as the Markdown the API gives, near enough.
fn html_to_text(html: &str) -> String {
    let mut text = String::new();
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else { break };
        match &rest[open + 1..open + close] {
            tag @ ("h1" | "h2" | "h3" | "h4") => text.push_str(&format!("{} ", "#".repeat(tag[1..].parse().unwrap_or(2)))),
            "li" => text.push_str("- "),
            "code" | "/code" => text.push('`'),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    // At most one blank line in a row.
    let mut out = String::new();
    let mut blank = 0;
    for line in unescape(&text).trim().lines() {
        blank = if line.trim().is_empty() { blank + 1 } else { 0 };
        if blank < 2 {
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
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
    let response = agent(Duration::from_secs(600)).get(&asset.url).call().context("download")?;
    // The feed doesn't give sizes.
    let size = match asset.size {
        0 => response.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0),
        size => size,
    };
    let mut body = response.into_body();
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
        if size > 0 {
            progress(done as f32 / size as f32);
        }
    }
    drop(file);
    if size > 0 && done != size {
        let _ = std::fs::remove_file(&dest);
        bail!("the download stopped early");
    }
    #[cfg(unix)]
    let dest = {
        use std::os::unix::fs::PermissionsExt;
        let current = appimage().context("not running from an AppImage")?;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&dest, &current).context("can't replace the AppImage")?;
        current
    };
    Ok(dest)
}

/// Starts the update downloaded by [download]. The caller quits right after: a hidden
/// PowerShell (Windows) or a small shell (AppImage) waits for this process to end, then runs the
/// installer or starts Dotlink again.
pub fn launch(path: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // The installer kills a running Dotlink, and gives up when that fails, which it does
        // while Dotlink is still shutting down: start it only once this process is gone.
        // Passive: a progress bar only. /R starts Dotlink when done; /NS keeps the shortcuts as
        // they are (the first install made them).
        let script = "Wait-Process -Id $env:DOTLINK_PID -Timeout 60 -ErrorAction SilentlyContinue; \
                      Start-Process -FilePath $env:DOTLINK_SETUP -ArgumentList '/P','/R','/NS'";
        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
            .env("DOTLINK_PID", std::process::id().to_string())
            .env("DOTLINK_SETUP", path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .context("can't start the installer")?;
    }
    #[cfg(not(windows))]
    {
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
    use super::{Version, parse_feed};

    #[test]
    fn feed() {
        let xml = r#"<feed><entry>
    <link rel="alternate" type="text/html" href="https://github.com/sofyan-rs/dotlink/releases/tag/v0.1.0-beta.4"/>
    <content type="html">&lt;h2&gt;Fixes&lt;/h2&gt;
&lt;ul&gt;
&lt;li&gt;Scanning works &amp;amp; &lt;code&gt;again&lt;/code&gt;.&lt;/li&gt;
&lt;/ul&gt;
&lt;h2&gt;Downloads&lt;/h2&gt;
&lt;td&gt;&lt;code&gt;Dotlink-0.1.0-beta.4.apk&lt;/code&gt;&lt;/td&gt;
&lt;td&gt;&lt;code&gt;dotlink_0.1.0-beta.4_x64-setup.exe&lt;/code&gt;&lt;/td&gt;</content>
  </entry><entry>
    <link rel="alternate" type="text/html" href="https://github.com/sofyan-rs/dotlink/releases/tag/v0.2.0"/>
    <content type="html"></content>
  </entry></feed>"#;
        let releases = parse_feed(xml);
        assert_eq!(releases.len(), 2);
        let r = &releases[0];
        assert_eq!(r.tag_name, "v0.1.0-beta.4");
        assert!(r.prerelease && !releases[1].prerelease);
        assert_eq!(r.body.as_deref(), Some("## Fixes\n\n- Scanning works & `again`."));
        assert_eq!(r.assets.len(), 2);
        assert_eq!(r.assets[1].name, "dotlink_0.1.0-beta.4_x64-setup.exe");
        assert_eq!(
            r.assets[1].url,
            "https://github.com/sofyan-rs/dotlink/releases/download/v0.1.0-beta.4/dotlink_0.1.0-beta.4_x64-setup.exe"
        );
    }

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
