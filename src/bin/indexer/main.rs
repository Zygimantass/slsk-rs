//! Soulseek peer file indexer
//!
//! Connects to the Soulseek network, discovers users via rooms,
//! fetches their shared file lists, and stores them in SQLite for local searching.

use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use slsk_rs::constants::{ConnectionType, DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT, UserStatus};
use slsk_rs::db::Database;
use slsk_rs::peer::{PeerMessage, SharedDirectory, read_peer_message};
use slsk_rs::peer_init::{PeerInitMessage, write_peer_init_message};
use slsk_rs::protocol::MessageWrite;
use slsk_rs::server::{ServerRequest, ServerResponse, read_server_message};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use tokio::sync::{Mutex, Semaphore};
use tokio::time::timeout;

const MAX_CONCURRENT_PEERS: usize = 10;

const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const PEER_READ_TIMEOUT: Duration = Duration::from_secs(30);
const DELAY_BETWEEN_PEERS: Duration = Duration::from_millis(500);

struct IndexerClient {
    stream: TcpStream,
    read_buf: BytesMut,
    username: String,
}

impl IndexerClient {
    async fn connect(username: &str, password: &str) -> anyhow::Result<Self> {
        let max_attempts = 5;
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
                    let delay = if err_str.contains("rate limit") || err_str.contains("reset") {
                        Duration::from_secs(30 * attempt as u64)
                    } else {
                        Duration::from_secs((10 * 2u64.pow((attempt - 1).min(2))).min(60))
                    };
                    println!("  Connect attempt {}/{} failed: {}", attempt, max_attempts, e);
                    println!("  Retrying in {}s...", delay.as_secs());
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn connect_once(username: &str, password: &str) -> anyhow::Result<Self> {
        let server_host =
            std::env::var("SOULSEEK_SERVER").unwrap_or_else(|_| DEFAULT_SERVER_HOST.to_string());
        let server_port: u16 = std::env::var("SOULSEEK_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(DEFAULT_SERVER_PORT);

        println!("Connecting to {}:{}...", server_host, server_port);
        let mut stream = TcpStream::connect((&*server_host, server_port)).await?;
        stream.set_nodelay(true)?;

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
                Ok(Ok(0)) => anyhow::bail!("Connection closed during login"),
                Ok(Ok(_)) => {}
            }

            if read_buf.len() >= 4 {
                let msg_len =
                    u32::from_le_bytes([read_buf[0], read_buf[1], read_buf[2], read_buf[3]])
                        as usize;

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
            status: UserStatus::Online,
        };
        set_status.write_message(&mut buf);
        stream.write_all(&buf).await?;

        Ok(Self {
            stream,
            read_buf,
            username: username.to_string(),
        })
    }

    async fn join_room(&mut self, room: &str) -> anyhow::Result<Vec<String>> {
        let mut buf = BytesMut::new();
        let req = ServerRequest::JoinRoom {
            room: room.to_string(),
            private: false,
        };
        req.write_message(&mut buf);
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(10) {
                anyhow::bail!("Timeout waiting for room join");
            }

            match timeout(Duration::from_millis(100), self.stream.read_buf(&mut self.read_buf))
                .await
            {
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

                        if let Ok(ServerResponse::JoinRoom { room: r, users, .. }) =
                            read_server_message(&mut msg_buf)
                        {
                            if r == room {
                                return Ok(users.into_iter().map(|u| u.username).collect());
                            }
                        }
                    }
                }
                Ok(Err(e)) => anyhow::bail!("Read error: {}", e),
                Err(_) => {}
            }
        }
    }

    async fn get_room_list(&mut self) -> anyhow::Result<Vec<(String, u32)>> {
        let mut buf = BytesMut::new();
        let req = ServerRequest::RoomList;
        req.write_message(&mut buf);
        self.stream.write_all(&buf).await?;
        self.stream.flush().await?;

        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(10) {
                anyhow::bail!("Timeout waiting for room list");
            }

            match timeout(Duration::from_millis(100), self.stream.read_buf(&mut self.read_buf))
                .await
            {
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

                        if let Ok(ServerResponse::RoomList { rooms, .. }) =
                            read_server_message(&mut msg_buf)
                        {
                            return Ok(rooms);
                        }
                    }
                }
                Ok(Err(e)) => anyhow::bail!("Read error: {}", e),
                Err(_) => {}
            }
        }
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

            match timeout(Duration::from_millis(100), self.stream.read_buf(&mut self.read_buf))
                .await
            {
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

                        if let Ok(ServerResponse::GetPeerAddress {
                            username: u,
                            ip,
                            port,
                            ..
                        }) = read_server_message(&mut msg_buf)
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
}

async fn fetch_shared_files(
    our_username: &str,
    peer_username: &str,
    ip: Ipv4Addr,
    port: u32,
) -> anyhow::Result<Vec<SharedDirectory>> {
    let addr = format!("{}:{}", ip, port);
    let mut stream = match timeout(PEER_CONNECT_TIMEOUT, TcpStream::connect(&addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => anyhow::bail!("Connect failed: {}", e),
        Err(_) => anyhow::bail!("Connect timeout"),
    };
    stream.set_nodelay(true)?;

    // Send PeerInit
    let init = PeerInitMessage::PeerInit {
        username: our_username.to_string(),
        connection_type: ConnectionType::Peer,
        token: 0,
    };
    let mut buf = BytesMut::new();
    write_peer_init_message(&init, &mut buf);
    stream.write_all(&buf).await?;
    stream.flush().await?;

    // Send SharedFileListRequest
    buf.clear();
    PeerMessage::SharedFileListRequest.write_message(&mut buf);
    stream.write_all(&buf).await?;
    stream.flush().await?;

    // Read response
    let mut read_buf = BytesMut::with_capacity(1024 * 1024); // 1MB buffer for large file lists

    let start = std::time::Instant::now();
    while start.elapsed() < PEER_READ_TIMEOUT {
        match timeout(Duration::from_secs(5), stream.read_buf(&mut read_buf)).await {
            Ok(Ok(0)) => break,
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
                        Ok(PeerMessage::SharedFileListResponse {
                            directories,
                            private_directories,
                        }) => {
                            let mut all_dirs = directories;
                            all_dirs.extend(private_directories);
                            return Ok(all_dirs);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            // Some parse errors are okay, continue
                            if read_buf.is_empty() {
                                anyhow::bail!("Parse error and no more data: {}", e);
                            }
                        }
                    }
                }
            }
            Ok(Err(e)) => anyhow::bail!("Read error: {}", e),
            Err(_) => {}
        }
    }

    anyhow::bail!("No file list received from {}", peer_username)
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  slsk-indexer index [--rooms <room1,room2,...>]  - Index users from rooms");
    eprintln!("  slsk-indexer search <query>                     - Search local index");
    eprintln!("  slsk-indexer stats                              - Show index statistics");
    eprintln!();
    eprintln!("Environment variables:");
    eprintln!("  SOULSEEK_ACCOUNT   - Soulseek username");
    eprintln!("  SOULSEEK_PASSWORD  - Soulseek password");
    eprintln!("  SOULSEEK_SERVER    - Server host (default: server.slsknet.org)");
    eprintln!("  SOULSEEK_PORT      - Server port (default: 2416)");
    eprintln!("  SLSK_INDEX_DB      - Database path (default: slsk_index.db)");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let db_path = std::env::var("SLSK_INDEX_DB").unwrap_or_else(|_| "slsk_index.db".to_string());
    let mut db = Database::open(&db_path)?;

    match args[1].as_str() {
        "index" => {
            let username = std::env::var("SOULSEEK_ACCOUNT").expect("SOULSEEK_ACCOUNT not set");
            let password = std::env::var("SOULSEEK_PASSWORD").expect("SOULSEEK_PASSWORD not set");

            let rooms: Option<Vec<String>> = if args.len() > 3 && args[2] == "--rooms" {
                Some(args[3].split(',').map(|s| s.trim().to_string()).collect())
            } else {
                None // Will join all rooms
            };

            run_indexer(&username, &password, rooms.as_deref(), &mut db).await?;
        }
        "search" => {
            if args.len() < 3 {
                eprintln!("Usage: slsk-indexer search <query>");
                std::process::exit(1);
            }
            let query = args[2..].join(" ");
            run_search(&query, &db)?;
        }
        "stats" => {
            show_stats(&db)?;
        }
        _ => {
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn run_indexer(
    username: &str,
    password: &str,
    rooms: Option<&[String]>,
    db: &mut Database,
) -> anyhow::Result<()> {
    let mut client = IndexerClient::connect(username, password).await?;

    // Collect users from rooms
    let mut all_users: HashSet<String> = HashSet::new();

    // First get room list to find popular rooms
    println!("\nFetching room list...");
    let room_list = client.get_room_list().await?;
    println!("Found {} rooms", room_list.len());

    // Show top 10 rooms by user count
    let mut sorted_rooms = room_list.clone();
    sorted_rooms.sort_by(|a, b| b.1.cmp(&a.1));
    println!("\nTop 10 rooms:");
    for (name, count) in sorted_rooms.iter().take(10) {
        println!("  {} ({} users)", name, count);
    }

    // Determine which rooms to join
    let rooms_to_join: Vec<String> = match rooms {
        Some(r) => r.to_vec(),
        None => {
            // Join all rooms with at least 50 users
            println!("\nJoining all rooms with 50+ users...");
            sorted_rooms
                .iter()
                .filter(|(_, count)| *count >= 50)
                .map(|(name, _)| name.clone())
                .collect()
        }
    };

    println!("Will join {} rooms", rooms_to_join.len());

    // Join rooms
    for room in &rooms_to_join {
        println!("\nJoining room: {}", room);
        match client.join_room(room).await {
            Ok(users) => {
                println!("  Found {} users", users.len());
                for user in users {
                    if user != username {
                        all_users.insert(user);
                    }
                }
            }
            Err(e) => {
                println!("  Failed to join: {}", e);
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    println!("\nTotal unique users to index: {}", all_users.len());

    // Get already indexed users
    let indexed_users = db.get_indexed_users()?;
    let indexed_set: HashSet<_> = indexed_users.into_iter().collect();

    let users_to_index: Vec<_> = all_users
        .difference(&indexed_set)
        .cloned()
        .collect();

    println!("New users to index: {}", users_to_index.len());
    println!("Already indexed: {}", indexed_set.len());
    println!("Concurrent connections: {}", MAX_CONCURRENT_PEERS);

    // First, get all peer addresses (must be done sequentially through server connection)
    println!("\nResolving peer addresses...");
    let mut peer_addresses: Vec<(String, Ipv4Addr, u32)> = Vec::new();
    for (i, peer_user) in users_to_index.iter().enumerate() {
        if i % 50 == 0 {
            println!("  Resolved {}/{} addresses...", i, users_to_index.len());
        }
        match client.get_peer_address(peer_user).await {
            Ok((ip, port)) => {
                peer_addresses.push((peer_user.clone(), ip, port));
            }
            Err(_) => {
                // Skip offline users silently
            }
        }
    }
    println!("  Resolved {} peer addresses", peer_addresses.len());

    // Now fetch file lists in parallel
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_PEERS));
    let progress = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let total = peer_addresses.len() as u32;
    let results: Arc<Mutex<Vec<(String, Vec<SharedDirectory>)>>> = Arc::new(Mutex::new(Vec::new()));
    let our_username = username.to_string();

    let mut handles = Vec::new();

    for (peer_user, ip, port) in peer_addresses {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let prog = progress.clone();
        let results = results.clone();
        let our_user = our_username.clone();

        let handle = tokio::spawn(async move {
            let current = prog.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            
            match fetch_shared_files(&our_user, &peer_user, ip, port).await {
                Ok(directories) => {
                    let file_count: usize = directories.iter().map(|d| d.files.len()).sum();
                    println!(
                        "[{}/{}] ✓ {} - {} files",
                        current, total, peer_user, file_count
                    );
                    
                    let mut res = results.lock().await;
                    res.push((peer_user, directories));
                }
                Err(e) => {
                    println!("[{}/{}] ✗ {} - {}", current, total, peer_user, e);
                }
            }

            drop(permit);
        });

        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        let _ = handle.await;
    }

    // Write results to database in a single transaction
    println!("\nWriting to database...");
    let results = Arc::try_unwrap(results).unwrap().into_inner();
    let (success_count, fail_count) = db.index_users_batch(results)?;

    println!("\n========================================");
    println!("INDEXING COMPLETE");
    println!("========================================");
    println!("Success: {} | Failed: {}", success_count, fail_count);

    Ok(())
}

fn run_search(query: &str, db: &Database) -> anyhow::Result<()> {
    println!("Searching for: {}\n", query);

    let results = db.search(query, 50)?;

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    println!("Found {} results:\n", results.len());

    for (i, result) in results.iter().enumerate() {
        let size_mb = result.size as f64 / 1_000_000.0;
        println!(
            "{}. [{}] {} ({:.1} MB)",
            i + 1,
            result.username,
            result.filename,
            size_mb
        );
    }

    Ok(())
}

fn show_stats(db: &Database) -> anyhow::Result<()> {
    let stats = db.get_stats()?;
    println!("Index Statistics:");
    println!("  Users indexed: {}", stats.user_count);
    println!("  Total files: {}", stats.file_count);
    println!("  Database size: {:.1} MB", stats.db_size_bytes as f64 / 1_000_000.0);
    Ok(())
}
