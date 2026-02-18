//! Spotify API integration for fetching tracks and playlists.

use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API_BASE: &str = "https://api.spotify.com/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyTrack {
    pub id: String,
    pub name: String,
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u64,
}

impl SpotifyTrack {
    pub fn to_search_query(&self) -> String {
        let artist = self.artists.first().map(|s| s.as_str()).unwrap_or("");
        format!("{} {}", artist, self.name)
    }

    pub fn display_name(&self) -> String {
        let artists = self.artists.join(", ");
        format!("{} - {}", artists, self.name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyPlaylist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<SpotifyTrack>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct SpotifyArtist {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbum {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrackFull {
    id: String,
    name: String,
    artists: Vec<SpotifyArtist>,
    album: SpotifyAlbum,
    duration_ms: u64,
}

impl From<SpotifyTrackFull> for SpotifyTrack {
    fn from(t: SpotifyTrackFull) -> Self {
        SpotifyTrack {
            id: t.id,
            name: t.name,
            artists: t.artists.into_iter().map(|a| a.name).collect(),
            album: t.album.name,
            duration_ms: t.duration_ms,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PlaylistTrackItem {
    track: Option<SpotifyTrackFull>,
}

#[derive(Debug, Deserialize)]
struct PlaylistTracksResponse {
    items: Vec<PlaylistTrackItem>,
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaylistResponse {
    id: String,
    name: String,
    tracks: PlaylistTracksResponse,
}

pub struct SpotifyClient {
    client: Client,
    client_id: String,
    client_secret: String,
    token: Option<String>,
    token_expires: Option<Instant>,
}

impl SpotifyClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client: Client::new(),
            client_id,
            client_secret,
            token: None,
            token_expires: None,
        }
    }

    pub fn from_env() -> Result<Self> {
        let client_id = std::env::var("SPOTIFY_CLIENT_ID").context("SPOTIFY_CLIENT_ID not set")?;
        let client_secret =
            std::env::var("SPOTIFY_CLIENT_SECRET").context("SPOTIFY_CLIENT_SECRET not set")?;
        Ok(Self::new(client_id, client_secret))
    }

    async fn ensure_token(&mut self) -> Result<String> {
        let needs_refresh = match self.token_expires {
            Some(expires) => Instant::now() >= expires,
            None => true,
        };

        if needs_refresh {
            self.refresh_token().await?;
        }

        self.token.clone().context("No token available")
    }

    async fn refresh_token(&mut self) -> Result<()> {
        let credentials = format!("{}:{}", self.client_id, self.client_secret);
        let encoded = BASE64.encode(credentials.as_bytes());

        let resp: TokenResponse = self
            .client
            .post(TOKEN_URL)
            .header("Authorization", format!("Basic {encoded}"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        self.token = Some(resp.access_token);
        self.token_expires = Some(Instant::now() + Duration::from_secs(resp.expires_in - 60));

        Ok(())
    }

    pub async fn get_track(&mut self, track_id: &str) -> Result<SpotifyTrack> {
        let token = self.ensure_token().await?;
        let url = format!("{API_BASE}/tracks/{track_id}");

        let track: SpotifyTrackFull = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(track.into())
    }

    pub async fn get_playlist(&mut self, playlist_id: &str) -> Result<SpotifyPlaylist> {
        let token = self.ensure_token().await?;
        let url = format!("{API_BASE}/playlists/{playlist_id}");

        let resp: PlaylistResponse = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let mut tracks: Vec<SpotifyTrack> = resp
            .tracks
            .items
            .into_iter()
            .filter_map(|item| item.track.map(Into::into))
            .collect();

        let mut next_url = resp.tracks.next;
        while let Some(url) = next_url {
            let token = self.ensure_token().await?;
            let page: PlaylistTracksResponse = self
                .client
                .get(&url)
                .header("Authorization", format!("Bearer {token}"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            tracks.extend(
                page.items
                    .into_iter()
                    .filter_map(|item| item.track.map(Into::into)),
            );
            next_url = page.next;
        }

        Ok(SpotifyPlaylist {
            id: resp.id,
            name: resp.name,
            tracks,
        })
    }

    pub fn parse_spotify_url(url: &str) -> Option<SpotifyResource> {
        let url = url.trim();

        if let Some(rest) = url.strip_prefix("spotify:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() == 2 {
                return match parts[0] {
                    "track" => Some(SpotifyResource::Track(parts[1].to_string())),
                    "playlist" => Some(SpotifyResource::Playlist(parts[1].to_string())),
                    "album" => Some(SpotifyResource::Album(parts[1].to_string())),
                    _ => None,
                };
            }
        }

        if url.contains("open.spotify.com") {
            let path = url.split("open.spotify.com/").nth(1)?;
            let path = path.split('?').next()?;
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 2 {
                return match parts[0] {
                    "track" => Some(SpotifyResource::Track(parts[1].to_string())),
                    "playlist" => Some(SpotifyResource::Playlist(parts[1].to_string())),
                    "album" => Some(SpotifyResource::Album(parts[1].to_string())),
                    _ => None,
                };
            }
        }

        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpotifyResource {
    Track(String),
    Playlist(String),
    Album(String),
}

#[derive(Debug, Clone)]
pub struct SoulseekPlaylist {
    pub name: String,
    pub tracks: Vec<SoulseekPlaylistTrack>,
}

#[derive(Debug, Clone)]
pub struct SoulseekPlaylistTrack {
    pub spotify_track: SpotifyTrack,
    pub search_query: String,
    pub matched_file: Option<MatchedFile>,
}

#[derive(Debug, Clone)]
pub struct MatchedFile {
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub bitrate: Option<u32>,
}

impl SoulseekPlaylist {
    pub fn from_spotify_playlist(playlist: SpotifyPlaylist) -> Self {
        let tracks = playlist
            .tracks
            .into_iter()
            .map(|track| {
                let search_query = track.to_search_query();
                SoulseekPlaylistTrack {
                    spotify_track: track,
                    search_query,
                    matched_file: None,
                }
            })
            .collect();

        SoulseekPlaylist {
            name: playlist.name,
            tracks,
        }
    }

    pub fn from_single_track(track: SpotifyTrack) -> Self {
        let search_query = track.to_search_query();
        SoulseekPlaylist {
            name: track.display_name(),
            tracks: vec![SoulseekPlaylistTrack {
                spotify_track: track,
                search_query,
                matched_file: None,
            }],
        }
    }

    pub fn matched_count(&self) -> usize {
        self.tracks
            .iter()
            .filter(|t| t.matched_file.is_some())
            .count()
    }

    pub fn unmatched_tracks(&self) -> impl Iterator<Item = &SoulseekPlaylistTrack> {
        self.tracks.iter().filter(|t| t.matched_file.is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_spotify_track_url() {
        let url = "https://open.spotify.com/track/4iV5W9uYEdYUVa79Axb7Rh";
        let result = SpotifyClient::parse_spotify_url(url);
        assert_eq!(
            result,
            Some(SpotifyResource::Track("4iV5W9uYEdYUVa79Axb7Rh".to_string()))
        );
    }

    #[test]
    fn test_parse_spotify_track_url_with_query() {
        let url = "https://open.spotify.com/track/4iV5W9uYEdYUVa79Axb7Rh?si=abc123";
        let result = SpotifyClient::parse_spotify_url(url);
        assert_eq!(
            result,
            Some(SpotifyResource::Track("4iV5W9uYEdYUVa79Axb7Rh".to_string()))
        );
    }

    #[test]
    fn test_parse_spotify_playlist_url() {
        let url = "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M";
        let result = SpotifyClient::parse_spotify_url(url);
        assert_eq!(
            result,
            Some(SpotifyResource::Playlist(
                "37i9dQZF1DXcBWIGoYBM5M".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_spotify_uri() {
        let uri = "spotify:track:4iV5W9uYEdYUVa79Axb7Rh";
        let result = SpotifyClient::parse_spotify_url(uri);
        assert_eq!(
            result,
            Some(SpotifyResource::Track("4iV5W9uYEdYUVa79Axb7Rh".to_string()))
        );
    }

    #[test]
    fn test_parse_spotify_playlist_uri() {
        let uri = "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M";
        let result = SpotifyClient::parse_spotify_url(uri);
        assert_eq!(
            result,
            Some(SpotifyResource::Playlist(
                "37i9dQZF1DXcBWIGoYBM5M".to_string()
            ))
        );
    }

    #[test]
    fn test_parse_invalid_url() {
        let url = "https://example.com/not-spotify";
        let result = SpotifyClient::parse_spotify_url(url);
        assert_eq!(result, None);
    }

    #[test]
    fn test_track_to_search_query() {
        let track = SpotifyTrack {
            id: "abc".to_string(),
            name: "Bohemian Rhapsody".to_string(),
            artists: vec!["Queen".to_string()],
            album: "A Night at the Opera".to_string(),
            duration_ms: 354000,
        };
        assert_eq!(track.to_search_query(), "Queen Bohemian Rhapsody");
    }

    #[test]
    fn test_track_display_name() {
        let track = SpotifyTrack {
            id: "abc".to_string(),
            name: "Under Pressure".to_string(),
            artists: vec!["Queen".to_string(), "David Bowie".to_string()],
            album: "Hot Space".to_string(),
            duration_ms: 248000,
        };
        assert_eq!(track.display_name(), "Queen, David Bowie - Under Pressure");
    }

    #[test]
    fn test_soulseek_playlist_from_spotify() {
        let spotify_playlist = SpotifyPlaylist {
            id: "123".to_string(),
            name: "Test Playlist".to_string(),
            tracks: vec![
                SpotifyTrack {
                    id: "1".to_string(),
                    name: "Song One".to_string(),
                    artists: vec!["Artist A".to_string()],
                    album: "Album".to_string(),
                    duration_ms: 180000,
                },
                SpotifyTrack {
                    id: "2".to_string(),
                    name: "Song Two".to_string(),
                    artists: vec!["Artist B".to_string()],
                    album: "Album".to_string(),
                    duration_ms: 200000,
                },
            ],
        };

        let slsk_playlist = SoulseekPlaylist::from_spotify_playlist(spotify_playlist);

        assert_eq!(slsk_playlist.name, "Test Playlist");
        assert_eq!(slsk_playlist.tracks.len(), 2);
        assert_eq!(slsk_playlist.tracks[0].search_query, "Artist A Song One");
        assert_eq!(slsk_playlist.tracks[1].search_query, "Artist B Song Two");
        assert_eq!(slsk_playlist.matched_count(), 0);
    }

    #[test]
    fn test_matched_count() {
        let mut playlist = SoulseekPlaylist {
            name: "Test".to_string(),
            tracks: vec![
                SoulseekPlaylistTrack {
                    spotify_track: SpotifyTrack {
                        id: "1".to_string(),
                        name: "Song".to_string(),
                        artists: vec!["Artist".to_string()],
                        album: "Album".to_string(),
                        duration_ms: 180000,
                    },
                    search_query: "Artist Song".to_string(),
                    matched_file: None,
                },
                SoulseekPlaylistTrack {
                    spotify_track: SpotifyTrack {
                        id: "2".to_string(),
                        name: "Song 2".to_string(),
                        artists: vec!["Artist".to_string()],
                        album: "Album".to_string(),
                        duration_ms: 180000,
                    },
                    search_query: "Artist Song 2".to_string(),
                    matched_file: Some(MatchedFile {
                        username: "user".to_string(),
                        filename: "song2.mp3".to_string(),
                        size: 5000000,
                        bitrate: Some(320),
                    }),
                },
            ],
        };

        assert_eq!(playlist.matched_count(), 1);
        assert_eq!(playlist.unmatched_tracks().count(), 1);

        playlist.tracks[0].matched_file = Some(MatchedFile {
            username: "user".to_string(),
            filename: "song.mp3".to_string(),
            size: 4000000,
            bitrate: Some(320),
        });

        assert_eq!(playlist.matched_count(), 2);
        assert_eq!(playlist.unmatched_tracks().count(), 0);
    }
}
