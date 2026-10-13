//! What's playing on this computer (Spotify, a browser tab, Media Player, …),
//! from Windows' media controls — the same source as the volume flyout — and
//! play / pause / skip for it. Shown on Home under the status card.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager_,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Playback,
};
use windows::Storage::Streams::DataReader;

use crate::paths;

const POLL: Duration = Duration::from_millis(1500);
/// Album art bigger than this is skipped (it travels to the UI as a data URL).
const MAX_THUMB: u64 = 512 * 1024;

#[derive(Clone, Debug, Serialize, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: Option<String>,
    /// The app playing it ("Spotify", "Chrome", …).
    pub app: String,
    pub playing: bool,
    /// Where it was at `updated_at` (unix ms); the UI moves it on while playing.
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub updated_at: Option<u64>,
    pub can_prev: bool,
    pub can_next: bool,
    /// Album art as a data URL.
    pub thumb: Option<String>,
}

#[derive(Default)]
pub struct MediaState {
    current: Mutex<Option<Media>>,
}

/// "Spotify.exe" → "Spotify", "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" → "Media Player".
fn app_name(id: &str) -> String {
    let lower = id.to_ascii_lowercase();
    let known = [
        ("spotify", "Spotify"),
        ("zunemusic", "Media Player"),
        ("zunevideo", "Movies & TV"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("opera", "Opera"),
        ("brave", "Brave"),
        ("vlc", "VLC"),
        ("tidal", "TIDAL"),
        ("deezer", "Deezer"),
        ("applemusic", "Apple Music"),
        ("itunes", "iTunes"),
        ("youtube", "YouTube Music"),
    ];
    if let Some((_, name)) = known.iter().find(|(k, _)| lower.contains(k)) {
        return name.to_string();
    }
    let last = id.rsplit(['\\', '/', '!']).next().unwrap_or(id);
    let name = last.trim_end_matches(".exe").trim_end_matches(".EXE");
    let name = name.rsplit('.').next().unwrap_or(name);
    if name.is_empty() { "Media".into() } else { name.to_string() }
}

/// Windows FILETIME (100 ns since 1601) → unix ms.
fn unix_ms(universal: i64) -> Option<u64> {
    const EPOCH_DIFF: i64 = 116_444_736_000_000_000;
    (universal > EPOCH_DIFF).then(|| ((universal - EPOCH_DIFF) / 10_000) as u64)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn thumbnail(session: &Session) -> windows::core::Result<Option<String>> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let Ok(thumb) = props.Thumbnail() else { return Ok(None) };
    let stream = thumb.OpenReadAsync()?.join()?;
    let size = stream.Size()?;
    if size == 0 || size > MAX_THUMB {
        return Ok(None);
    }
    let kind = stream.ContentType().map(|t| t.to_string_lossy()).unwrap_or_default();
    let kind = if kind.starts_with("image/") { kind } else { "image/png".into() };
    let reader = DataReader::CreateDataReader(&stream)?;
    reader.LoadAsync(size as u32)?.join()?;
    let mut buf = vec![0u8; size as usize];
    reader.ReadBytes(&mut buf)?;
    Ok(Some(format!("data:{kind};base64,{}", base64(&buf))))
}

/// The session Windows shows in its own media flyout; None when nothing plays.
fn read(manager: &Manager_, last: Option<&Media>) -> windows::core::Result<Option<Media>> {
    let Ok(session) = manager.GetCurrentSession() else { return Ok(None) };
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string_lossy();
    if title.trim().is_empty() {
        return Ok(None);
    }
    let artist = props.Artist().map(|a| a.to_string_lossy()).ok().filter(|a| !a.trim().is_empty());
    let info = session.GetPlaybackInfo()?;
    let playing = info.PlaybackStatus()? == Playback::Playing;
    let (can_prev, can_next) = info
        .Controls()
        .map(|c| (c.IsPreviousEnabled().unwrap_or(false), c.IsNextEnabled().unwrap_or(false)))
        .unwrap_or((false, false));
    let (mut position_ms, mut duration_ms, mut updated_at) = (None, None, None);
    if let Ok(t) = session.GetTimelineProperties() {
        let end = t.EndTime().map(|d| d.Duration).unwrap_or(0) - t.StartTime().map(|d| d.Duration).unwrap_or(0);
        if end > 0 {
            duration_ms = Some((end / 10_000) as u64);
            position_ms = t.Position().ok().map(|p| (p.Duration.max(0) / 10_000) as u64);
            updated_at = t.LastUpdatedTime().ok().and_then(|d| unix_ms(d.UniversalTime));
        }
    }
    let app = app_name(&session.SourceAppUserModelId().map(|s| s.to_string_lossy()).unwrap_or_default());
    // album art only when the track changes
    let same_track = last.is_some_and(|l| l.title == title && l.artist == artist && l.app == app);
    let thumb = match last {
        Some(l) if same_track => l.thumb.clone(),
        _ => thumbnail(&session).ok().flatten(),
    };
    Ok(Some(Media { title, artist, app, playing, position_ms, duration_ms, updated_at, can_prev, can_next, thumb }))
}

/// Watches the media controls for the app's lifetime and tells the UI (`media`) when anything changes.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let manager = match Manager_::RequestAsync().and_then(|op| op.join()) {
            Ok(m) => m,
            Err(e) => return paths::log(format!("media: unavailable: {e}")),
        };
        loop {
            let last = app.state::<MediaState>().current.lock().unwrap().clone();
            let now = read(&manager, last.as_ref()).unwrap_or(None);
            if now != last {
                *app.state::<MediaState>().current.lock().unwrap() = now.clone();
                let _ = app.emit("media", now);
            }
            std::thread::sleep(POLL);
        }
    });
}

#[tauri::command]
pub fn media_status(state: tauri::State<MediaState>) -> Option<Media> {
    state.current.lock().unwrap().clone()
}

/// "toggle" | "next" | "prev" on whatever Windows considers the current player.
#[tauri::command]
pub async fn media_control(action: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || -> windows::core::Result<()> {
        let session = Manager_::RequestAsync()?.join()?.GetCurrentSession()?;
        let op = match action.as_str() {
            "next" => session.TrySkipNextAsync()?,
            "prev" => session.TrySkipPreviousAsync()?,
            _ => session.TryTogglePlayPauseAsync()?,
        };
        op.join()?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.message())
}
