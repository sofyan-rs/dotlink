//! What plays on this Mac and controlling it, through AppleScript for Spotify and Music.
//! MediaRemote (what Control Center shows, browsers included) only answers Apple's own apps
//! since macOS 15.4, so a browser's player isn't seen; the media keys still reach it.

use super::super::{Art, MediaCmd, NowPlaying};
use anyhow::{Result, bail};
use std::process::Command;

/// The apps asked, in order: the first playing (else paused) one wins.
const APPS: [&str; 2] = ["Spotify", "Music"];

fn osascript(script: &str) -> Option<String> {
    let out = Command::new("osascript").args(["-e", script]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

/// Whether `app` runs, without starting it (`tell application` would).
fn running(app: &str) -> bool {
    osascript(&format!("application \"{app}\" is running")).as_deref() == Some("true")
}

fn state(app: &str) -> Option<NowPlaying> {
    // Spotify reports ms for duration and s for position; Music s for both.
    let (duration, art) = if app == "Spotify" {
        ("(duration of current track) / 1000", "artwork url of current track")
    } else {
        ("duration of current track", "\"\"")
    };
    let script = format!(
        "tell application \"{app}\"\n\
         set s to (player state as text)\n\
         if s is \"stopped\" then return \"stopped\"\n\
         set sep to (ASCII character 9)\n\
         return s & sep & (player position as text) & sep & ({duration} as text) & sep & (name of current track) & sep & (artist of current track) & sep & (album of current track) & sep & ({art})\n\
         end tell"
    );
    let line = osascript(&script)?;
    let f: Vec<&str> = line.split('\t').collect();
    let [status, position, duration, title, artist, album, art] = f[..] else { return None };
    // AppleScript writes decimals with the system's separator.
    let secs = |s: &str| s.replace(',', ".").parse::<f64>().unwrap_or(0.0);
    Some(NowPlaying {
        player: app.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        playing: status == "playing",
        position_ms: (secs(position) * 1000.0) as u64,
        duration_ms: (secs(duration) * 1000.0) as u64,
        art: (!art.is_empty()).then(|| Art::Url(art.to_string())),
        volume: volume(),
    })
}

fn volume() -> Option<f32> {
    osascript("output volume of (get volume settings)")?.parse::<f32>().ok().map(|v| v / 100.0)
}

pub fn now_playing() -> Option<NowPlaying> {
    let states: Vec<NowPlaying> = APPS.iter().filter(|app| running(app)).filter_map(|app| state(app)).collect();
    states.iter().find(|s| s.playing).or(states.first()).cloned()
}

pub fn media_command(cmd: &MediaCmd) -> Result<()> {
    if let MediaCmd::Volume(level) = cmd {
        return match osascript(&format!("set volume output volume {}", (level * 100.0).round() as u32)) {
            Some(_) => Ok(()),
            None => bail!("couldn't set the volume"),
        };
    }
    let Some(app) = now_playing().map(|s| s.player) else { bail!("nothing plays in Spotify or Music") };
    let verb = match cmd {
        MediaCmd::Play => "play".to_string(),
        MediaCmd::Pause => "pause".to_string(),
        MediaCmd::Toggle => "playpause".to_string(),
        MediaCmd::Next => "next track".to_string(),
        MediaCmd::Prev => "previous track".to_string(),
        MediaCmd::Seek(ms) => format!("set player position to {}", *ms as f64 / 1000.0),
        MediaCmd::Volume(_) => unreachable!(),
    };
    match osascript(&format!("tell application \"{app}\" to {verb}")) {
        Some(_) => Ok(()),
        None => bail!("{app} didn't take it"),
    }
}
