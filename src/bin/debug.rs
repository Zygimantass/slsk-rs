use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::BytesMut;
use slsk_rs::constants::{ConnectionType, DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT, TransferDirection};
use slsk_rs::file::{FileOffset, FileTransferInit};
use slsk_rs::peer::{PeerMessage, SearchResultFile, read_peer_message};
use slsk_rs::peer_init::{PeerInitMessage, write_peer_init_message};
use slsk_rs::protocol::MessageWrite;
use slsk_rs::server::{ServerRequest, ServerResponse, read_server_message};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

static TOKEN_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_token() -> u32 {
    TOKEN_COUNTER.fetch_add(1, Ordering::SeqCst)
}

const AGGREGATION_TIMEOUT: Duration = Duration::from_secs(8);
const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TRANSFER_WAIT_TIMEOUT: Duration = Duration::from_secs(60);
const RECONNECT_DELAY: Duration = Duration::from_secs(10);
const MAX_RETRIES: u32 = 3;
const MAX_CANDIDATES: usize = 10;

#[derive(Debug, Clone)]
struct AccumulatedResult {
    username: String,
    file: SearchResultFile,
}

#[derive(Debug, Clone)]
struct SpotifyTrack {
    name: String,
    artist: String,
}

impl SpotifyTrack {
    fn to_search_query(&self) -> String {
        format!("{} {}", self.artist, self.name)
    }

    fn display_name(&self) -> String {
        format!("{} - {}", self.artist, self.name)
    }
}

#[derive(Debug, Clone)]
struct TrackDownload {
    track: SpotifyTrack,
    status: DownloadStatus,
    retry_count: u32,
    tried_users: Vec<String>,
}

#[derive(Debug, Clone)]
struct MatchedFile {
    username: String,
    filename: String,
    size: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum DownloadStatus {
    Pending,
    Searching,
    Downloading,
    Completed,
    Failed(String),
}

fn parse_spotify_url(url: &str) -> Option<(SpotifyResourceType, String)> {
    let url = url.trim();

    if let Some(rest) = url.strip_prefix("spotify:playlist:") {
        return Some((SpotifyResourceType::Playlist, rest.to_string()));
    }
    if let Some(rest) = url.strip_prefix("spotify:track:") {
        return Some((SpotifyResourceType::Track, rest.to_string()));
    }

    if url.contains("open.spotify.com/playlist/") {
        let path = url.split("open.spotify.com/playlist/").nth(1)?;
        let id = path.split('?').next()?;
        return Some((SpotifyResourceType::Playlist, id.to_string()));
    }
    if url.contains("open.spotify.com/track/") {
        let path = url.split("open.spotify.com/track/").nth(1)?;
        let id = path.split('?').next()?;
        return Some((SpotifyResourceType::Track, id.to_string()));
    }

    None
}

#[derive(Debug, Clone, PartialEq)]
enum SpotifyResourceType {
    Track,
    Playlist,
}

async fn get_spotify_token() -> anyhow::Result<String> {
    let client_id = std::env::var("SPOTIFY_CLIENT_ID")?;
    let client_secret = std::env::var("SPOTIFY_CLIENT_SECRET")?;

    let client = reqwest::Client::new();
    let credentials = format!("{}:{}", client_id, client_secret);
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        credentials.as_bytes(),
    );

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let resp: TokenResponse = client
        .post("https://accounts.spotify.com/api/token")
        .header("Authorization", format!("Basic {}", encoded))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(resp.access_token)
}

async fn fetch_spotify_track(token: &str, track_id: &str) -> anyhow::Result<SpotifyTrack> {
    let client = reqwest::Client::new();

    #[derive(serde::Deserialize)]
    struct Artist {
        name: String,
    }

    #[derive(serde::Deserialize)]
    struct Track {
        name: String,
        artists: Vec<Artist>,
    }

    let track: Track = client
        .get(format!("https://api.spotify.com/v1/tracks/{}", track_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let artist = track
        .artists
        .first()
        .map(|a| a.name.clone())
        .unwrap_or_default();

    Ok(SpotifyTrack {
        name: track.name,
        artist,
    })
}

async fn fetch_spotify_playlist(token: &str, playlist_id: &str) -> anyhow::Result<(String, Vec<SpotifyTrack>)> {
    let client = reqwest::Client::new();

    #[derive(serde::Deserialize)]
    struct Artist {
        name: String,
    }

    #[derive(serde::Deserialize)]
    struct Track {
        name: String,
        artists: Vec<Artist>,
    }

    #[derive(serde::Deserialize)]
    struct PlaylistItem {
        track: Option<Track>,
    }

    #[derive(serde::Deserialize)]
    struct PlaylistTracks {
        items: Vec<PlaylistItem>,
        next: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Playlist {
        name: String,
        tracks: PlaylistTracks,
    }

    let playlist: Playlist = client
        .get(format!("https://api.spotify.com/v1/playlists/{}", playlist_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let playlist_name = playlist.name;
    let mut tracks: Vec<SpotifyTrack> = playlist
        .tracks
        .items
        .into_iter()
        .filter_map(|item| {
            item.track.map(|t| SpotifyTrack {
                name: t.name,
                artist: t.artists.first().map(|a| a.name.clone()).unwrap_or_default(),
            })
        })
        .collect();

    let mut next_url = playlist.tracks.next;
    while let Some(url) = next_url {
        let page: PlaylistTracks = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        for item in page.items {
            if let Some(t) = item.track {
                tracks.push(SpotifyTrack {
                    name: t.name,
                    artist: t.artists.first().map(|a| a.name.clone()).unwrap_or_default(),
                });
            }
        }

        next_url = page.next;
    }

    Ok((playlist_name, tracks))
}

fn get_bitrate(attributes: &[slsk_rs::peer::FileAttribute]) -> Option<u32> {
    attributes.iter().find(|a| a.code == 0).map(|a| a.value)
}

fn pick_best_files<'a>(results: &'a [AccumulatedResult], exclude_users: &[String]) -> Vec<&'a AccumulatedResult> {
    let audio_exts = [
        ".mp3", ".flac", ".m4a", ".ogg", ".opus", ".wav", ".aac", ".wma", ".ape", ".alac", ".aiff",
        ".aif", ".wv", ".mpc",
    ];

    let mut candidates: Vec<_> = results
        .iter()
        .filter(|r| {
            let lower = r.file.filename.to_lowercase();
            audio_exts.iter().any(|ext| lower.ends_with(ext))
                && !exclude_users.contains(&r.username)
        })
        .collect();

    if candidates.is_empty() {
        return Vec::new();
    }

    candidates.sort_by(|a, b| {
        let a_bitrate_opt = get_bitrate(&a.file.attributes);
        let b_bitrate_opt = get_bitrate(&b.file.attributes);

        let a_is_flac = a.file.filename.to_lowercase().ends_with(".flac");
        let b_is_flac = b.file.filename.to_lowercase().ends_with(".flac");

        // Prefer files with bitrate info
        let a_has_bitrate = a_bitrate_opt.is_some() || a_is_flac;
        let b_has_bitrate = b_bitrate_opt.is_some() || b_is_flac;
        if a_has_bitrate != b_has_bitrate {
            return b_has_bitrate.cmp(&a_has_bitrate);
        }

        // Prefer FLAC
        if a_is_flac != b_is_flac {
            return b_is_flac.cmp(&a_is_flac);
        }

        // Higher bitrate wins
        let a_bitrate = a_bitrate_opt.unwrap_or(0);
        let b_bitrate = b_bitrate_opt.unwrap_or(0);
        b_bitrate.cmp(&a_bitrate)
    });

    // Return top candidates (unique users)
    let mut seen_users = std::collections::HashSet::new();
    candidates
        .into_iter()
        .filter(|c| seen_users.insert(c.username.clone()))
        .take(MAX_CANDIDATES)
        .collect()
}

struct SoulseekClient {
    stream: TcpStream,
    read_buf: BytesMut,
    username: String,
}

impl SoulseekClient {
    async fn connect_with_retry(username: &str, password: &str, max_attempts: u32) -> anyhow::Result<Self> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match Self::connect_once(username, password).await {
                Ok(client) => return Ok(client),
                Err(e) => {
                    let err_str = e.to_string();
                    if attempt >= max_attempts {
                        return Err(e);
                    }
                    // If rate limited, wait longer
                    let delay = if err_str.contains("rate limit") || err_str.contains("reset") {
                        Duration::from_secs(30 * attempt as u64)
                    } else {
                        // Exponential backoff: 10, 20, 40, 60 seconds
                        Duration::from_secs((10 * 2u64.pow((attempt - 1).min(2))).min(60))
                    };
                    println!("  Connect attempt {}/{} failed: {}", attempt, max_attempts, e);
                    println!("  Retrying in {}s...", delay.as_secs());
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn connect(username: &str, password: &str) -> anyhow::Result<Self> {
        Self::connect_with_retry(username, password, 5).await
    }

    async fn connect_once(username: &str, password: &str) -> anyhow::Result<Self> {
        let server_host = std::env::var("SOULSEEK_SERVER").unwrap_or_else(|_| DEFAULT_SERVER_HOST.to_string());
        let server_port: u16 = std::env::var("SOULSEEK_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_SERVER_PORT);
        
        println!("Connecting to {}:{}...", server_host, server_port);
        let mut stream = TcpStream::connect((&*server_host, server_port)).await?;
        stream.set_nodelay(true)?;
        println!("Connected!");

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

        let mut read_buf = BytesMut::with_capacity(65536);

        loop {
            let read_result = timeout(Duration::from_secs(30), stream.read_buf(&mut read_buf)).await;
            match read_result {
                Err(_) => anyhow::bail!("Timeout waiting for login response"),
                Ok(Err(e)) => anyhow::bail!("Read error: {}", e),
                Ok(Ok(0)) => anyhow::bail!("Connection closed during login (server may be rate limiting)"),
                Ok(Ok(_)) => {}
            }

            if read_buf.len() >= 4 {
                let msg_len = u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;

                if read_buf.len() >= 4 + msg_len {
                    let mut msg_buf = read_buf.split_to(4 + msg_len);

                    match read_server_message(&mut msg_buf) {
                        Ok(ServerResponse::LoginSuccess { .. }) => {
                            println!("✓ Login successful!");
                            break;
                        }
                        Ok(ServerResponse::LoginFailure { reason, detail }) => {
                            anyhow::bail!("Login failed: {:?} - {:?}", reason, detail);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            println!("Failed to parse message: {e}");
                        }
                    }
                }
            }
        }

        buf.clear();
        let set_status = ServerRequest::SetStatus {
            status: slsk_rs::constants::UserStatus::Online,
        };
        set_status.write_message(&mut buf);
        stream.write_all(&buf).await?;

        Ok(Self {
            stream,
            read_buf,
            username: username.to_string(),
        })
    }

    async fn search(&mut self, query: &str) -> anyhow::Result<Vec<AccumulatedResult>> {
        let search_token = next_token();
        let mut buf = BytesMut::new();
        let search = ServerRequest::FileSearch {
            token: search_token,
            query: query.to_string(),
        };
        search.write_message(&mut buf);
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;

        let accumulated_results: Arc<Mutex<Vec<AccumulatedResult>>> = Arc::new(Mutex::new(Vec::new()));
        let mut peer_tasks = Vec::new();

        let start = std::time::Instant::now();

        while start.elapsed() < AGGREGATION_TIMEOUT {
            match timeout(Duration::from_millis(200), self.stream.read_buf(&mut self.read_buf)).await {
                Ok(Ok(0)) => {
                    // Connection closed - need to reconnect
                    return Err(anyhow::anyhow!("Server connection closed during search"));
                }
                Ok(Ok(_)) => {
                    while self.read_buf.len() >= 4 {
                        let msg_len = u32::from_le_bytes([
                            self.read_buf[0],
                            self.read_buf[1],
                            self.read_buf[2],
                            self.read_buf[3],
                        ]) as usize;

                        if self.read_buf.len() < 4 + msg_len {
                            break;
                        }

                        let mut msg_buf = self.read_buf.split_to(4 + msg_len);

                        if let Ok(response) = read_server_message(&mut msg_buf) {
                            if let ServerResponse::ConnectToPeer {
                                username,
                                connection_type: ConnectionType::Peer,
                                ip,
                                port,
                                token,
                                ..
                            } = response
                            {
                                let results = accumulated_results.clone();
                                let peer_user = username.clone();
                                let task = tokio::spawn(async move {
                                    let _ = connect_and_receive_search(&peer_user, ip, port, token, &results).await;
                                });
                                peer_tasks.push(task);
                            }
                        }
                    }
                }
                Ok(Err(e)) => {
                    return Err(anyhow::anyhow!("Read error during search: {}", e));
                }
                Err(_) => {} // Timeout, continue loop
            }
        }

        // Wait for peer tasks with timeout
        let _ = timeout(Duration::from_secs(3), async {
            for task in peer_tasks {
                let _ = task.await;
            }
        }).await;

        let results = accumulated_results.lock().await;
        Ok(results.clone())
    }

    async fn get_peer_address(&mut self, username: &str) -> anyhow::Result<(Ipv4Addr, u32)> {
        let mut buf = BytesMut::new();
        let req = ServerRequest::GetPeerAddress {
            username: username.to_string(),
        };
        req.write_message(&mut buf);
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(10) {
                anyhow::bail!("Timeout waiting for peer address");
            }

            match timeout(Duration::from_millis(100), self.stream.read_buf(&mut self.read_buf)).await {
                Ok(Ok(0)) => anyhow::bail!("Connection closed"),
                Ok(Ok(_)) => {
                    while self.read_buf.len() >= 4 {
                        let msg_len = u32::from_le_bytes([
                            self.read_buf[0],
                            self.read_buf[1],
                            self.read_buf[2],
                            self.read_buf[3],
                        ]) as usize;

                        if self.read_buf.len() < 4 + msg_len {
                            break;
                        }

                        let mut msg_buf = self.read_buf.split_to(4 + msg_len);

                        if let Ok(ServerResponse::GetPeerAddress { username: u, ip, port, .. }) =
                            read_server_message(&mut msg_buf)
                        {
                            if u == username {
                                if ip == Ipv4Addr::new(0, 0, 0, 0) {
                                    anyhow::bail!("User {} is offline", username);
                                }
                                return Ok((ip, port));
                            }
                        }
                    }
                }
                Ok(Err(e)) => anyhow::bail!("Read error: {}", e),
                Err(_) => {}
            }
        }
    }

    async fn download_file(&mut self, matched: &MatchedFile) -> anyhow::Result<PathBuf> {
        let (ip, port) = self.get_peer_address(&matched.username).await?;

        let addr = format!("{}:{}", ip, port);
        let mut peer_stream = match timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(&addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => anyhow::bail!("Connect failed: {}", e),
            Err(_) => anyhow::bail!("Connect timeout"),
        };
        peer_stream.set_nodelay(true)?;

        let peer_token = next_token();
        let init = PeerInitMessage::PeerInit {
            username: self.username.clone(),
            connection_type: ConnectionType::Peer,
            token: peer_token,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&init, &mut buf);
        peer_stream.write_all(&buf).await?;

        buf.clear();
        let queue_msg = PeerMessage::QueueUpload {
            filename: matched.filename.clone(),
        };
        queue_msg.write_message(&mut buf);
        peer_stream.write_all(&buf).await?;
        peer_stream.flush().await?;

        let mut read_buf = BytesMut::with_capacity(65536);
        let start = std::time::Instant::now();
        let mut transfer_token: Option<u32> = None;
        let mut file_size = matched.size;

        loop {
            if start.elapsed() > TRANSFER_WAIT_TIMEOUT {
                anyhow::bail!("Timeout waiting for transfer request");
            }

            match timeout(Duration::from_secs(1), peer_stream.read_buf(&mut read_buf)).await {
                Ok(Ok(0)) => {
                    if transfer_token.is_some() {
                        break;
                    }
                    anyhow::bail!("Peer closed connection (user may not allow uploads)");
                }
                Ok(Ok(_)) => {
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
                            Ok(PeerMessage::TransferRequest {
                                direction: TransferDirection::Upload,
                                token,
                                filename,
                                file_size: size,
                            }) => {
                                if filename == matched.filename {
                                    transfer_token = Some(token);
                                    if let Some(sz) = size {
                                        file_size = sz;
                                    }

                                    buf.clear();
                                    let response = PeerMessage::TransferResponse {
                                        token,
                                        allowed: true,
                                        reason: None,
                                        file_size: None,
                                    };
                                    response.write_message(&mut buf);
                                    peer_stream.write_all(&buf).await?;
                                    peer_stream.flush().await?;
                                }
                            }
                            Ok(PeerMessage::UploadDenied { reason, .. }) => {
                                anyhow::bail!("Upload denied: {:?}", reason);
                            }
                            Ok(PeerMessage::UploadFailed { .. }) => {
                                anyhow::bail!("Upload failed by peer");
                            }
                            Ok(PeerMessage::PlaceInQueueResponse { place, .. }) => {
                                println!("    Queued at position {}", place);
                            }
                            _ => {}
                        }
                    }

                    if transfer_token.is_some() {
                        break;
                    }
                }
                Ok(Err(e)) => anyhow::bail!("Read error: {}", e),
                Err(_) => {} // Timeout, continue waiting
            }
        }

        let token = transfer_token.ok_or_else(|| anyhow::anyhow!("No transfer token received"))?;

        drop(peer_stream);

        // Small delay before opening file connection
        tokio::time::sleep(Duration::from_millis(100)).await;

        let mut file_stream = match timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(&addr)).await {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => anyhow::bail!("File connect failed: {}", e),
            Err(_) => anyhow::bail!("File connect timeout"),
        };
        file_stream.set_nodelay(true)?;

        let file_init = PeerInitMessage::PeerInit {
            username: self.username.clone(),
            connection_type: ConnectionType::File,
            token: peer_token,
        };
        buf.clear();
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
        file_stream.flush().await?;

        let filename = matched
            .filename
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&matched.filename);
        let download_path = PathBuf::from("downloads").join(filename);

        tokio::fs::create_dir_all("downloads").await?;
        let mut file = File::create(&download_path).await?;

        let mut received = 0u64;
        let mut file_buf = vec![0u8; 65536];
        let mut last_print = std::time::Instant::now();

        loop {
            match timeout(Duration::from_secs(30), file_stream.read(&mut file_buf)).await {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    file.write_all(&file_buf[..n]).await?;
                    received += n as u64;
                    
                    if last_print.elapsed() > Duration::from_secs(2) {
                        let pct = (received as f64 / file_size as f64 * 100.0).min(100.0);
                        print!("\r    Progress: {:.1}% ({:.1}MB / {:.1}MB)    ", 
                            pct, received as f64 / 1_000_000.0, file_size as f64 / 1_000_000.0);
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                        last_print = std::time::Instant::now();
                    }
                }
                Ok(Err(e)) => anyhow::bail!("Read error during transfer: {}", e),
                Err(_) => anyhow::bail!("Transfer stalled (30s timeout)"),
            }
        }

        println!(); // Newline after progress

        if received >= file_size * 95 / 100 {
            Ok(download_path)
        } else if received > 0 {
            anyhow::bail!("Incomplete download: {} / {} bytes ({:.1}%)", 
                received, file_size, received as f64 / file_size as f64 * 100.0)
        } else {
            anyhow::bail!("No data received")
        }
    }
}

async fn connect_and_receive_search(
    peer_username: &str,
    ip: Ipv4Addr,
    port: u32,
    token: u32,
    accumulated: &Arc<Mutex<Vec<AccumulatedResult>>>,
) -> anyhow::Result<usize> {
    let addr = format!("{}:{}", ip, port);

    let mut stream = match timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(_)) => return Ok(0),
        Err(_) => return Ok(0),
    };

    let pierce = PeerInitMessage::PierceFirewall { token };
    let mut buf = BytesMut::new();
    write_peer_init_message(&pierce, &mut buf);
    if stream.write_all(&buf).await.is_err() {
        return Ok(0);
    }

    let mut read_buf = BytesMut::with_capacity(256 * 1024);
    let mut result_count = 0;

    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        match timeout(Duration::from_millis(500), stream.read_buf(&mut read_buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(_)) => {
                while read_buf.len() >= 4 {
                    let msg_len =
                        u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]])
                            as usize;

                    if read_buf.len() < 4 + msg_len {
                        break;
                    }

                    let mut msg_buf = read_buf.split_to(4 + msg_len);

                    if let Ok(PeerMessage::FileSearchResponse { results, .. }) = read_peer_message(&mut msg_buf) {
                        result_count += results.len();
                        let mut acc = accumulated.lock().await;
                        for file in results {
                            acc.push(AccumulatedResult {
                                username: peer_username.to_string(),
                                file,
                            });
                        }
                    }
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {}
        }
    }

    Ok(result_count)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: slsk-debug <spotify-playlist-url-or-search-query>");
        std::process::exit(1);
    }

    let url = &args[1];
    let username = std::env::var("SOULSEEK_ACCOUNT").expect("SOULSEEK_ACCOUNT not set");
    let password = std::env::var("SOULSEEK_PASSWORD").expect("SOULSEEK_PASSWORD not set");

    let tracks: Vec<SpotifyTrack> = if let Some((resource_type, id)) = parse_spotify_url(url) {
        let token = get_spotify_token().await?;
        match resource_type {
            SpotifyResourceType::Track => {
                let track = fetch_spotify_track(&token, &id).await?;
                println!("Track: {}", track.display_name());
                vec![track]
            }
            SpotifyResourceType::Playlist => {
                let (name, tracks) = fetch_spotify_playlist(&token, &id).await?;
                println!("Playlist: {} ({} tracks)", name, tracks.len());
                for (i, t) in tracks.iter().enumerate() {
                    println!("  {}. {}", i + 1, t.display_name());
                }
                tracks
            }
        }
    } else {
        vec![SpotifyTrack {
            name: url.clone(),
            artist: String::new(),
        }]
    };

    let mut client = SoulseekClient::connect(&username, &password).await?;

    let mut downloads: Vec<TrackDownload> = tracks
        .into_iter()
        .map(|track| TrackDownload {
            track,
            status: DownloadStatus::Pending,
            retry_count: 0,
            tried_users: Vec::new(),
        })
        .collect();

    let total = downloads.len();
    let mut completed = 0;
    let mut failed = 0;

    loop {
        let pending_idx = downloads.iter().position(|d| {
            matches!(d.status, DownloadStatus::Pending) && d.retry_count <= MAX_RETRIES
        });

        let Some(idx) = pending_idx else {
            break;
        };

        let track = &downloads[idx].track;
        let query = track.to_search_query();
        let retry = downloads[idx].retry_count;
        let tried_users = downloads[idx].tried_users.clone();

        println!(
            "\n[{}/{}] Searching: {} {}",
            idx + 1,
            total,
            track.display_name(),
            if retry > 0 { format!("(retry {})", retry) } else { String::new() }
        );

        downloads[idx].status = DownloadStatus::Searching;

        let results = match client.search(&query).await {
            Ok(r) => r,
            Err(e) => {
                let err_str = e.to_string();
                println!("  ✗ Search failed: {}", err_str);
                
                // Reconnect on any error with delay
                println!("  Waiting {}s before reconnecting...", RECONNECT_DELAY.as_secs());
                tokio::time::sleep(RECONNECT_DELAY).await;
                
                match SoulseekClient::connect(&username, &password).await {
                    Ok(new_client) => {
                        client = new_client;
                        downloads[idx].status = DownloadStatus::Pending;
                        continue;
                    }
                    Err(e) => {
                        println!("  ✗ Reconnect failed: {}", e);
                        println!("  Waiting {}s before retry...", RECONNECT_DELAY.as_secs());
                        tokio::time::sleep(RECONNECT_DELAY).await;
                        downloads[idx].retry_count += 1;
                        if downloads[idx].retry_count > MAX_RETRIES {
                            downloads[idx].status = DownloadStatus::Failed(e.to_string());
                            failed += 1;
                        } else {
                            downloads[idx].status = DownloadStatus::Pending;
                        }
                        continue;
                    }
                }
            }
        };
        println!("  Found {} results", results.len());

        let candidates = pick_best_files(&results, &tried_users);
        if !candidates.is_empty() {
            let mut downloaded = false;
            
            for (candidate_idx, best) in candidates.iter().enumerate() {
                let matched = MatchedFile {
                    username: best.username.clone(),
                    filename: best.file.filename.clone(),
                    size: best.file.size,
                };

                let is_flac = matched.filename.to_lowercase().ends_with(".flac");
                let bitrate = get_bitrate(&best.file.attributes);

                println!(
                    "  Trying [{}/{}]: [{}] {} ({} {})",
                    candidate_idx + 1,
                    candidates.len(),
                    matched.username,
                    matched.filename.rsplit(['/', '\\']).next().unwrap_or(&matched.filename),
                    if is_flac { "FLAC".to_string() } else { format!("{}kbps", bitrate.unwrap_or(0)) },
                    format!("{:.1}MB", matched.size as f64 / 1_000_000.0)
                );

                downloads[idx].tried_users.push(matched.username.clone());
                downloads[idx].status = DownloadStatus::Downloading;

                match client.download_file(&matched).await {
                    Ok(path) => {
                        println!("  ✓ Saved to {:?}", path);
                        downloads[idx].status = DownloadStatus::Completed;
                        completed += 1;
                        downloaded = true;
                        break;
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        println!("    ✗ Failed: {}", err_str);
                        
                        // Reconnect if connection issues
                        if err_str.contains("Broken pipe") || err_str.contains("reset") || err_str.contains("closed") {
                            println!("    Waiting {}s before reconnecting...", RECONNECT_DELAY.as_secs());
                            tokio::time::sleep(RECONNECT_DELAY).await;
                            if let Ok(new_client) = SoulseekClient::connect(&username, &password).await {
                                client = new_client;
                            }
                        }
                    }
                }
            }
            
            if !downloaded {
                downloads[idx].retry_count += 1;
                if downloads[idx].retry_count > MAX_RETRIES {
                    downloads[idx].status = DownloadStatus::Failed("All sources failed".to_string());
                    failed += 1;
                } else {
                    downloads[idx].status = DownloadStatus::Pending;
                }
            }
        } else {
            println!("  ✗ No audio files found");
            downloads[idx].retry_count += 1;
            if downloads[idx].retry_count > MAX_RETRIES {
                downloads[idx].status = DownloadStatus::Failed("No matches found".to_string());
                failed += 1;
            } else {
                downloads[idx].status = DownloadStatus::Pending;
            }
        }

        // Small delay between tracks
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    println!("\n========================================");
    println!("DOWNLOAD COMPLETE");
    println!("========================================");
    println!("Total: {} | Completed: {} | Failed: {}", total, completed, failed);

    if failed > 0 {
        println!("\nFailed tracks:");
        for d in &downloads {
            if let DownloadStatus::Failed(reason) = &d.status {
                println!("  - {} ({})", d.track.display_name(), reason);
            }
        }
    }

    Ok(())
}
