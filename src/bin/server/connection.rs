//! Client connection handling.

use std::net::SocketAddr;

use anyhow::Result;
use bytes::BytesMut;
use slsk_rs::server::read_server_request;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::config::Config;
use crate::handlers::handle_client_message;
use crate::state::{SharedState, next_connection_id};

pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    state: SharedState,
    config: Config,
) -> Result<()> {
    let ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => ip,
        std::net::IpAddr::V6(_) => {
            anyhow::bail!("IPv6 not supported");
        }
    };

    stream.set_nodelay(true)?;
    let (mut read_half, mut write_half) = stream.into_split();

    let (tx, mut rx) = mpsc::unbounded_channel::<BytesMut>();
    let connection_id = next_connection_id();

    // Writer task
    let write_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if write_half.write_all(&msg).await.is_err() {
                break;
            }
        }
    });

    let mut read_buf = BytesMut::with_capacity(65536);
    let mut username: Option<String> = None;

    loop {
        let n = read_half.read_buf(&mut read_buf).await?;
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

            match read_server_request(&mut msg_buf) {
                Ok(request) => {
                    let session_info = SessionInfo {
                        connection_id,
                        ip,
                        tx: tx.clone(),
                        username: username.clone(),
                    };

                    match handle_client_message(request, session_info, &state, &config).await {
                        Ok(Some(new_username)) => {
                            username = Some(new_username);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("Handler error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Parse error from {}: {}", addr, e);
                }
            }
        }
    }

    // Clean up on disconnect
    if let Some(ref name) = username {
        let mut state = state.write().await;
        if let Some(session) = state.remove_user(name) {
            println!("User disconnected: {} (was online)", session.username);

            // Notify watchers that user went offline
            let watchers: Vec<_> = state
                .users
                .values()
                .filter(|u| u.watched_users.contains(name))
                .map(|u| u.tx.clone())
                .collect();

            drop(state);

            for watcher_tx in watchers {
                let mut buf = BytesMut::new();
                use slsk_rs::protocol::MessageWrite;
                use slsk_rs::server::ServerResponse;

                let msg = ServerResponse::GetUserStatus {
                    username: name.clone(),
                    status: slsk_rs::constants::UserStatus::Offline,
                    privileged: false,
                };
                msg.write_message(&mut buf);
                let _ = watcher_tx.send(buf);
            }
        }
    }

    write_handle.abort();
    Ok(())
}

#[derive(Clone)]
pub struct SessionInfo {
    pub connection_id: u32,
    pub ip: std::net::Ipv4Addr,
    pub tx: mpsc::UnboundedSender<BytesMut>,
    pub username: Option<String>,
}
