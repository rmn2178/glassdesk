use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source_app: String,
    pub is_playing: bool,
    pub position_secs: i64,
    pub duration_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricLine {
    pub time_secs: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsResponse {
    pub found: bool,
    pub synced: bool,
    pub lines: Vec<LyricLine>,
    pub plain: String,
}

#[cfg(target_os = "windows")]
mod imp {
    use super::MediaInfo;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };

    pub fn get_media_info() -> MediaInfo {
        let Ok(op) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync() else {
            return MediaInfo::default();
        };
        let Ok(manager) = op.get() else {
            return MediaInfo::default();
        };
        let Ok(session) = manager.GetCurrentSession() else {
            return MediaInfo::default();
        };

        let source_app = session
            .SourceAppUserModelId()
            .map(|s| s.to_string())
            .unwrap_or_default();

        let is_playing = session
            .GetPlaybackInfo()
            .and_then(|p| p.PlaybackStatus())
            .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
            .unwrap_or(false);

        let (position_secs, duration_secs) = session
            .GetTimelineProperties()
            .map(|t| {
                let pos = t.Position().map(|d| d.Duration / 10_000_000).unwrap_or(0);
                let end = t.EndTime().map(|d| d.Duration / 10_000_000).unwrap_or(0);
                (pos.max(0), end.max(0))
            })
            .unwrap_or((0, 0));

        let (title, artist, album) = session
            .TryGetMediaPropertiesAsync()
            .ok()
            .and_then(|op| op.get().ok())
            .map(|props| {
                (
                    props.Title().map(|s| s.to_string()).unwrap_or_default(),
                    props.Artist().map(|s| s.to_string()).unwrap_or_default(),
                    props.AlbumTitle().map(|s| s.to_string()).unwrap_or_default(),
                )
            })
            .unwrap_or_default();

        MediaInfo {
            has_session: !title.is_empty() || !source_app.is_empty(),
            title,
            artist,
            album,
            source_app,
            is_playing,
            position_secs,
            duration_secs,
        }
    }

    pub fn media_command(cmd: &str) -> Result<(), String> {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|e| e.to_string())?
            .get()
            .map_err(|e| e.to_string())?;
        let session = manager.GetCurrentSession().map_err(|e| e.to_string())?;
        match cmd {
            "play_pause" => {
                let _ = session
                    .TryTogglePlayPauseAsync()
                    .map_err(|e| e.to_string())?
                    .get();
            }
            "next" => {
                let _ = session
                    .TrySkipNextAsync()
                    .map_err(|e| e.to_string())?
                    .get();
            }
            "prev" => {
                let _ = session
                    .TrySkipPreviousAsync()
                    .map_err(|e| e.to_string())?
                    .get();
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::MediaInfo;
    pub fn get_media_info() -> MediaInfo {
        MediaInfo::default()
    }
    pub fn media_command(_cmd: &str) -> Result<(), String> {
        Ok(())
    }
}

pub use imp::{get_media_info, media_command};

#[derive(Deserialize)]
struct LrcLibApi {
    #[serde(rename = "syncedLyrics")]
    synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics")]
    plain_lyrics: Option<String>,
}

fn parse_lrc(raw: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('[') {
            continue;
        }
        if let Some(close_idx) = trimmed.find(']') {
            let ts = &trimmed[1..close_idx];
            let text = trimmed[close_idx + 1..].trim().to_string();
            let parts: Vec<&str> = ts.split(':').collect();
            if parts.len() == 2 {
                if let (Ok(m), Ok(s)) = (parts[0].parse::<f64>(), parts[1].parse::<f64>()) {
                    lines.push(LyricLine {
                        time_secs: m * 60.0 + s,
                        text,
                    });
                }
            }
        }
    }
    lines
}

pub async fn fetch_lyrics(artist: String, title: String) -> LyricsResponse {
    if title.trim().is_empty() {
        return LyricsResponse {
            found: false,
            synced: false,
            lines: vec![],
            plain: String::new(),
        };
    }

    let url = format!(
        "https://lrclib.net/api/get?artist_name={}&track_name={}",
        urlencoding::encode(artist.trim()),
        urlencoding::encode(title.trim())
    );

    let client = reqwest::Client::builder()
        .user_agent("NotchGlass/0.1.0")
        .build();

    let Ok(client) = client else {
        return LyricsResponse {
            found: false,
            synced: false,
            lines: vec![],
            plain: String::new(),
        };
    };

    if let Ok(resp) = client.get(&url).send().await {
        if resp.status().is_success() {
            if let Ok(data) = resp.json::<LrcLibApi>().await {
                if let Some(synced) = data.synced_lyrics {
                    let parsed = parse_lrc(&synced);
                    if !parsed.is_empty() {
                        return LyricsResponse {
                            found: true,
                            synced: true,
                            lines: parsed,
                            plain: data.plain_lyrics.unwrap_or_default(),
                        };
                    }
                }
                if let Some(plain) = data.plain_lyrics {
                    return LyricsResponse {
                        found: true,
                        synced: false,
                        lines: vec![],
                        plain,
                    };
                }
            }
        }
    }

    LyricsResponse {
        found: false,
        synced: false,
        lines: vec![],
        plain: String::new(),
    }
}
