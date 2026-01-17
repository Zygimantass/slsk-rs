use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::BytesMut;
use slsk_rs::constants::{ConnectionType, DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT};
use slsk_rs::peer::{PeerMessage, read_peer_message};
use slsk_rs::peer_init::{PeerInitMessage, write_peer_init_message};
use slsk_rs::protocol::MessageWrite;
use slsk_rs::server::{ServerRequest, ServerResponse, read_server_message};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

static TOKEN_COUNTER: AtomicU32 = AtomicU32::new(1);

fn next_token() -> u32 {
    TOKEN_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let username = std::env::var("SOULSEEK_ACCOUNT").expect("SOULSEEK_ACCOUNT not set");
    let password = std::env::var("SOULSEEK_PASSWORD").expect("SOULSEEK_PASSWORD not set");

    println!("Connecting to {}:{}...", DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT);
    let mut stream = TcpStream::connect((DEFAULT_SERVER_HOST, DEFAULT_SERVER_PORT)).await?;
    println!("Connected!");

    // Login
    let login = ServerRequest::Login {
        username: username.clone(),
        password,
        version: 160,
        minor_version: 1,
    };

    let mut buf = BytesMut::new();
    login.write_message(&mut buf);
    stream.write_all(&buf).await?;
    println!("Sent login request");

    // Read login response
    let mut read_buf = BytesMut::with_capacity(65536);
    
    // Wait for login response
    loop {
        let n = stream.read_buf(&mut read_buf).await?;
        if n == 0 {
            anyhow::bail!("Connection closed");
        }
        
        if read_buf.len() >= 4 {
            let msg_len = u32::from_le_bytes([
                read_buf[0], read_buf[1], read_buf[2], read_buf[3]
            ]) as usize;
            
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
    let search_query = "sansibar";
    let search_token = next_token();
    buf.clear();
    let search = ServerRequest::FileSearch {
        token: search_token,
        query: search_query.to_string(),
    };
    search.write_message(&mut buf);
    stream.write_all(&buf).await?;
    println!("Sent search for '{}' with token {}", search_query, search_token);

    // Track peers we need to connect to
    let mut pending_peers: HashMap<u32, (String, Ipv4Addr, u32)> = HashMap::new();
    let mut search_results = 0;

    println!("\nListening for responses (30 seconds)...\n");

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
                    let msg_len = u32::from_le_bytes([
                        read_buf[0], read_buf[1], read_buf[2], read_buf[3]
                    ]) as usize;
                    
                    if read_buf.len() < 4 + msg_len {
                        break;
                    }
                    
                    let mut msg_buf = read_buf.split_to(4 + msg_len);
                    
                    match read_server_message(&mut msg_buf) {
                        Ok(response) => {
                            handle_response(
                                response,
                                &username,
                                search_token,
                                &mut pending_peers,
                                &mut search_results,
                            ).await;
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
                    println!("  Attempting peer connection to {} at {}:{}", peer_username, ip, port);
                    
                    let my_username = username.clone();
                    tokio::spawn(async move {
                        match connect_and_receive_search(&my_username, &peer_username, ip, port, token).await {
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

    println!("\nTotal search results received: {}", search_results);
    Ok(())
}

async fn handle_response(
    response: ServerResponse,
    _my_username: &str,
    _search_token: u32,
    pending_peers: &mut HashMap<u32, (String, Ipv4Addr, u32)>,
    _search_results: &mut usize,
) {
    match response {
        ServerResponse::ConnectToPeer { username, connection_type, ip, port, token, .. } => {
            println!("→ ConnectToPeer: {} ({:?}) at {}:{}, token={}", 
                username, connection_type, ip, port, token);
            
            if connection_type == ConnectionType::Peer {
                pending_peers.insert(token, (username, ip, port));
            }
        }
        ServerResponse::GetPeerAddress { username, ip, port, .. } => {
            println!("→ GetPeerAddress: {} at {}:{}", username, ip, port);
        }
        ServerResponse::FileSearch { username, token, query } => {
            println!("→ FileSearch request from {}: '{}' (token={})", username, query, token);
        }
        ServerResponse::GetUserStatus { username, status, .. } => {
            println!("→ UserStatus: {} is {:?}", username, status);
        }
        ServerResponse::WatchUser { username, exists, status, .. } => {
            println!("→ WatchUser: {} exists={} status={:?}", username, exists, status);
        }
        other => {
            println!("→ Other: {:?}", other);
        }
    }
}

async fn connect_and_receive_search(
    _my_username: &str,
    _peer_username: &str,
    ip: Ipv4Addr,
    port: u32,
    token: u32,
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
                    let msg_len = u32::from_le_bytes([
                        read_buf[0], read_buf[1], read_buf[2], read_buf[3]
                    ]) as usize;
                    
                    if read_buf.len() < 4 + msg_len {
                        break;
                    }
                    
                    let mut msg_buf = read_buf.split_to(4 + msg_len);
                    
                    match read_peer_message(&mut msg_buf) {
                        Ok(PeerMessage::FileSearchResponse { results, .. }) => {
                            result_count += results.len();
                            for file in results.iter().take(3) {
                                println!("    📄 {}", file.filename);
                            }
                            if results.len() > 3 {
                                println!("    ... and {} more", results.len() - 3);
                            }
                        }
                        Ok(other) => {
                            println!("    Peer message: {:?}", std::mem::discriminant(&other));
                        }
                        Err(e) => {
                            println!("    Peer parse error: {}", e);
                        }
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
