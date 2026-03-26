use crate::{AppResult, Error};
use qobuz_player_models::Track;
use reqwest::Client;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const LASTFM_API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Clone)]
pub struct LastFm {
    client: Client,
    api_key: String,
    api_secret: String,
    session_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthTokenResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct SessionResponse {
    session: Session,
}

#[derive(Debug, Deserialize)]
struct Session {
    key: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct LastFmError {
    #[allow(dead_code)]
    error: u32,
    message: String,
}

impl LastFm {
    pub fn new(api_key: String, api_secret: String, session_key: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_key,
            api_secret,
            session_key,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.session_key.is_some()
    }

    pub fn set_session_key(&mut self, session_key: String) {
        self.session_key = Some(session_key);
    }

    fn sign(&self, params: &BTreeMap<&str, &str>) -> String {
        let mut sig_string = String::new();
        for (key, value) in params {
            sig_string.push_str(key);
            sig_string.push_str(value);
        }
        sig_string.push_str(&self.api_secret);
        format!("{:x}", md5::compute(sig_string))
    }

    pub async fn get_auth_token(&self) -> AppResult<String> {
        let mut params = BTreeMap::new();
        params.insert("method", "auth.getToken");
        params.insert("api_key", &self.api_key);

        let sig = self.sign(&params);

        let response = self
            .client
            .get(LASTFM_API_URL)
            .query(&[
                ("method", "auth.getToken"),
                ("api_key", &self.api_key),
                ("api_sig", &sig),
                ("format", "json"),
            ])
            .send()
            .await?;

        let text = response.text().await?;

        if let Ok(error) = serde_json::from_str::<LastFmError>(&text) {
            return Err(Error::LastFmError {
                message: error.message,
            });
        }

        let token_response: AuthTokenResponse = serde_json::from_str(&text)?;
        Ok(token_response.token)
    }

    pub fn get_auth_url(&self, token: &str) -> String {
        format!(
            "https://www.last.fm/api/auth/?api_key={}&token={}",
            self.api_key, token
        )
    }

    pub async fn get_session(&self, token: &str) -> AppResult<(String, String)> {
        let mut params = BTreeMap::new();
        params.insert("method", "auth.getSession");
        params.insert("api_key", &self.api_key);
        params.insert("token", token);

        let sig = self.sign(&params);

        let response = self
            .client
            .get(LASTFM_API_URL)
            .query(&[
                ("method", "auth.getSession"),
                ("api_key", &self.api_key),
                ("token", token),
                ("api_sig", &sig),
                ("format", "json"),
            ])
            .send()
            .await?;

        let text = response.text().await?;

        if let Ok(error) = serde_json::from_str::<LastFmError>(&text) {
            return Err(Error::LastFmError {
                message: error.message,
            });
        }

        let session_response: SessionResponse = serde_json::from_str(&text)?;
        Ok((session_response.session.key, session_response.session.name))
    }

    pub async fn now_playing(&self, track: &Track) -> AppResult<()> {
        let Some(ref session_key) = self.session_key else {
            tracing::debug!("Last.fm: skipping now_playing - no session key");
            return Ok(());
        };
        tracing::debug!("Last.fm: sending now_playing for '{}'", track.title);

        let artist = track.artist_name.as_deref().unwrap_or("Unknown Artist");
        let album = track.album_title.as_deref().unwrap_or("");
        let duration = track.duration_seconds.to_string();

        let mut params = BTreeMap::new();
        params.insert("method", "track.updateNowPlaying");
        params.insert("api_key", &self.api_key);
        params.insert("sk", session_key);
        params.insert("artist", artist);
        params.insert("track", &track.title);
        if !album.is_empty() {
            params.insert("album", album);
        }
        params.insert("duration", &duration);

        let sig = self.sign(&params);

        let mut form_params = vec![
            ("method", "track.updateNowPlaying"),
            ("api_key", self.api_key.as_str()),
            ("sk", session_key.as_str()),
            ("artist", artist),
            ("track", &track.title),
            ("duration", &duration),
            ("api_sig", &sig),
            ("format", "json"),
        ];

        if !album.is_empty() {
            form_params.push(("album", album));
        }

        let response = self.client.post(LASTFM_API_URL).form(&form_params).send().await?;
        let status = response.status();
        let text = response.text().await?;

        tracing::debug!("Last.fm now_playing response ({}): {}", status, text);

        if let Ok(error) = serde_json::from_str::<LastFmError>(&text) {
            tracing::warn!("Last.fm now playing error: {}", error.message);
            return Err(Error::LastFmError {
                message: error.message,
            });
        }

        tracing::info!("Last.fm now playing: {} - {}", artist, track.title);
        Ok(())
    }

    pub async fn scrobble(&self, track: &Track, timestamp: u64) -> AppResult<()> {
        let Some(ref session_key) = self.session_key else {
            tracing::debug!("Last.fm: skipping scrobble - no session key");
            return Ok(());
        };
        tracing::debug!("Last.fm: scrobbling '{}' with timestamp {}", track.title, timestamp);

        let artist = track.artist_name.as_deref().unwrap_or("Unknown Artist");
        let album = track.album_title.as_deref().unwrap_or("");
        let timestamp_str = timestamp.to_string();
        let duration = track.duration_seconds.to_string();

        let mut params = BTreeMap::new();
        params.insert("method", "track.scrobble");
        params.insert("api_key", &self.api_key);
        params.insert("sk", session_key);
        params.insert("artist", artist);
        params.insert("track", &track.title);
        if !album.is_empty() {
            params.insert("album", album);
        }
        params.insert("timestamp", &timestamp_str);
        params.insert("duration", &duration);

        let sig = self.sign(&params);

        let mut form_params = vec![
            ("method", "track.scrobble"),
            ("api_key", self.api_key.as_str()),
            ("sk", session_key.as_str()),
            ("artist", artist),
            ("track", &track.title),
            ("timestamp", &timestamp_str),
            ("duration", &duration),
            ("api_sig", &sig),
            ("format", "json"),
        ];

        if !album.is_empty() {
            form_params.push(("album", album));
        }

        let response = self.client.post(LASTFM_API_URL).form(&form_params).send().await?;
        let status = response.status();
        let text = response.text().await?;

        tracing::debug!("Last.fm scrobble response ({}): {}", status, text);

        if let Ok(error) = serde_json::from_str::<LastFmError>(&text) {
            tracing::warn!("Last.fm scrobble error: {}", error.message);
            return Err(Error::LastFmError {
                message: error.message,
            });
        }

        tracing::info!("Last.fm scrobbled: {} - {}", artist, track.title);
        Ok(())
    }
}

#[derive(Default)]
pub struct ScrobbleState {
    current_track_id: Option<u32>,
    track_start_timestamp: u64,
    scrobbled: bool,
    now_playing_sent: bool,
}


impl ScrobbleState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn track_changed(&mut self, track_id: u32) {
        self.current_track_id = Some(track_id);
        self.track_start_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.scrobbled = false;
        self.now_playing_sent = false;
    }

    pub fn mark_now_playing_sent(&mut self) {
        self.now_playing_sent = true;
    }

    pub fn mark_scrobbled(&mut self) {
        self.scrobbled = true;
    }

    pub fn should_send_now_playing(&self, track_id: u32) -> bool {
        self.current_track_id == Some(track_id) && !self.now_playing_sent
    }

    pub fn should_scrobble(&self, track_id: u32, position_secs: u64, duration_secs: u32) -> bool {
        if self.scrobbled || self.current_track_id != Some(track_id) {
            return false;
        }

        // Last.fm requires: played for at least 4 minutes OR 50% of track (whichever is less)
        // Also track must be at least 30 seconds long
        if duration_secs < 30 {
            return false;
        }

        let half_duration = (duration_secs / 2) as u64;
        let scrobble_threshold = half_duration.min(240); // 4 minutes = 240 seconds

        position_secs >= scrobble_threshold
    }

    pub fn start_timestamp(&self) -> u64 {
        self.track_start_timestamp
    }

    pub fn is_tracking(&self, track_id: u32) -> bool {
        self.current_track_id == Some(track_id)
    }
}
