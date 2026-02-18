use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use slsk_rs::constants::{
    ConnectionType, DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT, TransferDirection,
};
use slsk_rs::db::Database;
use slsk_rs::file::{FileOffset, FileTransferInit};
use slsk_rs::peer::{PeerMessage, SearchResultFile, SharedDirectory, read_peer_message};
use slsk_rs::peer_init::{
    PeerInitMessage, peer_init_message_size, read_peer_init_message, write_peer_init_message,
};
use slsk_rs::protocol::MessageWrite;
use slsk_rs::server::{ServerRequest, ServerResponse, read_server_message};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc};

use crate::app::{AppEvent, ClientCommand, SearchResult};
use crate::spotify::{MatchedFile, SoulseekPlaylist, SpotifyClient, SpotifyResource};

const SEARCH_AGGREGATION_TIMEOUT: Duration = Duration::from_secs(5);

const SEARCH_RATE_LIMIT_MAX: usize = 34;
const SEARCH_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(220);

#[derive(Debug, Clone)]
enum QueuedSearch {
    Regular { query: String },
    SpotifyTrack { track_index: usize, query: String },
    RetryDownload { download_id: u32, original_filename: String, query: String },
}

#[derive(Debug)]
struct SearchRateLimiter {
    search_timestamps: VecDeque<Instant>,
    queued_searches: VecDeque<QueuedSearch>,
}

impl SearchRateLimiter {
    fn new() -> Self {
        Self {
            search_timestamps: VecDeque::new(),
            queued_searches: VecDeque::new(),
        }
    }

    fn prune_old_searches(&mut self) {
        let cutoff = Instant::now() - SEARCH_RATE_LIMIT_WINDOW;
        while let Some(&ts) = self.search_timestamps.front() {
            if ts < cutoff {
                self.search_timestamps.pop_front();
            } else {
                break;
            }
        }
    }

    fn can_search(&mut self) -> bool {
        self.prune_old_searches();
        self.search_timestamps.len() < SEARCH_RATE_LIMIT_MAX
    }

    fn record_search(&mut self) {
        self.search_timestamps.push_back(Instant::now());
    }

    fn time_until_next_slot(&mut self) -> Option<Duration> {
        self.prune_old_searches();
        if self.search_timestamps.len() < SEARCH_RATE_LIMIT_MAX {
            return None;
        }
        self.search_timestamps
            .front()
            .map(|&ts| (ts + SEARCH_RATE_LIMIT_WINDOW).saturating_duration_since(Instant::now()))
    }

    fn searches_remaining(&mut self) -> usize {
        self.prune_old_searches();
        SEARCH_RATE_LIMIT_MAX.saturating_sub(self.search_timestamps.len())
    }

    fn queue_search(&mut self, search: QueuedSearch) {
        self.queued_searches.push_back(search);
    }

    fn pop_queued(&mut self) -> Option<QueuedSearch> {
        self.queued_searches.pop_front()
    }

    fn queued_count(&self) -> usize {
        self.queued_searches.len()
    }
}

#[derive(Debug, Clone)]
struct AccumulatedResult {
    username: String,
    file: SearchResultFile,
}

#[derive(Debug)]
struct PendingSpotifySearch {
    track_index: usize,
    results: Vec<AccumulatedResult>,
}

#[derive(Debug)]
struct PendingRetrySearch {
    download_id: u32,
    #[allow(dead_code)]
    original_filename: String,
    results: Vec<AccumulatedResult>,
}

static TOKEN_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_token() -> u32 {
    TOKEN_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone)]
struct PendingDownload {
    id: u32,
    #[allow(dead_code)]
    username: String,
    filename: String,
    size: u64,
    token: u32,
}

struct ClientState {
    username: String,
    pending_searches: HashMap<u32, String>,
    pending_browse: HashMap<String, ()>,
    pending_downloads: HashMap<String, Vec<PendingDownload>>,
    active_download_users: std::collections::HashSet<String>,
    spotify_playlist: Option<SoulseekPlaylist>,
    spotify_track_searches: HashMap<u32, PendingSpotifySearch>,
    retry_searches: HashMap<u32, PendingRetrySearch>,
    rate_limiter: SearchRateLimiter,
}

async fn execute_search(
    search: QueuedSearch,
    state: &Arc<Mutex<ClientState>>,
    write_tx: &mpsc::UnboundedSender<BytesMut>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
) {
    let token = next_token();
    match search {
        QueuedSearch::Regular { query } => {
            {
                let mut st = state.lock().await;
                st.pending_searches.insert(token, query.clone());
                st.rate_limiter.record_search();
            }
            let req = ServerRequest::FileSearch {
                token,
                query: query.clone(),
            };
            let mut buf = BytesMut::new();
            req.write_message(&mut buf);
            let _ = write_tx.send(buf);

            let remaining = {
                let mut st = state.lock().await;
                st.rate_limiter.searches_remaining()
            };
            let _ = event_tx.send(AppEvent::StatusMessage(format!(
                "Searching '{}' ({} searches remaining)",
                query, remaining
            )));
        }
        QueuedSearch::SpotifyTrack { track_index, query } => {
            {
                let mut st = state.lock().await;
                st.pending_searches.insert(token, query.clone());
                st.spotify_track_searches.insert(
                    token,
                    PendingSpotifySearch {
                        track_index,
                        results: Vec::new(),
                    },
                );
                st.rate_limiter.record_search();
            }
            let _ = event_tx.send(AppEvent::SpotifyTrackSearching { track_index });
            let req = ServerRequest::FileSearch {
                token,
                query: query.clone(),
            };
            let mut buf = BytesMut::new();
            req.write_message(&mut buf);
            let _ = write_tx.send(buf);

            let remaining = {
                let mut st = state.lock().await;
                st.rate_limiter.searches_remaining()
            };
            let _ = event_tx.send(AppEvent::StatusMessage(format!(
                "Searching track '{}' ({} searches remaining)",
                query, remaining
            )));
        }
        QueuedSearch::RetryDownload { download_id, original_filename, query } => {
            {
                let mut st = state.lock().await;
                st.pending_searches.insert(token, query.clone());
                st.retry_searches.insert(
                    token,
                    PendingRetrySearch {
                        download_id,
                        original_filename,
                        results: Vec::new(),
                    },
                );
                st.rate_limiter.record_search();
            }
            let req = ServerRequest::FileSearch {
                token,
                query: query.clone(),
            };
            let mut buf = BytesMut::new();
            req.write_message(&mut buf);
            let _ = write_tx.send(buf);

            let remaining = {
                let mut st = state.lock().await;
                st.rate_limiter.searches_remaining()
            };
            let _ = event_tx.send(AppEvent::StatusMessage(format!(
                "Searching alternative '{}' ({} remaining)",
                query, remaining
            )));
        }
    }
}

async fn try_execute_or_queue_search(
    search: QueuedSearch,
    state: &Arc<Mutex<ClientState>>,
    write_tx: &mpsc::UnboundedSender<BytesMut>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    rate_limit_tx: &mpsc::UnboundedSender<()>,
) {
    let (can_search, wait_time, queued_count) = {
        let mut st = state.lock().await;
        let can = st.rate_limiter.can_search();
        let wait = st.rate_limiter.time_until_next_slot();
        if !can {
            st.rate_limiter.queue_search(search.clone());
        }
        (can, wait, st.rate_limiter.queued_count())
    };

    if can_search {
        execute_search(search, state, write_tx, event_tx).await;
    } else {
        let wait_secs = wait_time.map(|d| d.as_secs()).unwrap_or(0);
        let _ = event_tx.send(AppEvent::StatusMessage(format!(
            "Rate limited! {} searches queued, next slot in {}s",
            queued_count, wait_secs
        )));
        let _ = rate_limit_tx.send(());
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
                "flac" | "mp3" | "wav" | "ogg" | "m4a" | "320" | "256" | "128" | "192" | "24bit" | "16bit"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub async fn run_client(
    username: &str,
    password: &str,
    event_tx: mpsc::UnboundedSender<AppEvent>,
    mut cmd_rx: mpsc::UnboundedReceiver<ClientCommand>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind("0.0.0.0:0").await?;
    let listen_port = listener.local_addr()?.port();

    let server_host =
        std::env::var("SOULSEEK_SERVER").unwrap_or_else(|_| DEFAULT_SERVER_HOST.to_string());
    let server_port: u16 = std::env::var("SOULSEEK_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_SERVER_PORT);
    let mut stream = TcpStream::connect((&*server_host, server_port)).await?;
    stream.set_nodelay(true)?;
    let _ = event_tx.send(AppEvent::Connected);

    let login = ServerRequest::Login {
        username: username.to_string(),
        password: password.to_string(),
        version: 160,
        minor_version: 3,
    };

    let mut buf = BytesMut::new();
    login.write_message(&mut buf);
    stream.write_all(&buf).await?;
    stream.flush().await?;

    // Wait for login response before proceeding
    let mut read_buf = BytesMut::with_capacity(65536);
    loop {
        let n = stream.read_buf(&mut read_buf).await?;
        if n == 0 {
            return Err("Connection closed before login response".into());
        }

        if read_buf.len() >= 4 {
            let msg_len =
                u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;

            if read_buf.len() >= 4 + msg_len {
                let mut msg_buf = read_buf.split_to(4 + msg_len);

                match read_server_message(&mut msg_buf) {
                    Ok(ServerResponse::LoginSuccess { .. }) => {
                        let _ = event_tx.send(AppEvent::LoginSuccess {
                            username: username.to_string(),
                        });
                        break;
                    }
                    Ok(ServerResponse::LoginFailure { reason, detail }) => {
                        let _ = event_tx.send(AppEvent::LoginFailed {
                            reason: format!("{:?}: {}", reason, detail.unwrap_or_default()),
                        });
                        return Err("Login failed".into());
                    }
                    Ok(_) => {
                        // Ignore other messages during login
                    }
                    Err(e) => {
                        return Err(format!("Failed to parse login response: {e}").into());
                    }
                }
            }
        }
    }

    // Send SetStatus and SetWaitPort after successful login
    buf.clear();
    let set_status = ServerRequest::SetStatus {
        status: slsk_rs::constants::UserStatus::Online,
    };
    set_status.write_message(&mut buf);
    stream.write_all(&buf).await?;

    buf.clear();
    let set_port = ServerRequest::SetWaitPort {
        port: listen_port as u32,
        obfuscation_type: None,
        obfuscated_port: None,
    };
    set_port.write_message(&mut buf);
    stream.write_all(&buf).await?;
    stream.flush().await?;

    let state = Arc::new(Mutex::new(ClientState {
        username: username.to_string(),
        pending_searches: HashMap::new(),
        pending_browse: HashMap::new(),
        pending_downloads: HashMap::new(),
        active_download_users: std::collections::HashSet::new(),
        spotify_playlist: None,
        spotify_track_searches: HashMap::new(),
        retry_searches: HashMap::new(),
        rate_limiter: SearchRateLimiter::new(),
    }));

    let (write_tx, mut write_rx) = mpsc::unbounded_channel::<BytesMut>();
    let (search_timeout_tx, mut search_timeout_rx) = mpsc::unbounded_channel::<u32>();
    let (rate_limit_tx, mut rate_limit_rx) = mpsc::unbounded_channel::<()>();
    let (read_stream, mut write_stream) = stream.into_split();

    let state_for_listener = state.clone();
    let event_tx_for_listener = event_tx.clone();
    let search_timeout_tx_for_listener = search_timeout_tx.clone();
    let listen_handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let state = state_for_listener.clone();
                    let event_tx = event_tx_for_listener.clone();
                    let search_timeout_tx = search_timeout_tx_for_listener.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            handle_incoming_peer(stream, &state, &event_tx, &search_timeout_tx)
                                .await
                        {
                            let _ =
                                event_tx.send(AppEvent::Error(format!("Incoming peer error: {e}")));
                        }
                    });
                }
                Err(e) => {
                    eprintln!("Accept error: {e}");
                }
            }
        }
    });

    let write_handle = tokio::spawn(async move {
        while let Some(data) = write_rx.recv().await {
            if let Err(e) = write_stream.write_all(&data).await {
                eprintln!("Write error: {e}");
                break;
            }
            if let Err(e) = write_stream.flush().await {
                eprintln!("Flush error: {e}");
                break;
            }
        }
    });

    let state_for_cmd = state.clone();
    let write_tx_for_cmd = write_tx.clone();
    let event_tx_for_cmd = event_tx.clone();
    let rate_limit_tx_for_cmd = rate_limit_tx.clone();
    let cmd_handle = tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                ClientCommand::Search(query) => {
                    try_execute_or_queue_search(
                        QueuedSearch::Regular { query },
                        &state_for_cmd,
                        &write_tx_for_cmd,
                        &event_tx_for_cmd,
                        &rate_limit_tx_for_cmd,
                    )
                    .await;
                }
                ClientCommand::BrowseUser(username) => {
                    {
                        let mut st = state_for_cmd.lock().await;
                        st.pending_browse.insert(username.clone(), ());
                    }
                    let req = ServerRequest::GetPeerAddress { username };
                    let mut buf = BytesMut::new();
                    req.write_message(&mut buf);
                    let _ = write_tx_for_cmd.send(buf);
                }
                ClientCommand::DownloadFile {
                    username,
                    filename,
                    size,
                } => {
                    let download_id = next_token();
                    let transfer_token = next_token();

                    let download = PendingDownload {
                        id: download_id,
                        username: username.clone(),
                        filename: filename.clone(),
                        size,
                        token: transfer_token,
                    };

                    let should_request_address = {
                        let mut st = state_for_cmd.lock().await;
                        st.pending_downloads
                            .entry(username.clone())
                            .or_default()
                            .push(download);
                        !st.active_download_users.contains(&username)
                    };

                    let _ = event_tx_for_cmd.send(AppEvent::DownloadQueued {
                        id: download_id,
                        username: username.clone(),
                        filename: filename.clone(),
                        size,
                    });

                    if should_request_address {
                        let req = ServerRequest::GetPeerAddress { username };
                        let mut buf = BytesMut::new();
                        req.write_message(&mut buf);
                        let _ = write_tx_for_cmd.send(buf);
                    }
                }
                ClientCommand::FetchSpotify(url) => {
                    let event_tx = event_tx_for_cmd.clone();
                    let state = state_for_cmd.clone();
                    tokio::spawn(async move {
                        match fetch_spotify_playlist(&url).await {
                            Ok(playlist) => {
                                {
                                    let mut st = state.lock().await;
                                    st.spotify_playlist = Some(playlist.clone());
                                }
                                let _ = event_tx.send(AppEvent::SpotifyLoaded(playlist));
                            }
                            Err(e) => {
                                let _ = event_tx.send(AppEvent::SpotifyError(e.to_string()));
                            }
                        }
                    });
                }
                ClientCommand::SearchSpotifyTrack { track_index, query } => {
                    try_execute_or_queue_search(
                        QueuedSearch::SpotifyTrack { track_index, query },
                        &state_for_cmd,
                        &write_tx_for_cmd,
                        &event_tx_for_cmd,
                        &rate_limit_tx_for_cmd,
                    )
                    .await;
                }
                ClientCommand::DownloadSpotifyTrack { track_index } => {
                    let matched_file = {
                        let st = state_for_cmd.lock().await;
                        st.spotify_playlist
                            .as_ref()
                            .and_then(|p| p.tracks.get(track_index))
                            .and_then(|t| t.matched_file.clone())
                    };

                    if let Some(matched) = matched_file {
                        let download_id = next_token();
                        let transfer_token = next_token();

                        let download = PendingDownload {
                            id: download_id,
                            username: matched.username.clone(),
                            filename: matched.filename.clone(),
                            size: matched.size,
                            token: transfer_token,
                        };

                        let should_request_address = {
                            let mut st = state_for_cmd.lock().await;
                            st.pending_downloads
                                .entry(matched.username.clone())
                                .or_default()
                                .push(download);
                            !st.active_download_users.contains(&matched.username)
                        };

                        let _ = event_tx_for_cmd.send(AppEvent::DownloadQueued {
                            id: download_id,
                            username: matched.username.clone(),
                            filename: matched.filename.clone(),
                            size: matched.size,
                        });

                        if should_request_address {
                            let req = ServerRequest::GetPeerAddress {
                                username: matched.username.clone(),
                            };
                            let mut buf = BytesMut::new();
                            req.write_message(&mut buf);
                            let _ = write_tx_for_cmd.send(buf);
                        }
                    }
                }
                ClientCommand::RetryDownload {
                    download_id,
                    original_filename,
                } => {
                    let query = filename_to_search_query(&original_filename);
                    try_execute_or_queue_search(
                        QueuedSearch::RetryDownload {
                            download_id,
                            original_filename,
                            query,
                        },
                        &state_for_cmd,
                        &write_tx_for_cmd,
                        &event_tx_for_cmd,
                        &rate_limit_tx_for_cmd,
                    )
                    .await;
                }
                ClientCommand::RetryDownloadFile {
                    download_id,
                    username,
                    filename,
                    size,
                } => {
                    let transfer_token = next_token();

                    let download = PendingDownload {
                        id: download_id,
                        username: username.clone(),
                        filename: filename.clone(),
                        size,
                        token: transfer_token,
                    };

                    let should_request_address = {
                        let mut st = state_for_cmd.lock().await;
                        st.pending_downloads
                            .entry(username.clone())
                            .or_default()
                            .push(download);
                        !st.active_download_users.contains(&username)
                    };

                    if should_request_address {
                        let req = ServerRequest::GetPeerAddress { username };
                        let mut buf = BytesMut::new();
                        req.write_message(&mut buf);
                        let _ = write_tx_for_cmd.send(buf);
                    }
                }
            }
        }
    });

    let mut read_buf = BytesMut::with_capacity(65536);
    let mut read_stream = read_stream;

    loop {
        tokio::select! {
            result = read_stream.read_buf(&mut read_buf) => {
                let n = result?;
                if n == 0 {
                    break;
                }

                while read_buf.len() >= 4 {
                    let msg_len = u32::from_le_bytes([
                        read_buf[0], read_buf[1], read_buf[2], read_buf[3]
                    ]) as usize;

                    if read_buf.len() < 4 + msg_len {
                        break;
                    }

                    let mut msg_buf = read_buf.split_to(4 + msg_len);

                    match read_server_message(&mut msg_buf) {
                        Ok(response) => {
                            handle_server_response(
                                response,
                                &state,
                                &event_tx,
                                &write_tx,
                                listen_port,
                                &search_timeout_tx,
                            ).await;
                        }
                        Err(e) => {
                            let _ = event_tx.send(AppEvent::Error(format!("Parse error: {e}")));
                        }
                    }
                }
            }
            Some(token) = search_timeout_rx.recv() => {
                let mut st = state.lock().await;
                finalize_search(token, &mut st, &event_tx);
                finalize_retry_search(token, &mut st, &event_tx);
            }
            Some(()) = rate_limit_rx.recv() => {
                let wait_time = {
                    let mut st = state.lock().await;
                    st.rate_limiter.time_until_next_slot()
                };

                if let Some(wait) = wait_time {
                    let state_clone = state.clone();
                    let write_tx_clone = write_tx.clone();
                    let event_tx_clone = event_tx.clone();
                    let rate_limit_tx_clone = rate_limit_tx.clone();

                    tokio::spawn(async move {
                        tokio::time::sleep(wait + Duration::from_millis(100)).await;

                        loop {
                            let queued = {
                                let mut st = state_clone.lock().await;
                                if st.rate_limiter.can_search() {
                                    st.rate_limiter.pop_queued()
                                } else {
                                    None
                                }
                            };

                            match queued {
                                Some(search) => {
                                    execute_search(
                                        search,
                                        &state_clone,
                                        &write_tx_clone,
                                        &event_tx_clone,
                                    ).await;

                                    let (more_queued, can_continue) = {
                                        let mut st = state_clone.lock().await;
                                        (st.rate_limiter.queued_count() > 0, st.rate_limiter.can_search())
                                    };

                                    if !more_queued {
                                        break;
                                    }
                                    if !can_continue {
                                        let _ = rate_limit_tx_clone.send(());
                                        break;
                                    }
                                }
                                None => {
                                    let has_queued = {
                                        let st = state_clone.lock().await;
                                        st.rate_limiter.queued_count() > 0
                                    };
                                    if has_queued {
                                        let _ = rate_limit_tx_clone.send(());
                                    }
                                    break;
                                }
                            }
                        }
                    });
                }
            }
        }
    }

    write_handle.abort();
    cmd_handle.abort();
    listen_handle.abort();

    Ok(())
}

async fn handle_server_response(
    response: ServerResponse,
    state: &Arc<Mutex<ClientState>>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    _tx_to_server: &mpsc::UnboundedSender<BytesMut>,
    _listen_port: u16,
    search_timeout_tx: &mpsc::UnboundedSender<u32>,
) {
    match response {
        ServerResponse::LoginSuccess { .. } | ServerResponse::LoginFailure { .. } => {
            // Already handled before main loop
        }
        ServerResponse::GetPeerAddress {
            username, ip, port, ..
        } => {
            let (should_browse, downloads_for_user) = {
                let mut st = state.lock().await;
                let browse = st.pending_browse.contains_key(&username);
                let downloads = st.pending_downloads.remove(&username).unwrap_or_default();
                (browse, downloads)
            };

            if should_browse {
                let state_clone = state.clone();
                let event_tx_clone = event_tx.clone();
                let username_clone = username.clone();

                tokio::spawn(async move {
                    match connect_to_peer_and_browse(&username_clone, ip, port, &state_clone).await
                    {
                        Ok(dirs) => {
                            let _ = event_tx_clone.send(AppEvent::UserFiles(username_clone, dirs));
                        }
                        Err(e) => {
                            let _ = event_tx_clone.send(AppEvent::Error(format!(
                                "Failed to browse {username_clone}: {e}"
                            )));
                        }
                    }
                });

                let mut st = state.lock().await;
                st.pending_browse.remove(&username);
            }

            if !downloads_for_user.is_empty() {
                let state_clone = state.clone();
                let event_tx_clone = event_tx.clone();
                let username_for_task = username.clone();

                {
                    let mut st = state.lock().await;
                    st.active_download_users.insert(username_for_task.clone());
                }

                tokio::spawn(async move {
                    let mut downloads_queue = downloads_for_user;

                    loop {
                        for download in downloads_queue {
                            if let Err(e) = connect_to_peer_and_download(
                                ip,
                                port,
                                download.clone(),
                                &state_clone,
                                &event_tx_clone,
                            )
                            .await
                            {
                                let _ = event_tx_clone.send(AppEvent::DownloadFailed {
                                    id: download.id,
                                    reason: e.to_string(),
                                });
                            }
                        }

                        let more_downloads = {
                            let mut st = state_clone.lock().await;
                            st.pending_downloads
                                .remove(&username_for_task)
                                .unwrap_or_default()
                        };

                        if more_downloads.is_empty() {
                            let mut st = state_clone.lock().await;
                            st.active_download_users.remove(&username_for_task);
                            break;
                        }

                        downloads_queue = more_downloads;
                    }
                });
            }
        }
        ServerResponse::ConnectToPeer {
            username,
            connection_type,
            ip,
            port,
            token,
            ..
        } => {
            if connection_type == ConnectionType::Peer {
                let state_clone = state.clone();
                let event_tx_clone = event_tx.clone();
                let search_timeout_tx_clone = search_timeout_tx.clone();

                tokio::spawn(async move {
                    let _ = handle_peer_connection(
                        &username,
                        ip,
                        port,
                        token,
                        &state_clone,
                        &event_tx_clone,
                        &search_timeout_tx_clone,
                    )
                    .await;
                });
            }
        }
        _ => {}
    }
}

async fn connect_to_peer_and_browse(
    _username: &str,
    ip: Ipv4Addr,
    port: u32,
    state: &Arc<Mutex<ClientState>>,
) -> Result<Vec<SharedDirectory>, Box<dyn std::error::Error + Send + Sync>> {
    let my_username = {
        let st = state.lock().await;
        st.username.clone()
    };

    let addr = format!("{}:{}", ip, port);
    let mut stream = TcpStream::connect(&addr).await?;

    let token = next_token();
    let init = PeerInitMessage::PeerInit {
        username: my_username,
        connection_type: ConnectionType::Peer,
        token,
    };
    let mut buf = BytesMut::new();
    write_peer_init_message(&init, &mut buf);
    stream.write_all(&buf).await?;

    buf.clear();
    let request = PeerMessage::SharedFileListRequest;
    request.write_message(&mut buf);
    stream.write_all(&buf).await?;

    let mut read_buf = BytesMut::with_capacity(1024 * 1024);

    loop {
        let n = stream.read_buf(&mut read_buf).await?;
        if n == 0 {
            return Err("Connection closed".into());
        }

        if read_buf.len() >= 4 {
            let msg_len =
                u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;

            if read_buf.len() >= 4 + msg_len {
                let mut msg_buf = read_buf.split_to(4 + msg_len);

                match read_peer_message(&mut msg_buf) {
                    Ok(PeerMessage::SharedFileListResponse { directories, .. }) => {
                        return Ok(directories);
                    }
                    Ok(_) => {
                        continue;
                    }
                    Err(e) => {
                        return Err(format!("Failed to parse peer message: {e}").into());
                    }
                }
            }
        }
    }
}

async fn handle_peer_connection(
    _username: &str,
    ip: Ipv4Addr,
    port: u32,
    token: u32,
    state: &Arc<Mutex<ClientState>>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    search_timeout_tx: &mpsc::UnboundedSender<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = format!("{}:{}", ip, port);
    let mut stream = TcpStream::connect(&addr).await?;

    // Send PierceFirewall - we're responding to ConnectToPeer (indirect connection)
    let pierce = PeerInitMessage::PierceFirewall { token };
    let mut buf = BytesMut::new();
    write_peer_init_message(&pierce, &mut buf);
    stream.write_all(&buf).await?;

    let mut read_buf = BytesMut::with_capacity(65536);

    loop {
        let n = stream.read_buf(&mut read_buf).await?;
        if n == 0 {
            break;
        }

        while read_buf.len() >= 4 {
            let msg_len =
                u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;

            if read_buf.len() < 4 + msg_len {
                break;
            }

            let mut msg_buf = read_buf.split_to(4 + msg_len);

            match read_peer_message(&mut msg_buf) {
                Ok(PeerMessage::FileSearchResponse {
                    username: result_user,
                    token,
                    results,
                    slot_free,
                    avg_speed,
                    queue_length,
                    ..
                }) => {
                    let pending = {
                        let st = state.lock().await;
                        st.pending_searches.contains_key(&token)
                    };

                    if pending && !results.is_empty() {
                        let (is_spotify_search, is_retry_search) = {
                            let st = state.lock().await;
                            (
                                st.spotify_track_searches.contains_key(&token),
                                st.retry_searches.contains_key(&token),
                            )
                        };

                        if is_spotify_search {
                            accumulate_search_results(
                                token,
                                &result_user,
                                results,
                                state,
                                event_tx,
                                search_timeout_tx,
                            )
                            .await;
                        } else if is_retry_search {
                            accumulate_retry_results(
                                token,
                                &result_user,
                                results,
                                state,
                                event_tx,
                                search_timeout_tx,
                            )
                            .await;
                        } else {
                            let _ = event_tx.send(AppEvent::SearchResult(SearchResult {
                                username: result_user,
                                slot_free,
                                avg_speed,
                                queue_length,
                                files: results,
                            }));
                        }
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }

    Ok(())
}

async fn connect_to_peer_and_download(
    ip: Ipv4Addr,
    port: u32,
    download: PendingDownload,
    state: &Arc<Mutex<ClientState>>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let my_username = {
        let st = state.lock().await;
        st.username.clone()
    };

    let addr = format!("{}:{}", ip, port);
    let mut stream = TcpStream::connect(&addr).await?;

    let init = PeerInitMessage::PeerInit {
        username: my_username,
        connection_type: ConnectionType::Peer,
        token: download.token,
    };
    let mut buf = BytesMut::new();
    write_peer_init_message(&init, &mut buf);
    stream.write_all(&buf).await?;

    buf.clear();
    let queue_msg = PeerMessage::QueueUpload {
        filename: download.filename.clone(),
    };
    queue_msg.write_message(&mut buf);
    stream.write_all(&buf).await?;

    let _ = event_tx.send(AppEvent::DownloadStarted { id: download.id });

    let mut read_buf = BytesMut::with_capacity(65536);
    let transfer_started = false;
    let mut file_size = download.size;
    let mut transfer_token: Option<u32> = None;

    loop {
        let n = stream.read_buf(&mut read_buf).await?;
        if n == 0 {
            if transfer_started {
                break;
            }
            return Err("Connection closed before transfer started".into());
        }

        while read_buf.len() >= 4 {
            let msg_len =
                u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;

            if read_buf.len() < 4 + msg_len {
                break;
            }

            let mut msg_buf = read_buf.split_to(4 + msg_len);

            match read_peer_message(&mut msg_buf) {
                Ok(PeerMessage::TransferRequest {
                    direction,
                    token,
                    filename,
                    file_size: size,
                }) => {
                    if direction == TransferDirection::Upload && filename == download.filename {
                        transfer_token = Some(token);
                        if let Some(sz) = size {
                            file_size = sz;
                        }

                        buf.clear();
                        let response = PeerMessage::TransferResponse {
                            token,
                            allowed: true,
                            file_size: None,
                            reason: None,
                        };
                        response.write_message(&mut buf);
                        stream.write_all(&buf).await?;
                    }
                }
                Ok(PeerMessage::PlaceInQueueResponse { filename, place }) => {
                    if filename == download.filename {
                        let _ = event_tx.send(AppEvent::StatusMessage(format!(
                            "Queued at position {} for {}",
                            place, download.filename
                        )));
                    }
                }
                Ok(PeerMessage::UploadDenied { filename, reason }) => {
                    if filename == download.filename {
                        return Err(format!("Upload denied: {}", reason.as_str()).into());
                    }
                }
                Ok(PeerMessage::UploadFailed { filename }) => {
                    if filename == download.filename {
                        return Err("Upload failed".into());
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }

        if transfer_token.is_some() && !transfer_started {
            break;
        }
    }

    let token = transfer_token.ok_or("No transfer token received")?;

    drop(stream);

    let addr = format!("{}:{}", ip, port);
    let mut file_stream = TcpStream::connect(&addr).await?;

    let file_init = PeerInitMessage::PeerInit {
        username: {
            let st = state.lock().await;
            st.username.clone()
        },
        connection_type: ConnectionType::File,
        token: download.token,
    };
    let mut buf = BytesMut::new();
    write_peer_init_message(&file_init, &mut buf);
    file_stream.write_all(&buf).await?;

    buf.clear();
    let transfer_init = FileTransferInit::new(token);
    transfer_init.write_to(&mut buf);
    file_stream.write_all(&buf).await?;

    buf.clear();
    let offset = FileOffset::new(0);
    offset.write_to(&mut buf);
    file_stream.write_all(&buf).await?;

    let download_dir = PathBuf::from("downloads");
    tokio::fs::create_dir_all(&download_dir).await?;

    let filename = download
        .filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&download.filename);
    let file_path = download_dir.join(filename);

    let mut file = File::create(&file_path).await?;
    let mut downloaded: u64 = 0;
    let mut file_buf = vec![0u8; 65536];
    let mut last_progress_update = std::time::Instant::now();

    loop {
        let n = file_stream.read(&mut file_buf).await?;
        if n == 0 {
            break;
        }

        file.write_all(&file_buf[..n]).await?;
        downloaded += n as u64;

        if last_progress_update.elapsed() > std::time::Duration::from_millis(100) {
            let _ = event_tx.send(AppEvent::DownloadProgress {
                id: download.id,
                downloaded,
            });
            last_progress_update = std::time::Instant::now();
        }

        if downloaded >= file_size {
            break;
        }
    }

    let _ = event_tx.send(AppEvent::DownloadCompleted { id: download.id });

    Ok(())
}

async fn fetch_spotify_playlist(
    url: &str,
) -> Result<SoulseekPlaylist, Box<dyn std::error::Error + Send + Sync>> {
    let resource = SpotifyClient::parse_spotify_url(url).ok_or("Invalid Spotify URL")?;

    let mut client = SpotifyClient::from_env()?;

    match resource {
        SpotifyResource::Track(id) => {
            let track = client.get_track(&id).await?;
            Ok(SoulseekPlaylist::from_single_track(track))
        }
        SpotifyResource::Playlist(id) => {
            let playlist = client.get_playlist(&id).await?;
            Ok(SoulseekPlaylist::from_spotify_playlist(playlist))
        }
        SpotifyResource::Album(_) => Err("Album support not yet implemented".into()),
    }
}

fn get_bitrate(attributes: &[slsk_rs::peer::FileAttribute]) -> Option<u32> {
    attributes.iter().find(|a| a.code == 0).map(|a| a.value)
}

fn pick_best_file(results: &[AccumulatedResult]) -> Option<&AccumulatedResult> {
    let audio_exts = [
        ".mp3", ".flac", ".m4a", ".ogg", ".opus", ".wav", ".aac", ".wma", ".ape", ".alac", ".aiff",
        ".aif", ".wv", ".mpc",
    ];

    let mut candidates: Vec<_> = results
        .iter()
        .filter(|r| {
            let lower = r.file.filename.to_lowercase();
            audio_exts.iter().any(|ext| lower.ends_with(ext))
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|a, b| {
        let a_bitrate_opt = get_bitrate(&a.file.attributes);
        let b_bitrate_opt = get_bitrate(&b.file.attributes);

        let a_is_flac = a.file.filename.to_lowercase().ends_with(".flac");
        let b_is_flac = b.file.filename.to_lowercase().ends_with(".flac");

        let a_has_bitrate = a_bitrate_opt.is_some() || a_is_flac;
        let b_has_bitrate = b_bitrate_opt.is_some() || b_is_flac;
        if a_has_bitrate != b_has_bitrate {
            return b_has_bitrate.cmp(&a_has_bitrate);
        }

        if a_is_flac != b_is_flac {
            return b_is_flac.cmp(&a_is_flac);
        }

        let a_bitrate = a_bitrate_opt.unwrap_or(0);
        let b_bitrate = b_bitrate_opt.unwrap_or(0);
        b_bitrate.cmp(&a_bitrate)
    });

    candidates.first().copied()
}

async fn accumulate_search_results(
    token: u32,
    username: &str,
    results: Vec<SearchResultFile>,
    state: &Arc<Mutex<ClientState>>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    search_timeout_tx: &mpsc::UnboundedSender<u32>,
) {
    let should_start_timer = {
        let mut st = state.lock().await;
        if let Some(pending) = st.spotify_track_searches.get_mut(&token) {
            let was_empty = pending.results.is_empty();
            for file in results {
                pending.results.push(AccumulatedResult {
                    username: username.to_string(),
                    file,
                });
            }
            was_empty
        } else {
            false
        }
    };

    if should_start_timer {
        let tx = search_timeout_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SEARCH_AGGREGATION_TIMEOUT).await;
            let _ = tx.send(token);
        });

        let track_index = {
            let st = state.lock().await;
            st.spotify_track_searches.get(&token).map(|p| p.track_index)
        };
        if let Some(idx) = track_index {
            let _ = event_tx.send(AppEvent::StatusMessage(format!(
                "Collecting search results for track {} (waiting {}s)...",
                idx + 1,
                SEARCH_AGGREGATION_TIMEOUT.as_secs()
            )));
        }
    }
}

fn finalize_search(
    token: u32,
    state: &mut ClientState,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
) {
    if let Some(pending) = state.spotify_track_searches.remove(&token) {
        let track_index = pending.track_index;
        let result_count = pending.results.len();

        if let Some(best) = pick_best_file(&pending.results) {
            let matched = MatchedFile {
                username: best.username.clone(),
                filename: best.file.filename.clone(),
                size: best.file.size,
                bitrate: get_bitrate(&best.file.attributes),
            };

            if let Some(playlist) = &mut state.spotify_playlist
                && let Some(track) = playlist.tracks.get_mut(track_index)
            {
                track.matched_file = Some(matched.clone());
            }

            let _ = event_tx.send(AppEvent::SpotifyTrackMatched {
                track_index,
                matched_file: matched,
            });
        } else {
            let _ = event_tx.send(AppEvent::StatusMessage(format!(
                "No audio match found for track {} ({} results checked)",
                track_index + 1,
                result_count
            )));
        }

        state.pending_searches.remove(&token);
    }
}

async fn accumulate_retry_results(
    token: u32,
    username: &str,
    results: Vec<SearchResultFile>,
    state: &Arc<Mutex<ClientState>>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    search_timeout_tx: &mpsc::UnboundedSender<u32>,
) {
    let should_start_timer = {
        let mut st = state.lock().await;
        if let Some(pending) = st.retry_searches.get_mut(&token) {
            let was_empty = pending.results.is_empty();
            for file in results {
                pending.results.push(AccumulatedResult {
                    username: username.to_string(),
                    file,
                });
            }
            was_empty
        } else {
            false
        }
    };

    if should_start_timer {
        let tx = search_timeout_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(SEARCH_AGGREGATION_TIMEOUT).await;
            let _ = tx.send(token);
        });

        let _ = event_tx.send(AppEvent::StatusMessage(
            "Finding alternative sources...".to_string(),
        ));
    }
}

fn finalize_retry_search(
    token: u32,
    state: &mut ClientState,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
) {
    if let Some(pending) = state.retry_searches.remove(&token) {
        let download_id = pending.download_id;

        if let Some(best) = pick_best_file(&pending.results) {
            let matched = MatchedFile {
                username: best.username.clone(),
                filename: best.file.filename.clone(),
                size: best.file.size,
                bitrate: get_bitrate(&best.file.attributes),
            };

            let _ = event_tx.send(AppEvent::RetryDownloadMatched {
                download_id,
                matched_file: matched,
            });
        } else {
            let _ = event_tx.send(AppEvent::RetryDownloadFailed { download_id });
        }

        state.pending_searches.remove(&token);
    }
}

async fn handle_incoming_peer(
    mut stream: TcpStream,
    state: &Arc<Mutex<ClientState>>,
    event_tx: &mpsc::UnboundedSender<AppEvent>,
    search_timeout_tx: &mpsc::UnboundedSender<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut read_buf = BytesMut::with_capacity(65536);

    // Read until we have the complete peer init message
    while peer_init_message_size(&read_buf).is_none() {
        let n = stream.read_buf(&mut read_buf).await?;
        if n == 0 {
            return Err("Connection closed before init message complete".into());
        }
    }

    let init_msg = read_peer_init_message(&mut read_buf)?;

    match init_msg {
        PeerInitMessage::PierceFirewall { .. } => {
            // Firewall pierce - not needed for basic functionality
        }
        PeerInitMessage::PeerInit {
            connection_type, ..
        } => {
            if connection_type == ConnectionType::Peer {
                // Process any data already in buffer, then read more
                loop {
                    // First process any complete messages in the buffer
                    while read_buf.len() >= 4 {
                        let msg_len = u32::from_le_bytes([
                            read_buf[0],
                            read_buf[1],
                            read_buf[2],
                            read_buf[3],
                        ]) as usize;

                        if read_buf.len() < 4 + msg_len {
                            break;
                        }

                        let mut msg_buf = read_buf.split_to(4 + msg_len);

                        match read_peer_message(&mut msg_buf) {
                            Ok(PeerMessage::FileSearchResponse {
                                username: result_user,
                                token,
                                results,
                                slot_free,
                                avg_speed,
                                queue_length,
                                ..
                            }) => {
                                let pending = {
                                    let st = state.lock().await;
                                    st.pending_searches.contains_key(&token)
                                };

                                if pending && !results.is_empty() {
                                    let is_spotify_search = {
                                        let st = state.lock().await;
                                        st.spotify_track_searches.contains_key(&token)
                                    };

                                    if is_spotify_search {
                                        accumulate_search_results(
                                            token,
                                            &result_user,
                                            results,
                                            state,
                                            event_tx,
                                            search_timeout_tx,
                                        )
                                        .await;
                                    } else {
                                        let _ =
                                            event_tx.send(AppEvent::SearchResult(SearchResult {
                                                username: result_user,
                                                slot_free,
                                                avg_speed,
                                                queue_length,
                                                files: results,
                                            }));
                                    }
                                }
                            }
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }

                    // Read more data
                    let n = stream.read_buf(&mut read_buf).await?;
                    if n == 0 {
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}
