use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use slsk_rs::peer::{SearchResultFile, SharedDirectory};
use tokio::sync::mpsc;

use crate::spotify::{MatchedFile, SoulseekPlaylist, SpotifyClient, SpotifyResource};

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub username: String,
    pub slot_free: bool,
    pub avg_speed: u32,
    #[allow(dead_code)]
    pub queue_length: u32,
    pub files: Vec<SearchResultFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadStatus {
    Queued,
    #[allow(dead_code)]
    Connecting,
    Downloading,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct Download {
    pub id: u32,
    #[allow(dead_code)]
    pub username: String,
    pub filename: String,
    pub size: u64,
    pub downloaded: u64,
    pub status: DownloadStatus,
}

#[derive(Debug)]
pub enum AppEvent {
    Connected,
    LoginSuccess {
        username: String,
    },
    LoginFailed {
        reason: String,
    },
    SearchResult(SearchResult),
    UserFiles(String, Vec<SharedDirectory>),
    StatusMessage(String),
    Error(String),
    DownloadQueued {
        id: u32,
        username: String,
        filename: String,
        size: u64,
    },
    DownloadStarted {
        id: u32,
    },
    DownloadProgress {
        id: u32,
        downloaded: u64,
    },
    DownloadCompleted {
        id: u32,
    },
    DownloadFailed {
        id: u32,
        reason: String,
    },
    SpotifyLoaded(SoulseekPlaylist),
    SpotifyError(String),
    SpotifyTrackSearching {
        track_index: usize,
    },
    SpotifyTrackMatched {
        track_index: usize,
        matched_file: MatchedFile,
    },
}

#[derive(Debug, Clone)]
pub enum ClientCommand {
    Search(String),
    #[allow(dead_code)]
    BrowseUser(String),
    DownloadFile {
        username: String,
        filename: String,
        size: u64,
    },
    FetchSpotify(String),
    SearchSpotifyTrack {
        track_index: usize,
        query: String,
    },
    DownloadSpotifyTrack {
        track_index: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Search,
    Results,
    Files,
    Downloads,
    Playlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    pub cmd_tx: mpsc::UnboundedSender<ClientCommand>,
    pub focus: Focus,
    pub input_mode: InputMode,
    pub search_input: String,
    pub cursor_position: usize,
    pub status: String,
    pub logged_in_user: Option<String>,
    pub search_results: Vec<SearchResult>,
    pub selected_result: usize,
    pub selected_file: usize,
    pub current_user_files: Option<(String, Vec<SharedDirectory>)>,
    pub current_search_files: Option<(String, Vec<SearchResultFile>)>,
    pub file_scroll: usize,
    pub downloads: Vec<Download>,
    pub selected_download: usize,
    pub spotify_playlist: Option<SoulseekPlaylist>,
    pub selected_playlist_track: usize,
    pub spotify_searching_track: Option<usize>,
}

impl App {
    pub fn new(cmd_tx: mpsc::UnboundedSender<ClientCommand>) -> Self {
        Self {
            cmd_tx,
            focus: Focus::Search,
            input_mode: InputMode::Normal,
            search_input: String::new(),
            cursor_position: 0,
            status: "Connecting...".to_string(),
            logged_in_user: None,
            search_results: Vec::new(),
            selected_result: 0,
            selected_file: 0,
            current_user_files: None,
            current_search_files: None,
            file_scroll: 0,
            downloads: Vec::new(),
            selected_download: 0,
            spotify_playlist: None,
            selected_playlist_track: 0,
            spotify_searching_track: None,
        }
    }

    pub fn is_input_mode(&self) -> bool {
        self.input_mode == InputMode::Editing
    }

    pub fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Connected => {
                self.status = "Connected, logging in...".to_string();
            }
            AppEvent::LoginSuccess { username } => {
                self.logged_in_user = Some(username.clone());
                self.status = format!("Logged in as {username}. Press / to search.");
            }
            AppEvent::LoginFailed { reason } => {
                self.status = format!("Login failed: {reason}");
            }
            AppEvent::SearchResult(result) => {
                self.search_results.push(result);
                self.status = format!(
                    "{} results from {} users",
                    self.search_results
                        .iter()
                        .map(|r| r.files.len())
                        .sum::<usize>(),
                    self.search_results.len()
                );
            }
            AppEvent::UserFiles(username, dirs) => {
                self.current_user_files = Some((username.clone(), dirs));
                self.focus = Focus::Files;
                self.file_scroll = 0;
                self.status = format!("Browsing {username}'s files");
            }
            AppEvent::StatusMessage(msg) => {
                self.status = msg;
            }
            AppEvent::Error(err) => {
                self.status = format!("Error: {err}");
            }
            AppEvent::DownloadQueued {
                id,
                username,
                filename,
                size,
            } => {
                let name = filename
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&filename)
                    .to_string();
                self.downloads.push(Download {
                    id,
                    username,
                    filename: name.clone(),
                    size,
                    downloaded: 0,
                    status: DownloadStatus::Queued,
                });
                self.status = format!("Queued: {}", name);
            }
            AppEvent::DownloadStarted { id } => {
                if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
                    dl.status = DownloadStatus::Downloading;
                    self.status = format!("Downloading: {}", dl.filename);
                }
            }
            AppEvent::DownloadProgress { id, downloaded } => {
                if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
                    dl.downloaded = downloaded;
                }
            }
            AppEvent::DownloadCompleted { id } => {
                if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
                    dl.status = DownloadStatus::Completed;
                    dl.downloaded = dl.size;
                    self.status = format!("Completed: {}", dl.filename);
                }
            }
            AppEvent::DownloadFailed { id, reason } => {
                if let Some(dl) = self.downloads.iter_mut().find(|d| d.id == id) {
                    dl.status = DownloadStatus::Failed(reason.clone());
                    self.status = format!("Failed: {} - {}", dl.filename, reason);
                }
            }
            AppEvent::SpotifyLoaded(playlist) => {
                let count = playlist.tracks.len();
                let name = playlist.name.clone();
                self.spotify_playlist = Some(playlist);
                self.selected_playlist_track = 0;
                self.focus = Focus::Playlist;
                self.status = format!("Loaded {} tracks from '{}'", count, name);
            }
            AppEvent::SpotifyError(err) => {
                self.status = format!("Spotify error: {}", err);
            }
            AppEvent::SpotifyTrackSearching { track_index } => {
                self.spotify_searching_track = Some(track_index);
                if let Some(playlist) = &self.spotify_playlist
                    && let Some(track) = playlist.tracks.get(track_index)
                {
                    self.status = format!(
                        "Searching [{}/{}]: {}",
                        track_index + 1,
                        playlist.tracks.len(),
                        track.spotify_track.display_name()
                    );
                }
            }
            AppEvent::SpotifyTrackMatched {
                track_index,
                matched_file,
            } => {
                if let Some(playlist) = &mut self.spotify_playlist {
                    if let Some(track) = playlist.tracks.get_mut(track_index) {
                        track.matched_file = Some(matched_file);
                    }
                    self.status = format!(
                        "Matched [{}/{}] - {} of {} found",
                        track_index + 1,
                        playlist.tracks.len(),
                        playlist.matched_count(),
                        playlist.tracks.len()
                    );
                }
                self.spotify_searching_track = None;
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.input_mode {
            InputMode::Editing => self.handle_editing_key(key),
            InputMode::Normal => self.handle_normal_key(key),
        }
    }

    fn handle_editing_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                self.input_mode = InputMode::Normal;
                if !self.search_input.is_empty() {
                    if let Some(resource) = SpotifyClient::parse_spotify_url(&self.search_input) {
                        let url = self.search_input.clone();
                        self.search_input.clear();
                        self.cursor_position = 0;
                        match resource {
                            SpotifyResource::Track(_) | SpotifyResource::Playlist(_) => {
                                self.status = "Loading from Spotify...".to_string();
                                let _ = self.cmd_tx.send(ClientCommand::FetchSpotify(url));
                            }
                            SpotifyResource::Album(_) => {
                                self.status = "Album support coming soon".to_string();
                            }
                        }
                    } else {
                        self.search_results.clear();
                        self.selected_result = 0;
                        self.status = format!("Searching for '{}'...", self.search_input);
                        let _ = self
                            .cmd_tx
                            .send(ClientCommand::Search(self.search_input.clone()));
                    }
                }
            }
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char(c) => {
                self.search_input.insert(self.cursor_position, c);
                self.cursor_position += 1;
            }
            KeyCode::Backspace if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.cursor_position > 0 {
                    let text = &self.search_input[..self.cursor_position];
                    let new_pos = text
                        .trim_end()
                        .rfind(|c: char| c.is_whitespace())
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    self.search_input.drain(new_pos..self.cursor_position);
                    self.cursor_position = new_pos;
                }
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                    self.search_input.remove(self.cursor_position);
                }
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position -= 1;
                }
            }
            KeyCode::Right => {
                if self.cursor_position < self.search_input.len() {
                    self.cursor_position += 1;
                }
            }
            KeyCode::Home => {
                self.cursor_position = 0;
            }
            KeyCode::End => {
                self.cursor_position = self.search_input.len();
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('/') | KeyCode::Char('s')
                if self.focus != Focus::Files && self.focus != Focus::Playlist =>
            {
                self.focus = Focus::Search;
                self.input_mode = InputMode::Editing;
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Search => {
                        if self.spotify_playlist.is_some() {
                            Focus::Playlist
                        } else if !self.downloads.is_empty() {
                            Focus::Downloads
                        } else if self.current_user_files.is_some()
                            || self.current_search_files.is_some()
                        {
                            Focus::Files
                        } else {
                            Focus::Results
                        }
                    }
                    Focus::Results => Focus::Search,
                    Focus::Files => Focus::Results,
                    Focus::Downloads => {
                        if self.current_user_files.is_some() || self.current_search_files.is_some()
                        {
                            Focus::Files
                        } else {
                            Focus::Results
                        }
                    }
                    Focus::Playlist => {
                        if !self.downloads.is_empty() {
                            Focus::Downloads
                        } else {
                            Focus::Search
                        }
                    }
                };
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Search => {
                        if self.spotify_playlist.is_some() {
                            Focus::Playlist
                        } else {
                            Focus::Results
                        }
                    }
                    Focus::Results => {
                        if self.current_user_files.is_some() || self.current_search_files.is_some()
                        {
                            Focus::Files
                        } else if !self.downloads.is_empty() {
                            Focus::Downloads
                        } else {
                            Focus::Search
                        }
                    }
                    Focus::Files => {
                        if !self.downloads.is_empty() {
                            Focus::Downloads
                        } else {
                            Focus::Search
                        }
                    }
                    Focus::Downloads => Focus::Search,
                    Focus::Playlist => {
                        if !self.downloads.is_empty() {
                            Focus::Downloads
                        } else {
                            Focus::Search
                        }
                    }
                };
            }
            KeyCode::Esc => {
                if self.focus == Focus::Files {
                    self.focus = Focus::Results;
                    self.current_user_files = None;
                    self.current_search_files = None;
                } else if self.focus == Focus::Downloads {
                    self.focus = Focus::Results;
                } else if self.focus == Focus::Playlist {
                    self.spotify_playlist = None;
                    self.focus = Focus::Search;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10)
            }
            KeyCode::PageDown => self.move_selection(20),
            KeyCode::PageUp => self.move_selection(-20),
            KeyCode::Char('g') => self.jump_to_start(),
            KeyCode::Char('G') => self.jump_to_end(),
            KeyCode::Home => self.jump_to_start(),
            KeyCode::End => self.jump_to_end(),
            KeyCode::Enter | KeyCode::Char('b') if self.focus == Focus::Results => {
                if !self.search_results.is_empty() {
                    let result = &self.search_results[self.selected_result];
                    let username = result.username.clone();
                    let files = result.files.clone();
                    self.current_search_files = Some((username.clone(), files));
                    self.focus = Focus::Files;
                    self.selected_file = 0;
                    self.file_scroll = 0;
                    self.status = format!(
                        "Showing {} matching files from {}",
                        result.files.len(),
                        username
                    );
                }
            }
            KeyCode::Char('d') if self.focus == Focus::Files => {
                self.download_selected_file();
            }
            KeyCode::Enter if self.focus == Focus::Files => {
                self.download_selected_file();
            }
            KeyCode::Enter if self.focus == Focus::Playlist => {
                self.search_selected_playlist_track();
            }
            KeyCode::Char('a') if self.focus == Focus::Playlist => {
                self.search_all_playlist_tracks();
            }
            KeyCode::Char('D') if self.focus == Focus::Playlist => {
                self.download_all_matched_tracks();
            }
            KeyCode::Char('r') if self.focus == Focus::Downloads => {
                self.retry_failed_download();
            }
            _ => {}
        }
    }

    fn retry_failed_download(&mut self) {
        if self.selected_download < self.downloads.len() {
            let download = &self.downloads[self.selected_download];
            if matches!(download.status, DownloadStatus::Failed(_)) {
                let query = Self::filename_to_search_query(&download.filename);
                self.search_input = query.clone();
                self.cursor_position = query.len();
                let _ = self.cmd_tx.send(ClientCommand::Search(query.clone()));
                self.search_results.clear();
                self.selected_result = 0;
                self.focus = Focus::Results;
                self.status = format!("Re-searching for: {}", query);
            } else {
                self.status = "Can only retry failed downloads".to_string();
            }
        }
    }

    fn filename_to_search_query(filename: &str) -> String {
        let name = std::path::Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);

        name.replace(['_', '-', '.'], " ")
            .split_whitespace()
            .filter(|word| {
                let lower = word.to_lowercase();
                !matches!(
                    lower.as_str(),
                    "flac"
                        | "mp3"
                        | "wav"
                        | "ogg"
                        | "m4a"
                        | "320"
                        | "256"
                        | "128"
                        | "192"
                        | "24bit"
                        | "16bit"
                )
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn download_selected_file(&mut self) {
        if let Some((username, files)) = &self.current_search_files
            && self.selected_file < files.len()
        {
            let file = &files[self.selected_file];
            let _ = self.cmd_tx.send(ClientCommand::DownloadFile {
                username: username.clone(),
                filename: file.filename.clone(),
                size: file.size,
            });
        }
    }

    fn search_selected_playlist_track(&mut self) {
        if let Some(playlist) = &self.spotify_playlist
            && let Some(track) = playlist.tracks.get(self.selected_playlist_track)
        {
            let _ = self.cmd_tx.send(ClientCommand::SearchSpotifyTrack {
                track_index: self.selected_playlist_track,
                query: track.search_query.clone(),
            });
        }
    }

    fn search_all_playlist_tracks(&mut self) {
        if let Some(playlist) = &self.spotify_playlist {
            for (i, track) in playlist.tracks.iter().enumerate() {
                if track.matched_file.is_none() {
                    let _ = self.cmd_tx.send(ClientCommand::SearchSpotifyTrack {
                        track_index: i,
                        query: track.search_query.clone(),
                    });
                }
            }
            self.status = format!(
                "Searching for {} unmatched tracks...",
                playlist.unmatched_tracks().count()
            );
        }
    }

    fn download_all_matched_tracks(&mut self) {
        if let Some(playlist) = &self.spotify_playlist {
            let matched: Vec<_> = playlist
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.matched_file.is_some())
                .collect();

            for (i, _) in &matched {
                let _ = self
                    .cmd_tx
                    .send(ClientCommand::DownloadSpotifyTrack { track_index: *i });
            }
            self.status = format!("Queued {} matched tracks for download", matched.len());
        }
    }

    fn move_selection(&mut self, delta: i32) {
        match self.focus {
            Focus::Results => {
                let len = self.search_results.len();
                if len > 0 {
                    let new_pos = (self.selected_result as i32 + delta).clamp(0, len as i32 - 1);
                    self.selected_result = new_pos as usize;
                }
            }
            Focus::Files => {
                let total = self.file_count();
                if total > 0 {
                    let new_pos = (self.selected_file as i32 + delta).clamp(0, total as i32 - 1);
                    self.selected_file = new_pos as usize;
                }
            }
            Focus::Downloads => {
                let len = self.downloads.len();
                if len > 0 {
                    let new_pos = (self.selected_download as i32 + delta).clamp(0, len as i32 - 1);
                    self.selected_download = new_pos as usize;
                }
            }
            Focus::Playlist => {
                if let Some(playlist) = &self.spotify_playlist {
                    let len = playlist.tracks.len();
                    if len > 0 {
                        let new_pos =
                            (self.selected_playlist_track as i32 + delta).clamp(0, len as i32 - 1);
                        self.selected_playlist_track = new_pos as usize;
                    }
                }
            }
            Focus::Search => {}
        }
    }

    fn jump_to_start(&mut self) {
        match self.focus {
            Focus::Results => self.selected_result = 0,
            Focus::Files => self.selected_file = 0,
            Focus::Downloads => self.selected_download = 0,
            Focus::Playlist => self.selected_playlist_track = 0,
            Focus::Search => {}
        }
    }

    fn jump_to_end(&mut self) {
        match self.focus {
            Focus::Results => {
                if !self.search_results.is_empty() {
                    self.selected_result = self.search_results.len() - 1;
                }
            }
            Focus::Files => {
                let total = self.file_count();
                if total > 0 {
                    self.selected_file = total - 1;
                }
            }
            Focus::Downloads => {
                if !self.downloads.is_empty() {
                    self.selected_download = self.downloads.len() - 1;
                }
            }
            Focus::Playlist => {
                if let Some(playlist) = &self.spotify_playlist
                    && !playlist.tracks.is_empty()
                {
                    self.selected_playlist_track = playlist.tracks.len() - 1;
                }
            }
            Focus::Search => {}
        }
    }

    fn file_count(&self) -> usize {
        if let Some((_, files)) = &self.current_search_files {
            files.len()
        } else if let Some((_, dirs)) = &self.current_user_files {
            dirs.iter().map(|d| d.files.len() + 1).sum()
        } else {
            0
        }
    }

    pub fn get_current_files_flat(&self) -> Vec<(String, Option<&slsk_rs::peer::SharedFile>)> {
        let mut items = Vec::new();
        if let Some((_, dirs)) = &self.current_user_files {
            for dir in dirs {
                items.push((dir.path.clone(), None));
                for file in &dir.files {
                    items.push((format!("  {}", file.filename), Some(file)));
                }
            }
        }
        items
    }
}
