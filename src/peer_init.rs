//! Peer initialization messages.
//!
//! Peer init messages are used to initiate P, F, or D connections to a peer.

use bytes::{Buf, BufMut};

use crate::constants::ConnectionType;
use crate::protocol::{MessageRead, MessageWrite, ProtocolRead, ProtocolWrite};
use crate::{Error, Result};

/// Peer init message codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PeerInitCode {
    PierceFirewall = 0,
    PeerInit = 1,
}

impl TryFrom<u8> for PeerInitCode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            0 => Ok(PeerInitCode::PierceFirewall),
            1 => Ok(PeerInitCode::PeerInit),
            _ => Err(Error::InvalidPeerInitCode(value)),
        }
    }
}

impl From<PeerInitCode> for u8 {
    fn from(code: PeerInitCode) -> Self {
        code as u8
    }
}

/// Peer initialization messages.
#[derive(Debug, Clone)]
pub enum PeerInitMessage {
    /// Response to an indirect connection request.
    /// Token is from ConnectToPeer server message.
    PierceFirewall { token: u32 },

    /// Initiate a direct connection to another peer.
    PeerInit {
        username: String,
        connection_type: ConnectionType,
        token: u32,
    },
}

impl MessageWrite for PeerInitMessage {
    type Code = PeerInitCode;

    fn code(&self) -> PeerInitCode {
        match self {
            PeerInitMessage::PierceFirewall { .. } => PeerInitCode::PierceFirewall,
            PeerInitMessage::PeerInit { .. } => PeerInitCode::PeerInit,
        }
    }

    fn write_payload<B: BufMut>(&self, buf: &mut B) {
        match self {
            PeerInitMessage::PierceFirewall { token } => {
                token.write_to(buf);
            }
            PeerInitMessage::PeerInit {
                username,
                connection_type,
                token,
            } => {
                username.write_to(buf);
                connection_type.as_str().write_to(buf);
                token.write_to(buf);
            }
        }
    }
}

impl MessageRead for PeerInitMessage {
    type Code = PeerInitCode;

    fn read_with_code<B: Buf>(code: PeerInitCode, buf: &mut B) -> Result<Self> {
        match code {
            PeerInitCode::PierceFirewall => {
                let token = u32::read_from(buf)?;
                Ok(PeerInitMessage::PierceFirewall { token })
            }
            PeerInitCode::PeerInit => {
                let username = String::read_from(buf)?;
                let conn_type_str = String::read_from(buf)?;
                let connection_type = ConnectionType::parse(&conn_type_str)?;
                let token = u32::read_from(buf)?;
                Ok(PeerInitMessage::PeerInit {
                    username,
                    connection_type,
                    token,
                })
            }
        }
    }
}

/// Read a peer init message from a buffer (including length prefix).
pub fn read_peer_init_message<B: Buf>(buf: &mut B) -> Result<PeerInitMessage> {
    let _len = u32::read_from(buf)?;
    let code = PeerInitCode::try_from(u8::read_from(buf)?)?;
    PeerInitMessage::read_with_code(code, buf)
}

/// Write a peer init message to a buffer (with length prefix and code).
pub fn write_peer_init_message<B: BufMut>(msg: &PeerInitMessage, buf: &mut B) {
    msg.write_message_u8(buf);
}

/// Check if the buffer contains a complete peer init message.
///
/// Returns the total message size (including 4-byte length prefix) if complete,
/// or `None` if more data is needed.
///
/// Use this before calling `read_peer_init_message` to avoid buffer underflow errors.
pub fn peer_init_message_size(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    let msg_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let total = 4 + msg_len;
    if buf.len() >= total {
        Some(total)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{Buf, BytesMut};

    #[test]
    fn test_pierce_firewall_roundtrip() {
        let msg = PeerInitMessage::PierceFirewall { token: 12345 };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);

        let parsed = read_peer_init_message(&mut buf.freeze()).unwrap();
        match parsed {
            PeerInitMessage::PierceFirewall { token } => assert_eq!(token, 12345),
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_peer_init_roundtrip() {
        let msg = PeerInitMessage::PeerInit {
            username: "testuser".to_string(),
            connection_type: ConnectionType::Peer,
            token: 0,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);

        let parsed = read_peer_init_message(&mut buf.freeze()).unwrap();
        match parsed {
            PeerInitMessage::PeerInit {
                username,
                connection_type,
                token,
            } => {
                assert_eq!(username, "testuser");
                assert_eq!(connection_type, ConnectionType::Peer);
                assert_eq!(token, 0);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_read_peer_init_incomplete_length() {
        // Only 3 bytes - not enough for the 4-byte length prefix
        let mut buf = BytesMut::from(&[0u8, 1, 2][..]);
        let result = read_peer_init_message(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_peer_init_incomplete_payload() {
        // Write a valid message, then truncate it
        let msg = PeerInitMessage::PeerInit {
            username: "testuser".to_string(),
            connection_type: ConnectionType::Peer,
            token: 12345,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);

        // Truncate to only have length + partial payload
        buf.truncate(6);
        let result = read_peer_init_message(&mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_peer_init_with_trailing_data() {
        // Simulate TCP delivering init message + extra data in one read
        let msg = PeerInitMessage::PeerInit {
            username: "testuser".to_string(),
            connection_type: ConnectionType::Peer,
            token: 42,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);
        // Append trailing "garbage" (simulating start of next message)
        buf.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03, 0x04]);

        let parsed = read_peer_init_message(&mut buf).unwrap();
        match parsed {
            PeerInitMessage::PeerInit {
                username, token, ..
            } => {
                assert_eq!(username, "testuser");
                assert_eq!(token, 42);
            }
            _ => panic!("Wrong message type"),
        }

        // Verify trailing data is still in buffer (not consumed)
        assert_eq!(buf.remaining(), 8);
        assert_eq!(&buf[..4], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_peer_init_message_size() {
        let msg = PeerInitMessage::PeerInit {
            username: "user".to_string(),
            connection_type: ConnectionType::Peer,
            token: 1,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);
        let complete_len = buf.len();

        // Empty buffer
        assert_eq!(peer_init_message_size(&[]), None);

        // Partial length (only 3 bytes, need 4 for length prefix)
        assert_eq!(peer_init_message_size(&buf[..3]), None);

        // Length only, no payload
        assert_eq!(peer_init_message_size(&buf[..4]), None);

        // Partial payload
        assert_eq!(peer_init_message_size(&buf[..complete_len - 1]), None);

        // Complete message - returns total size
        assert_eq!(peer_init_message_size(&buf), Some(complete_len));

        // Complete message + extra data - still returns original message size
        buf.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(peer_init_message_size(&buf), Some(complete_len));
    }
}
