//! Server state management.

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use bytes::BytesMut;
use slsk_rs::constants::UserStatus;
use tokio::sync::{RwLock, mpsc};

static CONNECTION_ID: AtomicU32 = AtomicU32::new(1);

pub fn next_connection_id() -> u32 {
    CONNECTION_ID.fetch_add(1, Ordering::SeqCst)
}

/// A connected user session
#[derive(Debug)]
pub struct UserSession {
    pub id: u32,
    pub username: String,
    pub password_hash: String,
    pub status: UserStatus,
    pub ip: Ipv4Addr,
    pub port: u32,
    pub obfuscated_port: Option<u32>,

    /// Channel to send messages to this user
    pub tx: mpsc::UnboundedSender<BytesMut>,

    /// User statistics
    pub avg_speed: u32,
    pub upload_count: u32,
    pub shared_files: u32,
    pub shared_folders: u32,

    /// Distributed network info
    pub branch_level: i32,
    pub branch_root: Option<String>,
    pub accepts_children: bool,
    pub child_depth: u32,

    /// Privileged (donor)
    pub privileged: bool,

    /// Rooms joined
    pub joined_rooms: HashSet<String>,

    /// Users being watched
    pub watched_users: HashSet<String>,
}

impl UserSession {
    pub fn new(
        id: u32,
        username: String,
        password_hash: String,
        ip: Ipv4Addr,
        tx: mpsc::UnboundedSender<BytesMut>,
    ) -> Self {
        Self {
            id,
            username,
            password_hash,
            status: UserStatus::Online,
            ip,
            port: 0,
            obfuscated_port: None,
            tx,
            avg_speed: 0,
            upload_count: 0,
            shared_files: 0,
            shared_folders: 0,
            branch_level: -1,
            branch_root: None,
            accepts_children: false,
            child_depth: 0,
            privileged: false,
            joined_rooms: HashSet::new(),
            watched_users: HashSet::new(),
        }
    }

    pub fn send(&self, msg: BytesMut) -> bool {
        self.tx.send(msg).is_ok()
    }
}

/// A chat room
#[derive(Debug, Default)]
pub struct Room {
    pub name: String,
    pub users: HashSet<String>,
    pub is_private: bool,
    pub owner: Option<String>,
    pub operators: HashSet<String>,
    pub members: HashSet<String>,
    pub tickers: HashMap<String, String>,
}

impl Room {
    pub fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }
}

/// Distributed network node for parent selection
#[derive(Debug, Clone)]
pub struct DistributedNode {
    pub username: String,
    pub ip: Ipv4Addr,
    pub port: u32,
    pub branch_level: i32,
}

/// Registered user (persisted)
#[derive(Debug, Clone)]
pub struct RegisteredUser {
    pub username: String,
    pub password_hash: String,
    pub privileged: bool,
}

/// The main server state
#[derive(Debug, Default)]
pub struct ServerState {
    /// Connected users by username
    pub users: HashMap<String, UserSession>,

    /// Connection ID to username mapping
    pub connections: HashMap<u32, String>,

    /// Registered users (username -> password hash)
    pub registered: HashMap<String, RegisteredUser>,

    /// Chat rooms
    pub rooms: HashMap<String, Room>,

    /// Branch roots (level 0 users)
    pub branch_roots: HashSet<String>,

    /// Users who accept children
    pub potential_parents: Vec<DistributedNode>,

    /// Search token counter
    search_token: AtomicU32,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            search_token: AtomicU32::new(1),
            ..Default::default()
        }
    }

    pub fn next_search_token(&self) -> u32 {
        self.search_token.fetch_add(1, Ordering::SeqCst)
    }

    pub fn add_user(&mut self, session: UserSession) {
        let username = session.username.clone();
        let id = session.id;
        self.users.insert(username.clone(), session);
        self.connections.insert(id, username);
    }

    pub fn remove_user(&mut self, username: &str) -> Option<UserSession> {
        if let Some(session) = self.users.remove(username) {
            self.connections.remove(&session.id);
            self.branch_roots.remove(username);
            self.potential_parents.retain(|p| p.username != username);

            for room in self.rooms.values_mut() {
                room.users.remove(username);
            }

            Some(session)
        } else {
            None
        }
    }

    pub fn get_user(&self, username: &str) -> Option<&UserSession> {
        self.users.get(username)
    }

    pub fn get_user_mut(&mut self, username: &str) -> Option<&mut UserSession> {
        self.users.get_mut(username)
    }

    pub fn is_online(&self, username: &str) -> bool {
        self.users.contains_key(username)
    }

    pub fn online_count(&self) -> u32 {
        self.users.len() as u32
    }

    pub fn get_or_create_room(&mut self, name: &str) -> &mut Room {
        if !self.rooms.contains_key(name) {
            self.rooms.insert(name.to_string(), Room::new(name.to_string()));
        }
        self.rooms.get_mut(name).unwrap()
    }

    pub fn update_potential_parents(&mut self, max_depth: u32) {
        self.potential_parents = self
            .users
            .values()
            .filter(|u| {
                u.accepts_children
                    && u.branch_level >= 0
                    && (u.branch_level as u32 + u.child_depth) < max_depth
            })
            .map(|u| DistributedNode {
                username: u.username.clone(),
                ip: u.ip,
                port: u.port,
                branch_level: u.branch_level,
            })
            .collect();

        self.potential_parents.sort_by_key(|p| p.branch_level);
    }

    /// Register a new user or verify existing credentials
    pub fn register_or_verify(
        &mut self,
        username: &str,
        password_hash: &str,
    ) -> Result<bool, &'static str> {
        if let Some(registered) = self.registered.get(username) {
            if registered.password_hash == password_hash {
                Ok(true)
            } else {
                Err("INVALIDPASS")
            }
        } else {
            // New user - register them
            self.registered.insert(
                username.to_string(),
                RegisteredUser {
                    username: username.to_string(),
                    password_hash: password_hash.to_string(),
                    privileged: false,
                },
            );
            Ok(false)
        }
    }
}

pub type SharedState = Arc<RwLock<ServerState>>;
