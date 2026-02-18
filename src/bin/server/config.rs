//! Server configuration.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Port to listen on
    pub port: u16,

    /// Maximum number of connected users
    pub max_users: u32,

    /// Message of the day shown on login
    pub motd: String,

    /// Whether the server is in private mode (registration disabled)
    pub private_mode: bool,

    /// Minimum client version allowed
    pub min_version: u32,

    /// Maximum depth of distributed network tree
    pub max_distributed_depth: u32,

    /// Number of potential parents to send to clients
    pub potential_parents_count: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 2416,
            max_users: 100_000,
            motd: "Welcome to slsk-server!".to_string(),
            private_mode: false,
            min_version: 100,
            max_distributed_depth: 8,
            potential_parents_count: 10,
        }
    }
}

impl Config {
    pub fn load_or_default<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            let config = Config::default();
            // Don't write default config file, just use defaults
            Ok(config)
        }
    }
}
