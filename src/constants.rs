//! Protocol constants and enumerations.

use crate::{Error, Result};

/// Connection types used in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionType {
    /// Peer to Peer connection
    Peer,
    /// File Transfer connection
    File,
    /// Distributed Network connection
    Distributed,
}

impl ConnectionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionType::Peer => "P",
            ConnectionType::File => "F",
            ConnectionType::Distributed => "D",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "P" => Ok(ConnectionType::Peer),
            "F" => Ok(ConnectionType::File),
            "D" => Ok(ConnectionType::Distributed),
            _ => Err(Error::InvalidConnectionType(s.to_string())),
        }
    }
}

/// User status codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum UserStatus {
    #[default]
    Offline = 0,
    Away = 1,
    Online = 2,
}

impl TryFrom<u32> for UserStatus {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(UserStatus::Offline),
            1 => Ok(UserStatus::Away),
            2 => Ok(UserStatus::Online),
            _ => Err(Error::InvalidUserStatus(value)),
        }
    }
}

impl From<UserStatus> for u32 {
    fn from(status: UserStatus) -> Self {
        status as u32
    }
}

/// Upload permission settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum UploadPermission {
    #[default]
    NoOne = 0,
    Everyone = 1,
    UsersInList = 2,
    PermittedUsers = 3,
}

impl TryFrom<u32> for UploadPermission {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(UploadPermission::NoOne),
            1 => Ok(UploadPermission::Everyone),
            2 => Ok(UploadPermission::UsersInList),
            3 => Ok(UploadPermission::PermittedUsers),
            _ => Err(Error::Protocol(format!(
                "Invalid upload permission: {}",
                value
            ))),
        }
    }
}

/// Transfer direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum TransferDirection {
    /// Download from peer
    Download = 0,
    /// Upload to peer
    Upload = 1,
}

impl TryFrom<u32> for TransferDirection {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(TransferDirection::Download),
            1 => Ok(TransferDirection::Upload),
            _ => Err(Error::InvalidTransferDirection(value)),
        }
    }
}

impl From<TransferDirection> for u32 {
    fn from(dir: TransferDirection) -> Self {
        dir as u32
    }
}

/// File attribute types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum FileAttributeType {
    /// Bitrate in kbps
    Bitrate = 0,
    /// Duration in seconds
    Duration = 1,
    /// VBR (0 or 1)
    Vbr = 2,
    /// Encoder (unused)
    Encoder = 3,
    /// Sample rate in Hz
    SampleRate = 4,
    /// Bit depth in bits
    BitDepth = 5,
}

impl TryFrom<u32> for FileAttributeType {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(FileAttributeType::Bitrate),
            1 => Ok(FileAttributeType::Duration),
            2 => Ok(FileAttributeType::Vbr),
            3 => Ok(FileAttributeType::Encoder),
            4 => Ok(FileAttributeType::SampleRate),
            5 => Ok(FileAttributeType::BitDepth),
            _ => Err(Error::Protocol(format!(
                "Unknown file attribute type: {}",
                value
            ))),
        }
    }
}

/// Obfuscation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ObfuscationType {
    #[default]
    None = 0,
    Rotated = 1,
}

impl TryFrom<u32> for ObfuscationType {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(ObfuscationType::None),
            1 => Ok(ObfuscationType::Rotated),
            _ => Err(Error::Protocol(format!(
                "Unknown obfuscation type: {}",
                value
            ))),
        }
    }
}

/// Common transfer rejection reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferRejectionReason {
    Banned,
    Cancelled,
    Complete,
    FileNotShared,
    FileReadError,
    PendingShutdown,
    Queued,
    TooManyFiles,
    TooManyMegabytes,
    Other(String),
}

impl TransferRejectionReason {
    pub fn as_str(&self) -> &str {
        match self {
            TransferRejectionReason::Banned => "Banned",
            TransferRejectionReason::Cancelled => "Cancelled",
            TransferRejectionReason::Complete => "Complete",
            TransferRejectionReason::FileNotShared => "File not shared.",
            TransferRejectionReason::FileReadError => "File read error.",
            TransferRejectionReason::PendingShutdown => "Pending shutdown.",
            TransferRejectionReason::Queued => "Queued",
            TransferRejectionReason::TooManyFiles => "Too many files",
            TransferRejectionReason::TooManyMegabytes => "Too many megabytes",
            TransferRejectionReason::Other(s) => s,
        }
    }

    pub fn from_string(s: String) -> Self {
        match s.as_str() {
            "Banned" => TransferRejectionReason::Banned,
            "Cancelled" => TransferRejectionReason::Cancelled,
            "Complete" => TransferRejectionReason::Complete,
            "File not shared." => TransferRejectionReason::FileNotShared,
            "File read error." => TransferRejectionReason::FileReadError,
            "Pending shutdown." => TransferRejectionReason::PendingShutdown,
            "Queued" => TransferRejectionReason::Queued,
            "Too many files" => TransferRejectionReason::TooManyFiles,
            "Too many megabytes" => TransferRejectionReason::TooManyMegabytes,
            _ => TransferRejectionReason::Other(s),
        }
    }
}

/// Login rejection reasons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginRejectionReason {
    InvalidUsername,
    EmptyPassword,
    InvalidPassword,
    InvalidVersion,
    ServerFull,
    ServerPrivate,
    Other(String),
}

impl LoginRejectionReason {
    pub fn from_string(s: String) -> Self {
        match s.as_str() {
            "INVALIDUSERNAME" => LoginRejectionReason::InvalidUsername,
            "EMPTYPASSWORD" => LoginRejectionReason::EmptyPassword,
            "INVALIDPASS" => LoginRejectionReason::InvalidPassword,
            "INVALIDVERSION" => LoginRejectionReason::InvalidVersion,
            "SVRFULL" => LoginRejectionReason::ServerFull,
            "SVRPRIVATE" => LoginRejectionReason::ServerPrivate,
            _ => LoginRejectionReason::Other(s),
        }
    }
}

/// Default client version (matches Nicotine+).
pub const CLIENT_VERSION: u32 = 160;

/// Default listen port for peers.
pub const DEFAULT_PEER_PORT: u16 = 2234;

/// Default server port.
pub const DEFAULT_SERVER_PORT: u16 = 2242;

/// Default Soulseek server address.
pub const DEFAULT_SERVER_HOST: &str = "server.slsknet.org";
