//! slsk-server: A Soulseek server implementation in Rust
//!
//! This server handles:
//! - Client authentication and session management
//! - Search request distribution via the distributed network
//! - Chat rooms and private messaging
//! - User status and statistics tracking

mod config;
mod connection;
mod handlers;
mod state;

use std::sync::Arc;

use anyhow::Result;
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use config::Config;
use connection::handle_connection;
use state::ServerState;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let config = Config::load_or_default("slsk-server.toml")?;

    println!("╔════════════════════════════════════════╗");
    println!("║         slsk-server Soulseek Server    ║");
    println!("╠════════════════════════════════════════╣");
    println!("║ Port: {:<33}║", config.port);
    println!("║ Max users: {:<28}║", config.max_users);
    println!("╚════════════════════════════════════════╝");

    let state = Arc::new(RwLock::new(ServerState::new()));
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.port)).await?;

    println!("Listening on 0.0.0.0:{}", config.port);

    loop {
        let (stream, addr) = listener.accept().await?;
        let state = state.clone();
        let config = config.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, addr, state, config).await {
                eprintln!("Connection error from {}: {}", addr, e);
            }
        });
    }
}
