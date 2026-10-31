//! What plays on this PC and controlling it: MPRIS through `playerctl` (Spotify, browsers,
//! mpv, VLC… whichever player it picks), the output volume through `wpctl` (PipeWire).

use super::super::{Art, MediaCmd, NowPlaying};
use anyhow::{Context, Result, bail};
use std::process::Command;

fn playerctl(args: &[&str]) -> Result<String> {
    let out = Command::new("playerctl").args(args).output().context("playerctl is not installed (see Setup)")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

pub fn now_playing() -> Option<NowPlaying> {
    const FORMAT: &str = "{{playerName}}\t{{status}}\t{{position}}\t{{mpris:length}}\t{{xesam:title}}\t{{xesam:artist}}\t{{xesam:album}}\t{{mpris:artUrl}}";
    let line = playerctl(&["metadata", "--format", FORMAT]).ok()?;
    let f: Vec<&str> = line.split('\t').collect();
    let [player, status, position, length, title, artist, album, art] = f[..] else { return None };
    if status == "Stopped" || title.is_empty() {
        return None;
    }
    Some(NowPlaying {
        player: player.to_string(),
        title: title.to_string(),
        artist: artist.to_string(),
        album: album.to_string(),
        playing: status == "Playing",
        // MPRIS counts in microseconds.
        position_ms: position.parse::<u64>().unwrap_or(0) / 1000,
        duration_ms: length.parse::<u64>().unwrap_or(0) / 1000,
        art: (!art.is_empty()).then(|| Art::Url(art.to_string())),
        volume: volume(),
    })
}

/// `wpctl get-volume @DEFAULT_AUDIO_SINK@`: "Volume: 0.40" (" [MUTED]").
fn volume() -> Option<f32> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().nth(1)?.parse().ok()
}

pub fn media_command(cmd: &MediaCmd) -> Result<()> {
    match cmd {
        MediaCmd::Play => playerctl(&["play"]).map(drop),
        MediaCmd::Pause => playerctl(&["pause"]).map(drop),
        MediaCmd::Toggle => playerctl(&["play-pause"]).map(drop),
        MediaCmd::Next => playerctl(&["next"]).map(drop),
        MediaCmd::Prev => playerctl(&["previous"]).map(drop),
        MediaCmd::Seek(ms) => playerctl(&["position", &format!("{:.3}", *ms as f64 / 1000.0)]).map(drop),
        MediaCmd::Volume(level) => {
            let out = Command::new("wpctl")
                .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")])
                .output()
                .context("wpctl is not installed")?;
            if !out.status.success() {
                bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
            }
            Ok(())
        }
    }
}
