//! # slsk-rs
//!
//! A Rust implementation of the SoulSeek protocol.
//!
//! This library provides types and utilities for encoding/decoding SoulSeek protocol messages
//! for server, peer, file transfer, and distributed network communication.

pub mod constants;
pub mod error;
pub mod protocol;

pub mod server;
pub mod peer_init;
pub mod peer;
pub mod file;
pub mod distributed;

pub use error::{Error, Result};
pub use protocol::{MessageRead, MessageWrite, ProtocolRead, ProtocolWrite};
