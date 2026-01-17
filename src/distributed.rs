//! Distributed network messages sent over D connections.
//!
//! These messages are used for the distributed search network.

use bytes::{Buf, BufMut};

use crate::protocol::{MessageRead, MessageWrite, ProtocolRead, ProtocolWrite};
use crate::{Error, Result};

/// Distributed message codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DistributedCode {
    Ping = 0,
    Search = 3,
    BranchLevel = 4,
    BranchRoot = 5,
    ChildDepth = 7,
    EmbeddedMessage = 93,
}

impl TryFrom<u8> for DistributedCode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(DistributedCode::Ping),
            3 => Ok(DistributedCode::Search),
            4 => Ok(DistributedCode::BranchLevel),
            5 => Ok(DistributedCode::BranchRoot),
            7 => Ok(DistributedCode::ChildDepth),
            93 => Ok(DistributedCode::EmbeddedMessage),
            _ => Err(Error::InvalidDistributedCode(value)),
        }
    }
}

impl From<DistributedCode> for u8 {
    fn from(code: DistributedCode) -> Self {
        code as u8
    }
}

/// Distributed network messages.
#[derive(Debug, Clone)]
pub enum DistributedMessage {
    /// Ping children (deprecated).
    Ping,

    /// Distributed search request.
    Search {
        unknown: u32,
        username: String,
        token: u32,
        query: String,
    },

    /// Branch level in distributed network.
    BranchLevel { level: i32 },

    /// Branch root username.
    BranchRoot { root: String },

    /// Child depth (deprecated).
    ChildDepth { depth: u32 },

    /// Embedded message from branch root.
    EmbeddedMessage { code: u8, data: Vec<u8> },
}

impl MessageWrite for DistributedMessage {
    type Code = DistributedCode;

    fn code(&self) -> DistributedCode {
        match self {
            DistributedMessage::Ping => DistributedCode::Ping,
            DistributedMessage::Search { .. } => DistributedCode::Search,
            DistributedMessage::BranchLevel { .. } => DistributedCode::BranchLevel,
            DistributedMessage::BranchRoot { .. } => DistributedCode::BranchRoot,
            DistributedMessage::ChildDepth { .. } => DistributedCode::ChildDepth,
            DistributedMessage::EmbeddedMessage { .. } => DistributedCode::EmbeddedMessage,
        }
    }

    fn write_payload<B: BufMut>(&self, buf: &mut B) {
        match self {
            DistributedMessage::Ping => {}
            DistributedMessage::Search {
                unknown,
                username,
                token,
                query,
            } => {
                unknown.write_to(buf);
                username.write_to(buf);
                token.write_to(buf);
                query.write_to(buf);
            }
            DistributedMessage::BranchLevel { level } => {
                level.write_to(buf);
            }
            DistributedMessage::BranchRoot { root } => {
                root.write_to(buf);
            }
            DistributedMessage::ChildDepth { depth } => {
                depth.write_to(buf);
            }
            DistributedMessage::EmbeddedMessage { code, data } => {
                code.write_to(buf);
                buf.put_slice(data);
            }
        }
    }
}

impl MessageRead for DistributedMessage {
    type Code = DistributedCode;

    fn read_with_code<B: Buf>(code: DistributedCode, buf: &mut B) -> Result<Self> {
        match code {
            DistributedCode::Ping => Ok(DistributedMessage::Ping),
            DistributedCode::Search => {
                let unknown = u32::read_from(buf)?;
                let username = String::read_from(buf)?;
                let token = u32::read_from(buf)?;
                let query = String::read_from(buf)?;
                Ok(DistributedMessage::Search {
                    unknown,
                    username,
                    token,
                    query,
                })
            }
            DistributedCode::BranchLevel => {
                let level = i32::read_from(buf)?;
                Ok(DistributedMessage::BranchLevel { level })
            }
            DistributedCode::BranchRoot => {
                let root = String::read_from(buf)?;
                Ok(DistributedMessage::BranchRoot { root })
            }
            DistributedCode::ChildDepth => {
                let depth = u32::read_from(buf)?;
                Ok(DistributedMessage::ChildDepth { depth })
            }
            DistributedCode::EmbeddedMessage => {
                let inner_code = u8::read_from(buf)?;
                let mut data = vec![0u8; buf.remaining()];
                buf.copy_to_slice(&mut data);
                Ok(DistributedMessage::EmbeddedMessage {
                    code: inner_code,
                    data,
                })
            }
        }
    }
}

/// Read a distributed message from a buffer (including length prefix).
pub fn read_distributed_message<B: Buf>(buf: &mut B) -> Result<DistributedMessage> {
    let _len = u32::read_from(buf)?;
    let code = DistributedCode::try_from(u8::read_from(buf)?)?;
    DistributedMessage::read_with_code(code, buf)
}

/// Write a distributed message to a buffer (with length prefix and code).
pub fn write_distributed_message<B: BufMut>(msg: &DistributedMessage, buf: &mut B) {
    msg.write_message_u8(buf);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_search_roundtrip() {
        let msg = DistributedMessage::Search {
            unknown: 0,
            username: "testuser".to_string(),
            token: 12345,
            query: "test query".to_string(),
        };
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);

        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();
        match parsed {
            DistributedMessage::Search {
                unknown,
                username,
                token,
                query,
            } => {
                assert_eq!(unknown, 0);
                assert_eq!(username, "testuser");
                assert_eq!(token, 12345);
                assert_eq!(query, "test query");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_branch_level_roundtrip() {
        let msg = DistributedMessage::BranchLevel { level: 5 };
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);

        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();
        match parsed {
            DistributedMessage::BranchLevel { level } => {
                assert_eq!(level, 5);
            }
            _ => panic!("Wrong message type"),
        }
    }
}
