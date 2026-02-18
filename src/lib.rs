//! # slsk-rs
//!
//! A Rust implementation of the SoulSeek protocol.
//!
//! This library provides types and utilities for encoding/decoding SoulSeek protocol messages
//! for server, peer, file transfer, and distributed network communication.

pub mod constants;
pub mod db;
pub mod error;
pub mod protocol;

pub mod distributed;
pub mod file;
pub mod peer;
pub mod peer_init;
pub mod server;

pub use error::{Error, Result};
pub use protocol::{MessageRead, MessageWrite, ProtocolRead, ProtocolWrite};
