use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::BytesMut;
use slsk_rs::constants::{ConnectionType, DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT};
use slsk_rs::peer::{PeerMessage, SearchResultFile, read_peer_message};
use slsk_rs::peer_init::{PeerInitMessage, write_peer_init_message};
use slsk_rs::protocol::MessageWrite;
use slsk_rs::server::{ServerRequest, ServerResponse, read_server_message};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;

static TOKEN_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_token() -> u32 {
    TOKEN_COUNTER.fetch_add(1, Ordering::SeqCst)
}

const AGGREGATION_TIMEOUT: Duration = Duration::from_secs(10);

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

fn parse_spotify_url(url: &str) -> Option<String> {
    let url = url.trim();

    if let Some(rest) = url.strip_prefix("spotify:track:") {
        return Some(rest.to_string());
    }

    if url.contains("open.spotify.com/track/") {
        let path = url.split("open.spotify.com/track/").nth(1)?;
        let id = path.split('?').next()?;
        return Some(id.to_string());
    }

    None
}

async fn fetch_spotify_track(track_id: &str) -> anyhow::Result<SpotifyTrack> {
    let client_id = std::env::var("SPOTIFY_CLIENT_ID")?;
    let client_secret = std::env::var("SPOTIFY_CLIENT_SECRET")?;

    let client = reqwest::Client::new();

    // Get access token
    let credentials = format!("{}:{}", client_id, client_secret);
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        credentials.as_bytes(),
    );

    #[derive(serde::Deserialize)]
    struct TokenResponse {
        access_token: String,
    }

    let token_resp: TokenResponse = client
        .post("https://accounts.spotify.com/api/token")
        .header("Authorization", format!("Basic {}", encoded))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Get track info
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
        .header(
            "Authorization",
            format!("Bearer {}", token_resp.access_token),
        )
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
        let a_bitrate = get_bitrate(&a.file.attributes).unwrap_or(0);
        let b_bitrate = get_bitrate(&b.file.attributes).unwrap_or(0);

        let a_is_flac = a.file.filename.to_lowercase().ends_with(".flac");
        let b_is_flac = b.file.filename.to_lowercase().ends_with(".flac");

        if a_is_flac != b_is_flac {
            return b_is_flac.cmp(&a_is_flac);
        }

        b_bitrate.cmp(&a_bitrate)
    });

    candidates.first().copied()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();

    let (search_query, track_info) = if args.len() > 1 {
        let url = &args[1];
        if let Some(track_id) = parse_spotify_url(url) {
            println!("Fetching Spotify track {}...", track_id);
            match fetch_spotify_track(&track_id).await {
                Ok(track) => {
                    let query = format!("{} {}", track.artist, track.name);
                    println!("Track: {} - {}", track.artist, track.name);
                    println!("Search query: {}", query);
                    (query, Some(track))
                }
                Err(e) => {
                    eprintln!("Failed to fetch Spotify track: {}", e);
                    eprintln!("Using URL as search query");
                    (url.clone(), None)
                }
            }
        } else {
            (url.clone(), None)
        }
    } else {
        ("sansibar".to_string(), None)
    };

    let username = std::env::var("SOULSEEK_ACCOUNT").expect("SOULSEEK_ACCOUNT not set");
    let password = std::env::var("SOULSEEK_PASSWORD").expect("SOULSEEK_PASSWORD not set");

    println!(
        "\nConnecting to {}:{}...",
        DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT
    );
    let mut stream = TcpStream::connect((DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT)).await?;
    stream.set_nodelay(true)?;
    println!("Connected!");

    // Login
    let login = ServerRequest::Login {
        username: username.clone(),
        password,
        version: 160,
        minor_version: 3,
    };

    let mut buf = BytesMut::new();
    login.write_message(&mut buf);
    stream.write_all(&buf).await?;
    stream.flush().await?;
    println!("Sent login request");

    // Read login response
    let mut read_buf = BytesMut::with_capacity(65536);

    // Wait for login response
    loop {
        let read_result = timeout(Duration::from_secs(30), stream.read_buf(&mut read_buf)).await;
        match read_result {
            Err(_) => {
                anyhow::bail!("Timeout waiting for login response");
            }
            Ok(Err(e)) => {
                anyhow::bail!("Read error: {}", e);
            }
            Ok(Ok(0)) => {
                anyhow::bail!("Connection closed");
            }
            Ok(Ok(_)) => {}
        }

        if read_buf.len() >= 4 {
            let msg_len =
                u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]]) as usize;

            if read_buf.len() >= 4 + msg_len {
                let mut msg_buf = read_buf.split_to(4 + msg_len);

                match read_server_message(&mut msg_buf) {
                    Ok(ServerResponse::LoginSuccess { greet, own_ip, .. }) => {
                        println!("✓ Login successful!");
                        println!("  Greeting: {}", greet);
                        println!("  Our IP: {}", own_ip);
                        break;
                    }
                    Ok(ServerResponse::LoginFailure { reason, detail }) => {
                        anyhow::bail!("Login failed: {:?} - {:?}", reason, detail);
                    }
                    Ok(other) => {
                        println!("Received unexpected message before login: {:?}", other);
                    }
                    Err(e) => {
                        println!("Failed to parse message: {e}");
                    }
                }
            }
        }
    }

    // Set status online
    buf.clear();
    let set_status = ServerRequest::SetStatus {
        status: slsk_rs::constants::UserStatus::Online,
    };
    set_status.write_message(&mut buf);
    stream.write_all(&buf).await?;
    println!("Set status to Online");

    // Send search
    let search_token = next_token();
    buf.clear();
    let search = ServerRequest::FileSearch {
        token: search_token,
        query: search_query.clone(),
    };
    search.write_message(&mut buf);
    stream.write_all(&buf).await?;
    println!(
        "\nSent search for '{}' with token {}",
        search_query, search_token
    );

    // Track peers we need to connect to
    let mut pending_peers: HashMap<u32, (String, Ipv4Addr, u32)> = HashMap::new();

    // Shared accumulated results
    let accumulated_results: Arc<Mutex<Vec<AccumulatedResult>>> = Arc::new(Mutex::new(Vec::new()));

    println!(
        "Listening for responses ({}s aggregation window)...\n",
        AGGREGATION_TIMEOUT.as_secs()
    );

    let start = std::time::Instant::now();
    let duration = Duration::from_secs(30);

    loop {
        if start.elapsed() > duration {
            println!("\n--- Timeout reached ---");
            break;
        }

        match timeout(Duration::from_millis(100), stream.read_buf(&mut read_buf)).await {
            Ok(Ok(0)) => {
                println!("Connection closed by server");
                break;
            }
            Ok(Ok(_n)) => {
                // Process messages
                while read_buf.len() >= 4 {
                    let msg_len =
                        u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]])
                            as usize;

                    if read_buf.len() < 4 + msg_len {
                        break;
                    }

                    let mut msg_buf = read_buf.split_to(4 + msg_len);

                    match read_server_message(&mut msg_buf) {
                        Ok(response) => {
                            handle_response(response, &mut pending_peers);
                        }
                        Err(e) => {
                            println!("✗ Parse error: {e}");
                        }
                    }
                }
            }
            Ok(Err(e)) => {
                println!("Read error: {e}");
                break;
            }
            Err(_) => {
                // Timeout, try to connect to pending peers
                for (token, (peer_username, ip, port)) in pending_peers.drain() {
                    println!(
                        "  Attempting peer connection to {} at {}:{}",
                        peer_username, ip, port
                    );

                    let results = accumulated_results.clone();
                    tokio::spawn(async move {
                        match connect_and_receive_search(&peer_username, ip, port, token, &results)
                            .await
                        {
                            Ok(count) => {
                                if count > 0 {
                                    println!("  ✓ Got {} results from {}", count, peer_username);
                                }
                            }
                            Err(e) => {
                                println!("  ✗ Peer {} error: {}", peer_username, e);
                            }
                        }
                    });
                }
            }
        }
    }

    // Wait a moment for any final results to come in
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Now pick the best file
    let results = accumulated_results.lock().await;
    let total_results = results.len();

    println!("\n========================================");
    println!("RESULTS SUMMARY");
    println!("========================================");
    println!("Total files collected: {}", total_results);

    if let Some(track) = track_info {
        println!("Searching for: {} - {}", track.artist, track.name);
    }

    if let Some(best) = pick_best_file(&results) {
        let bitrate = get_bitrate(&best.file.attributes);
        let is_flac = best.file.filename.to_lowercase().ends_with(".flac");

        println!("\n✓ BEST MATCH:");
        println!("  User: {}", best.username);
        println!("  File: {}", best.file.filename);
        println!(
            "  Size: {} bytes ({:.1} MB)",
            best.file.size,
            best.file.size as f64 / 1_000_000.0
        );
        println!(
            "  Format: {}",
            if is_flac { "FLAC (lossless)" } else { "Lossy" }
        );
        if let Some(br) = bitrate {
            println!("  Bitrate: {} kbps", br);
        }
    } else {
        println!("\n✗ No suitable audio files found");
    }

    // Show top 5 candidates
    let mut sorted: Vec<_> = results.iter().collect();
    sorted.sort_by(|a, b| {
        let a_flac = a.file.filename.to_lowercase().ends_with(".flac");
        let b_flac = b.file.filename.to_lowercase().ends_with(".flac");
        if a_flac != b_flac {
            return b_flac.cmp(&a_flac);
        }
        let a_br = get_bitrate(&a.file.attributes).unwrap_or(0);
        let b_br = get_bitrate(&b.file.attributes).unwrap_or(0);
        b_br.cmp(&a_br)
    });

    let audio_exts = [
        ".mp3", ".flac", ".m4a", ".ogg", ".opus", ".wav", ".aac", ".aiff",
    ];
    let audio_files: Vec<_> = sorted
        .iter()
        .filter(|r| {
            let lower = r.file.filename.to_lowercase();
            audio_exts.iter().any(|ext| lower.ends_with(ext))
        })
        .take(10)
        .collect();

    if !audio_files.is_empty() {
        println!("\nTop {} audio files:", audio_files.len());
        for (i, r) in audio_files.iter().enumerate() {
            let bitrate = get_bitrate(&r.file.attributes);
            let br_str = bitrate.map(|b| format!(" {}kbps", b)).unwrap_or_default();
            println!(
                "  {}. [{}]{} {}",
                i + 1,
                r.username,
                br_str,
                r.file
                    .filename
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&r.file.filename)
            );
        }
    }

    Ok(())
}

fn handle_response(
    response: ServerResponse,
    pending_peers: &mut HashMap<u32, (String, Ipv4Addr, u32)>,
) {
    match response {
        ServerResponse::ConnectToPeer {
            username,
            connection_type,
            ip,
            port,
            token,
            ..
        } => {
            println!(
                "→ ConnectToPeer: {} ({:?}) at {}:{}, token={}",
                username, connection_type, ip, port, token
            );

            if connection_type == ConnectionType::Peer {
                pending_peers.insert(token, (username, ip, port));
            }
        }
        ServerResponse::GetPeerAddress {
            username, ip, port, ..
        } => {
            println!("→ GetPeerAddress: {} at {}:{}", username, ip, port);
        }
        _ => {}
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

    let mut stream = match timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(anyhow::anyhow!("Connect failed: {}", e)),
        Err(_) => return Err(anyhow::anyhow!("Connect timeout")),
    };

    // Send PierceFirewall (we're responding to ConnectToPeer)
    let pierce = PeerInitMessage::PierceFirewall { token };
    let mut buf = BytesMut::new();
    write_peer_init_message(&pierce, &mut buf);
    stream.write_all(&buf).await?;

    let mut read_buf = BytesMut::with_capacity(1024 * 1024);
    let mut result_count = 0;

    // Read for a few seconds
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
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

                    match read_peer_message(&mut msg_buf) {
                        Ok(PeerMessage::FileSearchResponse { results, .. }) => {
                            result_count += results.len();

                            // Accumulate results
                            let mut acc = accumulated.lock().await;
                            for file in results {
                                acc.push(AccumulatedResult {
                                    username: peer_username.to_string(),
                                    file,
                                });
                            }
                        }
                        Ok(_) => {}
                        Err(_) => {}
                    }
                }
            }
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("Read error: {}", e));
            }
            Err(_) => {
                // Timeout, continue
            }
        }
    }

    Ok(result_count)
}
