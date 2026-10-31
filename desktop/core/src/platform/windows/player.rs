//! What plays on this PC and controlling it: the System Media Transport Controls (what the
//! volume flyout shows: Spotify, browsers, Media Player…), and the output volume through the
//! default endpoint's IAudioEndpointVolume.

use super::super::{Art, MediaCmd, NowPlaying};
use anyhow::{Result, bail};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

fn session() -> windows::core::Result<Session> {
    Manager::RequestAsync()?.join()?.GetCurrentSession()
}

/// TimeSpan counts 100 ns ticks.
fn ms(ticks: i64) -> u64 {
    (ticks.max(0) / 10_000) as u64
}

pub fn now_playing() -> Option<NowPlaying> {
    let s = session().ok()?;
    let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    let status = s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?;
    if status == Status::Stopped || status == Status::Closed {
        return None;
    }
    let timeline = s.GetTimelineProperties().ok();
    let title = props.Title().ok()?.to_string();
    if title.is_empty() {
        return None;
    }
    // "Spotify.exe", or a long id for packaged apps ("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic").
    let app = s.SourceAppUserModelId().map(|id| id.to_string()).unwrap_or_default();
    let player = app.trim_end_matches(".exe").rsplit(['!', '\\']).next().unwrap_or(&app).to_string();
    Some(NowPlaying {
        player,
        title,
        artist: props.Artist().map(|a| a.to_string()).unwrap_or_default(),
        album: props.AlbumTitle().map(|a| a.to_string()).unwrap_or_default(),
        playing: status == Status::Playing,
        position_ms: timeline.as_ref().and_then(|t| t.Position().ok()).map(|t| ms(t.Duration)).unwrap_or(0),
        duration_ms: timeline.as_ref().and_then(|t| t.EndTime().ok()).map(|t| ms(t.Duration)).unwrap_or(0),
        art: thumbnail(&props).map(Art::Bytes),
        volume: volume().ok(),
    })
}

fn thumbnail(props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties) -> Option<Vec<u8>> {
    use windows::Storage::Streams::DataReader;
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok().filter(|&n| n > 0 && n < 4 * 1024 * 1024)?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    Some(bytes)
}

fn endpoint_volume() -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    // SAFETY: plain COM calls on this thread; COM is set up for it first (already set up is fine).
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        devices.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
    }
}

fn volume() -> windows::core::Result<f32> {
    // SAFETY: a COM call on a live interface.
    unsafe { endpoint_volume()?.GetMasterVolumeLevelScalar() }
}

pub fn media_command(cmd: &MediaCmd) -> Result<()> {
    if let MediaCmd::Volume(level) = cmd {
        // SAFETY: a COM call on a live interface; no event context.
        unsafe { endpoint_volume()?.SetMasterVolumeLevelScalar(*level, std::ptr::null())? };
        return Ok(());
    }
    let s = session().map_err(|_| anyhow::anyhow!("nothing is playing"))?;
    let done = match cmd {
        MediaCmd::Play => s.TryPlayAsync()?.join()?,
        MediaCmd::Pause => s.TryPauseAsync()?.join()?,
        MediaCmd::Toggle => s.TryTogglePlayPauseAsync()?.join()?,
        MediaCmd::Next => s.TrySkipNextAsync()?.join()?,
        MediaCmd::Prev => s.TrySkipPreviousAsync()?.join()?,
        MediaCmd::Seek(ms) => s.TryChangePlaybackPositionAsync(*ms as i64 * 10_000)?.join()?,
        MediaCmd::Volume(_) => unreachable!(),
    };
    if !done {
        bail!("the player didn't take it");
    }
    Ok(())
}
