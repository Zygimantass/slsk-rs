//! Message handlers for client requests.

use std::collections::HashMap;
use std::net::Ipv4Addr;

use anyhow::Result;
use bytes::BytesMut;
use slsk_rs::constants::{ConnectionType, ObfuscationType, UserStatus};
use slsk_rs::peer::{PeerMessage, SearchResultFile};
use slsk_rs::peer_init::{PeerInitMessage, write_peer_init_message};
use slsk_rs::protocol::MessageWrite;
use slsk_rs::server::{PossibleParent, ServerRequest, ServerResponse, UserStats};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::config::Config;
use crate::connection::SessionInfo;
use crate::state::{SharedState, UserSession};

/// Handle a client message, returns Some(username) if login succeeded
pub async fn handle_client_message(
    request: ServerRequest,
    session: SessionInfo,
    state: &SharedState,
    config: &Config,
) -> Result<Option<String>> {
    match request {
        ServerRequest::Login {
            username,
            password,
            version,
            ..
        } => {
            handle_login(username, password, version, session, state, config).await
        }

        ServerRequest::SetWaitPort {
            port,
            obfuscated_port,
            ..
        } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.port = port;
                    user.obfuscated_port = obfuscated_port;
                }
            }
            Ok(None)
        }

        ServerRequest::SetStatus { status } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.status = status;
                }
            }
            Ok(None)
        }

        ServerRequest::SharedFoldersFiles { dirs, files } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.shared_folders = dirs;
                    user.shared_files = files;
                }
            }
            Ok(None)
        }

        ServerRequest::GetPeerAddress { username: target } => {
            let state = state.read().await;
            let mut buf = BytesMut::new();

            let response = if let Some(user) = state.get_user(&target) {
                ServerResponse::GetPeerAddress {
                    username: target,
                    ip: user.ip,
                    port: user.port,
                    obfuscation_type: ObfuscationType::None,
                    obfuscated_port: 0,
                }
            } else {
                ServerResponse::GetPeerAddress {
                    username: target,
                    ip: std::net::Ipv4Addr::new(0, 0, 0, 0),
                    port: 0,
                    obfuscation_type: ObfuscationType::None,
                    obfuscated_port: 0,
                }
            };

            response.write_message(&mut buf);
            let _ = session.tx.send(buf);
            Ok(None)
        }

        ServerRequest::GetUserStatus { username: target } => {
            let state = state.read().await;
            let mut buf = BytesMut::new();

            let (status, privileged) = if let Some(user) = state.get_user(&target) {
                (user.status, user.privileged)
            } else {
                (UserStatus::Offline, false)
            };

            let response = ServerResponse::GetUserStatus {
                username: target,
                status,
                privileged,
            };
            response.write_message(&mut buf);
            let _ = session.tx.send(buf);
            Ok(None)
        }

        ServerRequest::GetUserStats { username: target } => {
            let state = state.read().await;
            let mut buf = BytesMut::new();

            let stats = if let Some(user) = state.get_user(&target) {
                UserStats {
                    avg_speed: user.avg_speed,
                    upload_num: user.upload_count,
                    unknown: 0,
                    files: user.shared_files,
                    dirs: user.shared_folders,
                }
            } else {
                UserStats::default()
            };

            let response = ServerResponse::GetUserStats {
                username: target,
                stats,
            };
            response.write_message(&mut buf);
            let _ = session.tx.send(buf);
            Ok(None)
        }

        ServerRequest::WatchUser { username: target } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;

                // Add to watch list
                if let Some(user) = state.get_user_mut(username) {
                    user.watched_users.insert(target.clone());
                }

                // Send current status
                let mut buf = BytesMut::new();
                if let Some(target_user) = state.get_user(&target) {
                    let response = ServerResponse::WatchUser {
                        username: target.clone(),
                        exists: true,
                        status: Some(target_user.status),
                        stats: Some(UserStats {
                            avg_speed: target_user.avg_speed,
                            upload_num: target_user.upload_count,
                            unknown: 0,
                            files: target_user.shared_files,
                            dirs: target_user.shared_folders,
                        }),
                        country_code: None,
                    };
                    response.write_message(&mut buf);
                } else {
                    let response = ServerResponse::WatchUser {
                        username: target,
                        exists: false,
                        status: None,
                        stats: None,
                        country_code: None,
                    };
                    response.write_message(&mut buf);
                }
                let _ = session.tx.send(buf);
            }
            Ok(None)
        }

        ServerRequest::UnwatchUser { username: target } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.watched_users.remove(&target);
                }
            }
            Ok(None)
        }

        ServerRequest::FileSearch { token, query } => {
            handle_file_search(token, query, session, state, config).await
        }

        ServerRequest::HaveNoParent { no_parent } => {
            if no_parent {
                if let Some(ref username) = session.username {
                    send_potential_parents(username, &session.tx, state, config).await;
                }
            }
            Ok(None)
        }

        ServerRequest::AcceptChildren { accept } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.accepts_children = accept;
                }
                state.update_potential_parents(config.max_distributed_depth);
            }
            Ok(None)
        }

        ServerRequest::BranchLevel { level } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.branch_level = level as i32;
                    if level == 0 {
                        state.branch_roots.insert(username.clone());
                    } else {
                        state.branch_roots.remove(username);
                    }
                }
                state.update_potential_parents(config.max_distributed_depth);
            }
            Ok(None)
        }

        ServerRequest::BranchRoot { root } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.branch_root = Some(root);
                }
            }
            Ok(None)
        }

        ServerRequest::RoomList => {
            let state = state.read().await;
            let mut buf = BytesMut::new();

            let rooms: Vec<(String, u32)> = state
                .rooms
                .values()
                .filter(|r| !r.is_private)
                .map(|r| (r.name.clone(), r.users.len() as u32))
                .collect();

            let response = ServerResponse::RoomList {
                rooms,
                owned_private_rooms: vec![],
                private_rooms: vec![],
                operated_private_rooms: vec![],
            };
            response.write_message(&mut buf);
            let _ = session.tx.send(buf);
            Ok(None)
        }

        ServerRequest::JoinRoom { room, .. } => {
            if let Some(ref username) = session.username {
                handle_join_room(username, &room, &session.tx, state).await;
            }
            Ok(None)
        }

        ServerRequest::LeaveRoom { room } => {
            if let Some(ref username) = session.username {
                handle_leave_room(username, &room, state).await;
            }
            Ok(None)
        }

        ServerRequest::SayChatroom { room, message } => {
            if let Some(ref username) = session.username {
                handle_say_chatroom(username, &room, &message, state).await;
            }
            Ok(None)
        }

        ServerRequest::ConnectToPeer {
            token,
            username: target,
            connection_type,
        } => {
            // Forward connection request to target user
            let state = state.read().await;
            if let (Some(username), Some(target_user)) =
                (&session.username, state.get_user(&target))
            {
                if let Some(requester) = state.get_user(username) {
                    let mut buf = BytesMut::new();
                    let response = ServerResponse::ConnectToPeer {
                        username: username.clone(),
                        connection_type,
                        ip: requester.ip,
                        port: requester.port,
                        token,
                        privileged: requester.privileged,
                        obfuscation_type: ObfuscationType::None,
                        obfuscated_port: 0,
                    };
                    response.write_message(&mut buf);
                    let _ = target_user.tx.send(buf);
                }
            }
            Ok(None)
        }

        ServerRequest::MessageUser { username: target, message } => {
            if let Some(ref username) = session.username {
                handle_private_message(username, &target, &message, state).await;
            }
            Ok(None)
        }

        ServerRequest::CheckPrivileges => {
            let mut buf = BytesMut::new();
            let response = ServerResponse::CheckPrivileges { time_left: 0 };
            response.write_message(&mut buf);
            let _ = session.tx.send(buf);
            Ok(None)
        }

        ServerRequest::ServerPing => {
            // No response needed
            Ok(None)
        }

        ServerRequest::SendUploadSpeed { speed } => {
            if let Some(ref username) = session.username {
                let mut state = state.write().await;
                if let Some(user) = state.get_user_mut(username) {
                    user.avg_speed = speed;
                    user.upload_count += 1;
                }
            }
            Ok(None)
        }

        _ => {
            // Unhandled message type
            Ok(None)
        }
    }
}

async fn handle_login(
    username: String,
    password: String,
    version: u32,
    session: SessionInfo,
    state: &SharedState,
    config: &Config,
) -> Result<Option<String>> {
    let mut buf = BytesMut::new();

    // Check version
    if version < config.min_version {
        let response = ServerResponse::LoginFailure {
            reason: slsk_rs::constants::LoginRejectionReason::InvalidVersion,
            detail: None,
        };
        response.write_message(&mut buf);
        let _ = session.tx.send(buf);
        return Ok(None);
    }

    // Validate username
    if username.is_empty() || username.len() > 30 {
        let response = ServerResponse::LoginFailure {
            reason: slsk_rs::constants::LoginRejectionReason::InvalidUsername,
            detail: None,
        };
        response.write_message(&mut buf);
        let _ = session.tx.send(buf);
        return Ok(None);
    }

    // Check password
    if password.is_empty() {
        let response = ServerResponse::LoginFailure {
            reason: slsk_rs::constants::LoginRejectionReason::EmptyPassword,
            detail: None,
        };
        response.write_message(&mut buf);
        let _ = session.tx.send(buf);
        return Ok(None);
    }

    let password_hash = format!("{:x}", md5::compute(&password));

    let mut state = state.write().await;

    // Check if already logged in
    if state.is_online(&username) {
        // Disconnect existing session (relogged)
        if let Some(old_session) = state.remove_user(&username) {
            let mut relogged_buf = BytesMut::new();
            let relogged = ServerResponse::Relogged;
            relogged.write_message(&mut relogged_buf);
            let _ = old_session.tx.send(relogged_buf);
        }
    }

    // Check server capacity
    if state.online_count() >= config.max_users {
        let response = ServerResponse::LoginFailure {
            reason: slsk_rs::constants::LoginRejectionReason::ServerFull,
            detail: None,
        };
        response.write_message(&mut buf);
        let _ = session.tx.send(buf);
        return Ok(None);
    }

    // Register or verify credentials
    match state.register_or_verify(&username, &password_hash) {
        Ok(_) => {
            // Login success
            let user_session = UserSession::new(
                session.connection_id,
                username.clone(),
                password_hash.clone(),
                session.ip,
                session.tx.clone(),
            );

            let privileged = state
                .registered
                .get(&username)
                .map(|r| r.privileged)
                .unwrap_or(false);

            state.add_user(user_session);

            println!("User logged in: {} from {}", username, session.ip);

            // Send login success
            let response = ServerResponse::LoginSuccess {
                greet: config.motd.clone(),
                own_ip: session.ip,
                password_hash,
                is_supporter: privileged,
            };
            response.write_message(&mut buf);
            let _ = session.tx.send(buf);

            // Send distributed network params
            let mut buf2 = BytesMut::new();
            let parent_speed = ServerResponse::ParentMinSpeed { speed: 1 };
            parent_speed.write_message(&mut buf2);
            let _ = session.tx.send(buf2);

            let mut buf3 = BytesMut::new();
            let speed_ratio = ServerResponse::ParentSpeedRatio { ratio: 50 };
            speed_ratio.write_message(&mut buf3);
            let _ = session.tx.send(buf3);

            let mut buf4 = BytesMut::new();
            let wishlist_interval = ServerResponse::WishlistInterval { interval: 720 };
            wishlist_interval.write_message(&mut buf4);
            let _ = session.tx.send(buf4);

            Ok(Some(username))
        }
        Err(reason) => {
            let response = ServerResponse::LoginFailure {
                reason: slsk_rs::constants::LoginRejectionReason::InvalidPassword,
                detail: Some(reason.to_string()),
            };
            response.write_message(&mut buf);
            let _ = session.tx.send(buf);
            Ok(None)
        }
    }
}

async fn handle_file_search(
    token: u32,
    query: String,
    session: SessionInfo,
    state: &SharedState,
    _config: &Config,
) -> Result<Option<String>> {
    let Some(ref _username) = session.username else {
        return Ok(None);
    };

    // Get the client's listen port and IP
    let (client_ip, client_port) = {
        let state = state.read().await;
        if let Some(ref username) = session.username {
            if let Some(user) = state.get_user(username) {
                (user.ip, user.port)
            } else {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
    };

    if client_port == 0 {
        return Ok(None);
    }

    // Search the local index
    let db_path = std::env::var("SLSK_INDEX_DB").unwrap_or_else(|_| "slsk_index.db".to_string());
    let db = match slsk_rs::db::Database::open(&db_path) {
        Ok(db) => db,
        Err(_) => return Ok(None),
    };

    let results = match db.search(&query, 200) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    if results.is_empty() {
        return Ok(None);
    }

    // Group results by username
    let mut by_user: HashMap<String, Vec<SearchResultFile>> = HashMap::new();
    for result in results {
        let extension = result
            .filename
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_string();

        by_user.entry(result.username).or_default().push(SearchResultFile {
            filename: result.filename,
            size: result.size,
            extension,
            attributes: vec![],
        });
    }

    println!("Search '{}': {} results from {} users", query, by_user.values().map(|v| v.len()).sum::<usize>(), by_user.len());

    // Connect to the client and send results as each user
    let client_ip = client_ip;
    let client_port = client_port;

    for (peer_username, files) in by_user {
        let addr = format!("{}:{}", client_ip, client_port);
        let peer_user = peer_username.clone();

        tokio::spawn(async move {
            if let Ok(mut stream) = TcpStream::connect(&addr).await {
                // Send PeerInit identifying as the peer user
                let init = PeerInitMessage::PeerInit {
                    username: peer_user.clone(),
                    connection_type: ConnectionType::Peer,
                    token: 0,
                };
                let mut buf = BytesMut::new();
                write_peer_init_message(&init, &mut buf);
                let _ = stream.write_all(&buf).await;

                // Send FileSearchResponse
                buf.clear();
                let response = PeerMessage::FileSearchResponse {
                    username: peer_user,
                    token,
                    results: files,
                    slot_free: true,
                    avg_speed: 0,
                    queue_length: 0,
                    private_results: vec![],
                };
                response.write_message(&mut buf);
                let _ = stream.write_all(&buf).await;
                let _ = stream.flush().await;
            }
        });
    }

    Ok(None)
}

async fn send_potential_parents(
    _username: &str,
    tx: &tokio::sync::mpsc::UnboundedSender<BytesMut>,
    state: &SharedState,
    config: &Config,
) {
    let state = state.read().await;

    let parents: Vec<PossibleParent> = state
        .potential_parents
        .iter()
        .take(config.potential_parents_count as usize)
        .map(|p| PossibleParent {
            username: p.username.clone(),
            ip: p.ip,
            port: p.port,
        })
        .collect();

    if !parents.is_empty() {
        let mut buf = BytesMut::new();
        let response = ServerResponse::PossibleParents { parents };
        response.write_message(&mut buf);
        let _ = tx.send(buf);
    }
}

async fn handle_join_room(
    username: &str,
    room_name: &str,
    tx: &tokio::sync::mpsc::UnboundedSender<BytesMut>,
    state: &SharedState,
) {
    let mut state = state.write().await;

    let room = state.get_or_create_room(room_name);
    room.users.insert(username.to_string());

    // Get user list for the room
    let users: Vec<String> = room.users.iter().cloned().collect();

    // Notify others that user joined
    for other_username in &users {
        if other_username != username {
            if let Some(other_user) = state.get_user(other_username) {
                let mut buf = BytesMut::new();
                let user_stats = state.get_user(username).map(|u| UserStats {
                    avg_speed: u.avg_speed,
                    upload_num: u.upload_count,
                    unknown: 0,
                    files: u.shared_files,
                    dirs: u.shared_folders,
                });

                let msg = ServerResponse::UserJoinedRoom {
                    room: room_name.to_string(),
                    username: username.to_string(),
                    status: UserStatus::Online,
                    stats: user_stats.unwrap_or_default(),
                    slots_full: false,
                    country_code: String::new(),
                };
                msg.write_message(&mut buf);
                let _ = other_user.tx.send(buf);
            }
        }
    }

    // Add room to user's joined rooms
    if let Some(user) = state.get_user_mut(username) {
        user.joined_rooms.insert(room_name.to_string());
    }

    // Build room info for joiner
    let room_users: Vec<slsk_rs::server::RoomUser> = users
        .iter()
        .filter_map(|u| {
            state.get_user(u).map(|user| slsk_rs::server::RoomUser {
                username: u.clone(),
                status: user.status,
                stats: UserStats {
                    avg_speed: user.avg_speed,
                    upload_num: user.upload_count,
                    unknown: 0,
                    files: user.shared_files,
                    dirs: user.shared_folders,
                },
                slots_full: false,
                country_code: String::new(),
            })
        })
        .collect();

    let tickers: Vec<slsk_rs::server::RoomTicker> = state
        .rooms
        .get(room_name)
        .map(|r| {
            r.tickers
                .iter()
                .map(|(u, t)| slsk_rs::server::RoomTicker {
                    username: u.clone(),
                    ticker: t.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    // Send JoinRoom response
    let mut buf = BytesMut::new();
    let response = ServerResponse::JoinRoom {
        room: room_name.to_string(),
        users: room_users,
        owner: None,
        operators: vec![],
    };
    response.write_message(&mut buf);
    let _ = tx.send(buf);

    // Send tickers
    if !tickers.is_empty() {
        let mut ticker_buf = BytesMut::new();
        let ticker_msg = ServerResponse::RoomTickerState {
            room: room_name.to_string(),
            tickers,
        };
        ticker_msg.write_message(&mut ticker_buf);
        let _ = tx.send(ticker_buf);
    }
}

async fn handle_leave_room(username: &str, room_name: &str, state: &SharedState) {
    let mut state = state.write().await;

    if let Some(room) = state.rooms.get_mut(room_name) {
        room.users.remove(username);

        // Notify others
        let users: Vec<_> = room.users.iter().cloned().collect();
        for other_username in users {
            if let Some(other_user) = state.get_user(&other_username) {
                let mut buf = BytesMut::new();
                let msg = ServerResponse::UserLeftRoom {
                    room: room_name.to_string(),
                    username: username.to_string(),
                };
                msg.write_message(&mut buf);
                let _ = other_user.tx.send(buf);
            }
        }
    }

    if let Some(user) = state.get_user_mut(username) {
        user.joined_rooms.remove(room_name);
    }
}

async fn handle_say_chatroom(username: &str, room_name: &str, message: &str, state: &SharedState) {
    let state = state.read().await;

    if let Some(room) = state.rooms.get(room_name) {
        for other_username in &room.users {
            if let Some(other_user) = state.get_user(other_username) {
                let mut buf = BytesMut::new();
                let msg = ServerResponse::SayChatroom {
                    room: room_name.to_string(),
                    username: username.to_string(),
                    message: message.to_string(),
                };
                msg.write_message(&mut buf);
                let _ = other_user.tx.send(buf);
            }
        }
    }
}

async fn handle_private_message(
    from: &str,
    to: &str,
    message: &str,
    state: &SharedState,
) {
    let state = state.read().await;

    if let Some(target_user) = state.get_user(to) {
        let mut buf = BytesMut::new();
        let msg = ServerResponse::MessageUser {
            id: 0, // TODO: message ID tracking
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as u32)
                .unwrap_or(0),
            username: from.to_string(),
            message: message.to_string(),
            new_message: true,
        };
        msg.write_message(&mut buf);
        let _ = target_user.tx.send(buf);
    }
}
