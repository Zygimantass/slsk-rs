//! Error types for the slsk-rs library.

use std::io;
use std::string::FromUtf8Error;

/// Result type alias for slsk-rs operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types that can occur during protocol operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("UTF-8 decoding error: {0}")]
    Utf8(#[from] FromUtf8Error),

    #[error("Invalid message code: {0}")]
    InvalidMessageCode(u32),

    #[error("Invalid peer init code: {0}")]
    InvalidPeerInitCode(u8),

    #[error("Invalid distributed code: {0}")]
    InvalidDistributedCode(u8),

    #[error("Buffer underflow: needed {needed} bytes, had {available}")]
    BufferUnderflow { needed: usize, available: usize },

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Compression error: {0}")]
    Compression(String),

    #[error("Invalid connection type: {0}")]
    InvalidConnectionType(String),

    #[error("Invalid user status: {0}")]
    InvalidUserStatus(u32),

    #[error("Invalid transfer direction: {0}")]
    InvalidTransferDirection(u32),

    #[error("Protocol error: {0}")]
    Protocol(String),
}
