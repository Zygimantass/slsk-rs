//! Integration tests for slsk-rs.
//!
//! These tests require a .env file with SLSK_USERNAME and SLSK_PASSWORD.

use bytes::BytesMut;
use slsk_rs::constants::{ConnectionType, TransferDirection, UserStatus};
use slsk_rs::distributed::{read_distributed_message, write_distributed_message, DistributedMessage};
use slsk_rs::file::{FileOffset, FileTransferInit};
use slsk_rs::peer::{
    read_peer_message, FileAttribute, PeerMessage, SearchResultFile, SharedDirectory, SharedFile,
};
use slsk_rs::peer_init::{read_peer_init_message, write_peer_init_message, PeerInitMessage};
use slsk_rs::protocol::{
    login_hash, zlib_compress, zlib_decompress, ProtocolRead, ProtocolWrite,
};
use slsk_rs::server::{ServerCode, ServerRequest, UserStats};
use slsk_rs::MessageWrite;
use std::net::Ipv4Addr;

fn load_env() -> Option<(String, String)> {
    dotenvy::dotenv().ok();
    let username = std::env::var("SLSK_USERNAME").ok()?;
    let password = std::env::var("SLSK_PASSWORD").ok()?;
    Some((username, password))
}

mod protocol_primitives {
    use super::*;

    #[test]
    fn test_u8_roundtrip() {
        let mut buf = BytesMut::new();
        255u8.write_to(&mut buf);
        assert_eq!(u8::read_from(&mut buf.freeze()).unwrap(), 255);
    }

    #[test]
    fn test_u16_roundtrip() {
        let mut buf = BytesMut::new();
        65535u16.write_to(&mut buf);
        assert_eq!(u16::read_from(&mut buf.freeze()).unwrap(), 65535);
    }

    #[test]
    fn test_u32_roundtrip() {
        let mut buf = BytesMut::new();
        0xDEADBEEFu32.write_to(&mut buf);
        assert_eq!(u32::read_from(&mut buf.freeze()).unwrap(), 0xDEADBEEF);
    }

    #[test]
    fn test_i32_roundtrip() {
        let mut buf = BytesMut::new();
        (-12345i32).write_to(&mut buf);
        assert_eq!(i32::read_from(&mut buf.freeze()).unwrap(), -12345);
    }

    #[test]
    fn test_u64_roundtrip() {
        let mut buf = BytesMut::new();
        0xDEADBEEFCAFEBABEu64.write_to(&mut buf);
        assert_eq!(
            u64::read_from(&mut buf.freeze()).unwrap(),
            0xDEADBEEFCAFEBABE
        );
    }

    #[test]
    fn test_bool_roundtrip() {
        let mut buf = BytesMut::new();
        true.write_to(&mut buf);
        false.write_to(&mut buf);
        let mut frozen = buf.freeze();
        assert!(bool::read_from(&mut frozen).unwrap());
        assert!(!bool::read_from(&mut frozen).unwrap());
    }

    #[test]
    fn test_string_roundtrip() {
        let mut buf = BytesMut::new();
        "hello world".write_to(&mut buf);
        assert_eq!(
            String::read_from(&mut buf.freeze()).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_empty_string_roundtrip() {
        let mut buf = BytesMut::new();
        "".write_to(&mut buf);
        assert_eq!(String::read_from(&mut buf.freeze()).unwrap(), "");
    }

    #[test]
    fn test_unicode_string_roundtrip() {
        let mut buf = BytesMut::new();
        "日本語テスト 🎵".write_to(&mut buf);
        assert_eq!(
            String::read_from(&mut buf.freeze()).unwrap(),
            "日本語テスト 🎵"
        );
    }

    #[test]
    fn test_ipv4_roundtrip() {
        let mut buf = BytesMut::new();
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        ip.write_to(&mut buf);
        assert_eq!(Ipv4Addr::read_from(&mut buf.freeze()).unwrap(), ip);
    }

    #[test]
    fn test_buffer_underflow_u32() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0, 1, 2]); // Only 3 bytes, need 4
        let result = u32::read_from(&mut buf.freeze());
        assert!(result.is_err());
    }
}

mod compression {
    use super::*;

    #[test]
    fn test_zlib_roundtrip() {
        let original = b"The quick brown fox jumps over the lazy dog";
        let compressed = zlib_compress(original).unwrap();
        let decompressed = zlib_decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_zlib_empty() {
        let original = b"";
        let compressed = zlib_compress(original).unwrap();
        let decompressed = zlib_decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_zlib_large_data() {
        let original: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let compressed = zlib_compress(&original).unwrap();
        let decompressed = zlib_decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
        assert!(compressed.len() < original.len());
    }
}

mod login {
    use super::*;

    #[test]
    fn test_login_hash_known_value() {
        let hash = login_hash("username", "password");
        assert_eq!(hash, "d51c9a7e9353746a6020f9602d452929");
    }

    #[test]
    fn test_login_hash_from_env() {
        let Some((username, password)) = load_env() else {
            eprintln!("Skipping test: SLSK_USERNAME/SLSK_PASSWORD not set");
            return;
        };
        let hash = login_hash(&username, &password);
        assert_eq!(hash.len(), 32);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_login_request_serialization() {
        let Some((username, password)) = load_env() else {
            eprintln!("Skipping test: SLSK_USERNAME/SLSK_PASSWORD not set");
            return;
        };
        let req = ServerRequest::Login {
            username: username.clone(),
            password,
            version: 160,
            minor_version: 1,
        };
        let mut buf = BytesMut::new();
        req.write_message(&mut buf);

        // Verify structure: length (4) + code (4) + payload
        assert!(buf.len() > 8);
        let frozen = buf.freeze();
        let len = u32::from_le_bytes([frozen[0], frozen[1], frozen[2], frozen[3]]) as usize;
        assert_eq!(len + 4, frozen.len());

        let code = u32::from_le_bytes([frozen[4], frozen[5], frozen[6], frozen[7]]);
        assert_eq!(code, ServerCode::Login as u32);
    }
}

mod server_messages {
    use super::*;

    #[test]
    fn test_set_wait_port_request() {
        let req = ServerRequest::SetWaitPort {
            port: 2234,
            obfuscation_type: None,
            obfuscated_port: None,
        };
        let mut buf = BytesMut::new();
        req.write_message(&mut buf);
        assert!(buf.len() > 8);
    }

    #[test]
    fn test_get_peer_address_request() {
        let req = ServerRequest::GetPeerAddress {
            username: "testuser".to_string(),
        };
        let mut buf = BytesMut::new();
        req.write_message(&mut buf);
        assert!(buf.len() > 8);
    }

    #[test]
    fn test_file_search_request() {
        let req = ServerRequest::FileSearch {
            token: 12345,
            query: "pink floyd mp3".to_string(),
        };
        let mut buf = BytesMut::new();
        req.write_message(&mut buf);
        assert!(buf.len() > 8);
    }

    #[test]
    fn test_join_room_request() {
        let req = ServerRequest::JoinRoom {
            room: "Music".to_string(),
            private: false,
        };
        let mut buf = BytesMut::new();
        req.write_message(&mut buf);
        assert!(buf.len() > 8);
    }

    #[test]
    fn test_message_user_request() {
        let req = ServerRequest::MessageUser {
            username: "friend".to_string(),
            message: "Hello there!".to_string(),
        };
        let mut buf = BytesMut::new();
        req.write_message(&mut buf);
        assert!(buf.len() > 8);
    }

    #[test]
    fn test_user_stats_roundtrip() {
        let stats = UserStats {
            avg_speed: 100000,
            upload_num: 50,
            unknown: 0,
            files: 1000,
            dirs: 100,
        };
        let mut buf = BytesMut::new();
        stats.write_to(&mut buf);
        let parsed = UserStats::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.avg_speed, 100000);
        assert_eq!(parsed.files, 1000);
        assert_eq!(parsed.dirs, 100);
    }

    #[test]
    fn test_server_code_conversions() {
        assert_eq!(ServerCode::Login as u32, 1);
        assert_eq!(ServerCode::FileSearch as u32, 26);
        assert_eq!(ServerCode::JoinRoom as u32, 14);

        assert_eq!(ServerCode::try_from(1).unwrap(), ServerCode::Login);
        assert_eq!(ServerCode::try_from(26).unwrap(), ServerCode::FileSearch);
        assert!(ServerCode::try_from(9999).is_err());
    }
}

mod peer_messages {
    use super::*;

    #[test]
    fn test_queue_upload_roundtrip() {
        let msg = PeerMessage::QueueUpload {
            filename: "Music/Artist/Album/track01.mp3".to_string(),
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();

        if let PeerMessage::QueueUpload { filename } = parsed {
            assert_eq!(filename, "Music/Artist/Album/track01.mp3");
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_transfer_request_download_roundtrip() {
        let msg = PeerMessage::TransferRequest {
            direction: TransferDirection::Download,
            token: 99999,
            filename: "test.mp3".to_string(),
            file_size: None,
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();

        if let PeerMessage::TransferRequest {
            direction,
            token,
            filename,
            file_size,
        } = parsed
        {
            assert_eq!(direction, TransferDirection::Download);
            assert_eq!(token, 99999);
            assert_eq!(filename, "test.mp3");
            assert!(file_size.is_none());
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_transfer_request_upload_roundtrip() {
        let msg = PeerMessage::TransferRequest {
            direction: TransferDirection::Upload,
            token: 12345,
            filename: "song.flac".to_string(),
            file_size: Some(50_000_000),
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();

        if let PeerMessage::TransferRequest {
            direction,
            token,
            filename,
            file_size,
        } = parsed
        {
            assert_eq!(direction, TransferDirection::Upload);
            assert_eq!(token, 12345);
            assert_eq!(filename, "song.flac");
            assert_eq!(file_size, Some(50_000_000));
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_place_in_queue_response_roundtrip() {
        let msg = PeerMessage::PlaceInQueueResponse {
            filename: "track.mp3".to_string(),
            place: 5,
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();

        if let PeerMessage::PlaceInQueueResponse { filename, place } = parsed {
            assert_eq!(filename, "track.mp3");
            assert_eq!(place, 5);
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_upload_failed_roundtrip() {
        let msg = PeerMessage::UploadFailed {
            filename: "missing.mp3".to_string(),
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();

        if let PeerMessage::UploadFailed { filename } = parsed {
            assert_eq!(filename, "missing.mp3");
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_user_info_request_roundtrip() {
        let msg = PeerMessage::UserInfoRequest;
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();
        assert!(matches!(parsed, PeerMessage::UserInfoRequest));
    }

    #[test]
    fn test_shared_file_list_request_roundtrip() {
        let msg = PeerMessage::SharedFileListRequest;
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();
        assert!(matches!(parsed, PeerMessage::SharedFileListRequest));
    }

    #[test]
    fn test_folder_contents_request_roundtrip() {
        let msg = PeerMessage::FolderContentsRequest {
            token: 42,
            folder: "Music/Jazz".to_string(),
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);
        let parsed = read_peer_message(&mut buf.freeze()).unwrap();

        if let PeerMessage::FolderContentsRequest { token, folder } = parsed {
            assert_eq!(token, 42);
            assert_eq!(folder, "Music/Jazz");
        } else {
            panic!("Wrong message type");
        }
    }
}

mod peer_init_messages {
    use super::*;

    #[test]
    fn test_pierce_firewall_roundtrip() {
        let msg = PeerInitMessage::PierceFirewall { token: 0xCAFEBABE };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);
        let parsed = read_peer_init_message(&mut buf.freeze()).unwrap();

        if let PeerInitMessage::PierceFirewall { token } = parsed {
            assert_eq!(token, 0xCAFEBABE);
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_peer_init_p_connection() {
        let msg = PeerInitMessage::PeerInit {
            username: "testuser".to_string(),
            connection_type: ConnectionType::Peer,
            token: 12345,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);
        let parsed = read_peer_init_message(&mut buf.freeze()).unwrap();

        if let PeerInitMessage::PeerInit {
            username,
            connection_type,
            token,
        } = parsed
        {
            assert_eq!(username, "testuser");
            assert_eq!(connection_type, ConnectionType::Peer);
            assert_eq!(token, 12345);
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_peer_init_f_connection() {
        let msg = PeerInitMessage::PeerInit {
            username: "uploader".to_string(),
            connection_type: ConnectionType::File,
            token: 99999,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);
        let parsed = read_peer_init_message(&mut buf.freeze()).unwrap();

        if let PeerInitMessage::PeerInit {
            username,
            connection_type,
            token,
        } = parsed
        {
            assert_eq!(username, "uploader");
            assert_eq!(connection_type, ConnectionType::File);
            assert_eq!(token, 99999);
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_peer_init_d_connection() {
        let msg = PeerInitMessage::PeerInit {
            username: "parent".to_string(),
            connection_type: ConnectionType::Distributed,
            token: 0,
        };
        let mut buf = BytesMut::new();
        write_peer_init_message(&msg, &mut buf);
        let parsed = read_peer_init_message(&mut buf.freeze()).unwrap();

        if let PeerInitMessage::PeerInit {
            username,
            connection_type,
            ..
        } = parsed
        {
            assert_eq!(username, "parent");
            assert_eq!(connection_type, ConnectionType::Distributed);
        } else {
            panic!("Wrong message type");
        }
    }
}

mod distributed_messages {
    use super::*;

    #[test]
    fn test_ping_roundtrip() {
        let msg = DistributedMessage::Ping;
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);
        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();
        assert!(matches!(parsed, DistributedMessage::Ping));
    }

    #[test]
    fn test_search_roundtrip() {
        let msg = DistributedMessage::Search {
            unknown: 0,
            username: "searcher".to_string(),
            token: 54321,
            query: "beatles mp3".to_string(),
        };
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);
        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();

        if let DistributedMessage::Search {
            unknown,
            username,
            token,
            query,
        } = parsed
        {
            assert_eq!(unknown, 0);
            assert_eq!(username, "searcher");
            assert_eq!(token, 54321);
            assert_eq!(query, "beatles mp3");
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_branch_level_roundtrip() {
        let msg = DistributedMessage::BranchLevel { level: -1 };
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);
        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();

        if let DistributedMessage::BranchLevel { level } = parsed {
            assert_eq!(level, -1);
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_branch_root_roundtrip() {
        let msg = DistributedMessage::BranchRoot {
            root: "rootuser".to_string(),
        };
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);
        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();

        if let DistributedMessage::BranchRoot { root } = parsed {
            assert_eq!(root, "rootuser");
        } else {
            panic!("Wrong message type");
        }
    }

    #[test]
    fn test_child_depth_roundtrip() {
        let msg = DistributedMessage::ChildDepth { depth: 3 };
        let mut buf = BytesMut::new();
        write_distributed_message(&msg, &mut buf);
        let parsed = read_distributed_message(&mut buf.freeze()).unwrap();

        if let DistributedMessage::ChildDepth { depth } = parsed {
            assert_eq!(depth, 3);
        } else {
            panic!("Wrong message type");
        }
    }
}

mod file_messages {
    use super::*;

    #[test]
    fn test_file_transfer_init_roundtrip() {
        let init = FileTransferInit::new(0xDEADBEEF);
        let mut buf = BytesMut::new();
        init.write_to(&mut buf);
        let parsed = FileTransferInit::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.token, 0xDEADBEEF);
    }

    #[test]
    fn test_file_offset_roundtrip() {
        let offset = FileOffset::new(1024 * 1024 * 1024 * 5); // 5GB
        let mut buf = BytesMut::new();
        offset.write_to(&mut buf);
        let parsed = FileOffset::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.offset, 1024 * 1024 * 1024 * 5);
    }

    #[test]
    fn test_file_offset_zero() {
        let offset = FileOffset::new(0);
        let mut buf = BytesMut::new();
        offset.write_to(&mut buf);
        let parsed = FileOffset::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.offset, 0);
    }
}

mod constants {
    use super::*;
    use slsk_rs::constants::{
        FileAttributeType, LoginRejectionReason, ObfuscationType, TransferRejectionReason,
        UploadPermission,
    };

    #[test]
    fn test_connection_type_strings() {
        assert_eq!(ConnectionType::Peer.as_str(), "P");
        assert_eq!(ConnectionType::File.as_str(), "F");
        assert_eq!(ConnectionType::Distributed.as_str(), "D");

        assert_eq!(ConnectionType::parse("P").unwrap(), ConnectionType::Peer);
        assert_eq!(ConnectionType::parse("F").unwrap(), ConnectionType::File);
        assert_eq!(
            ConnectionType::parse("D").unwrap(),
            ConnectionType::Distributed
        );
        assert!(ConnectionType::parse("X").is_err());
    }

    #[test]
    fn test_user_status_conversions() {
        assert_eq!(UserStatus::try_from(0).unwrap(), UserStatus::Offline);
        assert_eq!(UserStatus::try_from(1).unwrap(), UserStatus::Away);
        assert_eq!(UserStatus::try_from(2).unwrap(), UserStatus::Online);
        assert!(UserStatus::try_from(99).is_err());

        assert_eq!(u32::from(UserStatus::Offline), 0);
        assert_eq!(u32::from(UserStatus::Online), 2);
    }

    #[test]
    fn test_upload_permission_conversions() {
        assert_eq!(
            UploadPermission::try_from(0).unwrap(),
            UploadPermission::NoOne
        );
        assert_eq!(
            UploadPermission::try_from(1).unwrap(),
            UploadPermission::Everyone
        );
        assert_eq!(
            UploadPermission::try_from(2).unwrap(),
            UploadPermission::UsersInList
        );
        assert_eq!(
            UploadPermission::try_from(3).unwrap(),
            UploadPermission::PermittedUsers
        );
        assert!(UploadPermission::try_from(99).is_err());
    }

    #[test]
    fn test_transfer_direction_conversions() {
        assert_eq!(
            TransferDirection::try_from(0).unwrap(),
            TransferDirection::Download
        );
        assert_eq!(
            TransferDirection::try_from(1).unwrap(),
            TransferDirection::Upload
        );
        assert!(TransferDirection::try_from(99).is_err());

        assert_eq!(u32::from(TransferDirection::Download), 0);
        assert_eq!(u32::from(TransferDirection::Upload), 1);
    }

    #[test]
    fn test_file_attribute_type_conversions() {
        assert_eq!(
            FileAttributeType::try_from(0).unwrap(),
            FileAttributeType::Bitrate
        );
        assert_eq!(
            FileAttributeType::try_from(1).unwrap(),
            FileAttributeType::Duration
        );
        assert_eq!(
            FileAttributeType::try_from(4).unwrap(),
            FileAttributeType::SampleRate
        );
        assert!(FileAttributeType::try_from(99).is_err());
    }

    #[test]
    fn test_obfuscation_type_conversions() {
        assert_eq!(
            ObfuscationType::try_from(0).unwrap(),
            ObfuscationType::None
        );
        assert_eq!(
            ObfuscationType::try_from(1).unwrap(),
            ObfuscationType::Rotated
        );
        assert!(ObfuscationType::try_from(99).is_err());
    }

    #[test]
    fn test_transfer_rejection_reason() {
        assert_eq!(TransferRejectionReason::Banned.as_str(), "Banned");
        assert_eq!(
            TransferRejectionReason::FileNotShared.as_str(),
            "File not shared."
        );

        assert_eq!(
            TransferRejectionReason::from_string("Banned".to_string()),
            TransferRejectionReason::Banned
        );
        assert_eq!(
            TransferRejectionReason::from_string("Custom reason".to_string()),
            TransferRejectionReason::Other("Custom reason".to_string())
        );
    }

    #[test]
    fn test_login_rejection_reason() {
        assert_eq!(
            LoginRejectionReason::from_string("INVALIDUSERNAME".to_string()),
            LoginRejectionReason::InvalidUsername
        );
        assert_eq!(
            LoginRejectionReason::from_string("INVALIDPASS".to_string()),
            LoginRejectionReason::InvalidPassword
        );
        assert_eq!(
            LoginRejectionReason::from_string("UNKNOWN".to_string()),
            LoginRejectionReason::Other("UNKNOWN".to_string())
        );
    }
}

mod complex_structures {
    use super::*;

    #[test]
    fn test_file_attribute() {
        let attr = FileAttribute {
            code: 0,
            value: 320,
        };
        let mut buf = BytesMut::new();
        attr.write_to(&mut buf);
        let parsed = FileAttribute::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.code, 0);
        assert_eq!(parsed.value, 320);
    }

    #[test]
    fn test_shared_file() {
        let file = SharedFile {
            filename: "track.mp3".to_string(),
            size: 5_000_000,
            extension: "mp3".to_string(),
            attributes: vec![
                FileAttribute { code: 0, value: 320 },
                FileAttribute { code: 1, value: 180 },
            ],
        };
        let mut buf = BytesMut::new();
        file.write_to(&mut buf);
        let parsed = SharedFile::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.filename, "track.mp3");
        assert_eq!(parsed.size, 5_000_000);
        assert_eq!(parsed.attributes.len(), 2);
    }

    #[test]
    fn test_shared_directory() {
        let dir = SharedDirectory {
            path: "Music/Jazz".to_string(),
            files: vec![
                SharedFile {
                    filename: "song1.mp3".to_string(),
                    size: 1000,
                    extension: "mp3".to_string(),
                    attributes: vec![],
                },
                SharedFile {
                    filename: "song2.flac".to_string(),
                    size: 50000000,
                    extension: "flac".to_string(),
                    attributes: vec![FileAttribute {
                        code: 0,
                        value: 1411,
                    }],
                },
            ],
        };
        let mut buf = BytesMut::new();
        dir.write_to(&mut buf);
        let parsed = SharedDirectory::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.path, "Music/Jazz");
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(parsed.files[1].extension, "flac");
    }

    #[test]
    fn test_search_result_file() {
        let file = SearchResultFile {
            filename: "found.mp3".to_string(),
            size: 10_000_000,
            extension: "mp3".to_string(),
            attributes: vec![
                FileAttribute { code: 0, value: 256 },
                FileAttribute { code: 1, value: 300 },
            ],
        };
        let mut buf = BytesMut::new();
        file.write_to(&mut buf);
        let parsed = SearchResultFile::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.filename, "found.mp3");
        assert_eq!(parsed.size, 10_000_000);
        assert_eq!(parsed.attributes.len(), 2);
    }
}
