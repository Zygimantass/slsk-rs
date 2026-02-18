//! Server message types.
//!
//! Server messages are used by clients to interface with the Soulseek server.

use bytes::{Buf, BufMut};
use std::net::Ipv4Addr;

use crate::constants::{ConnectionType, LoginRejectionReason, ObfuscationType, UserStatus};
use crate::protocol::{
    MessageRead, MessageWrite, ProtocolRead, ProtocolWrite, login_hash, read_list, write_list,
};
use crate::{Error, Result};

/// Server message codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ServerCode {
    Login = 1,
    SetWaitPort = 2,
    GetPeerAddress = 3,
    WatchUser = 5,
    UnwatchUser = 6,
    GetUserStatus = 7,
    SayChatroom = 13,
    JoinRoom = 14,
    LeaveRoom = 15,
    UserJoinedRoom = 16,
    UserLeftRoom = 17,
    ConnectToPeer = 18,
    MessageUser = 22,
    MessageAcked = 23,
    FileSearch = 26,
    SetStatus = 28,
    ServerPing = 32,
    SharedFoldersFiles = 35,
    GetUserStats = 36,
    Relogged = 41,
    UserSearch = 42,
    InterestAdd = 51,
    InterestRemove = 52,
    GetRecommendations = 54,
    GetGlobalRecommendations = 56,
    GetUserInterests = 57,
    RoomList = 64,
    AdminMessage = 66,
    PrivilegedUsers = 69,
    HaveNoParent = 71,
    ParentMinSpeed = 83,
    ParentSpeedRatio = 84,
    CheckPrivileges = 92,
    EmbeddedMessage = 93,
    AcceptChildren = 100,
    PossibleParents = 102,
    WishlistSearch = 103,
    WishlistInterval = 104,
    GetSimilarUsers = 110,
    GetItemRecommendations = 111,
    GetItemSimilarUsers = 112,
    RoomTickerState = 113,
    RoomTickerAdd = 114,
    RoomTickerRemove = 115,
    RoomTickerSet = 116,
    HatedInterestAdd = 117,
    HatedInterestRemove = 118,
    RoomSearch = 120,
    SendUploadSpeed = 121,
    GivePrivileges = 123,
    BranchLevel = 126,
    BranchRoot = 127,
    ResetDistributed = 130,
    RoomMembers = 133,
    AddRoomMember = 134,
    RemoveRoomMember = 135,
    CancelRoomMembership = 136,
    CancelRoomOwnership = 137,
    RoomMembershipGranted = 139,
    RoomMembershipRevoked = 140,
    EnableRoomInvitations = 141,
    ChangePassword = 142,
    AddRoomOperator = 143,
    RemoveRoomOperator = 144,
    RoomOperatorshipGranted = 145,
    RoomOperatorshipRevoked = 146,
    RoomOperators = 148,
    MessageUsers = 149,
    JoinGlobalRoom = 150,
    LeaveGlobalRoom = 151,
    GlobalRoomMessage = 152,
    ExcludedSearchPhrases = 160,
    CantConnectToPeer = 1001,
    CantCreateRoom = 1003,
}

impl TryFrom<u32> for ServerCode {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            1 => Ok(ServerCode::Login),
            2 => Ok(ServerCode::SetWaitPort),
            3 => Ok(ServerCode::GetPeerAddress),
            5 => Ok(ServerCode::WatchUser),
            6 => Ok(ServerCode::UnwatchUser),
            7 => Ok(ServerCode::GetUserStatus),
            13 => Ok(ServerCode::SayChatroom),
            14 => Ok(ServerCode::JoinRoom),
            15 => Ok(ServerCode::LeaveRoom),
            16 => Ok(ServerCode::UserJoinedRoom),
            17 => Ok(ServerCode::UserLeftRoom),
            18 => Ok(ServerCode::ConnectToPeer),
            22 => Ok(ServerCode::MessageUser),
            23 => Ok(ServerCode::MessageAcked),
            26 => Ok(ServerCode::FileSearch),
            28 => Ok(ServerCode::SetStatus),
            32 => Ok(ServerCode::ServerPing),
            35 => Ok(ServerCode::SharedFoldersFiles),
            36 => Ok(ServerCode::GetUserStats),
            41 => Ok(ServerCode::Relogged),
            42 => Ok(ServerCode::UserSearch),
            51 => Ok(ServerCode::InterestAdd),
            52 => Ok(ServerCode::InterestRemove),
            54 => Ok(ServerCode::GetRecommendations),
            56 => Ok(ServerCode::GetGlobalRecommendations),
            57 => Ok(ServerCode::GetUserInterests),
            64 => Ok(ServerCode::RoomList),
            66 => Ok(ServerCode::AdminMessage),
            69 => Ok(ServerCode::PrivilegedUsers),
            71 => Ok(ServerCode::HaveNoParent),
            83 => Ok(ServerCode::ParentMinSpeed),
            84 => Ok(ServerCode::ParentSpeedRatio),
            92 => Ok(ServerCode::CheckPrivileges),
            93 => Ok(ServerCode::EmbeddedMessage),
            100 => Ok(ServerCode::AcceptChildren),
            102 => Ok(ServerCode::PossibleParents),
            103 => Ok(ServerCode::WishlistSearch),
            104 => Ok(ServerCode::WishlistInterval),
            110 => Ok(ServerCode::GetSimilarUsers),
            111 => Ok(ServerCode::GetItemRecommendations),
            112 => Ok(ServerCode::GetItemSimilarUsers),
            113 => Ok(ServerCode::RoomTickerState),
            114 => Ok(ServerCode::RoomTickerAdd),
            115 => Ok(ServerCode::RoomTickerRemove),
            116 => Ok(ServerCode::RoomTickerSet),
            117 => Ok(ServerCode::HatedInterestAdd),
            118 => Ok(ServerCode::HatedInterestRemove),
            120 => Ok(ServerCode::RoomSearch),
            121 => Ok(ServerCode::SendUploadSpeed),
            123 => Ok(ServerCode::GivePrivileges),
            126 => Ok(ServerCode::BranchLevel),
            127 => Ok(ServerCode::BranchRoot),
            130 => Ok(ServerCode::ResetDistributed),
            133 => Ok(ServerCode::RoomMembers),
            134 => Ok(ServerCode::AddRoomMember),
            135 => Ok(ServerCode::RemoveRoomMember),
            136 => Ok(ServerCode::CancelRoomMembership),
            137 => Ok(ServerCode::CancelRoomOwnership),
            139 => Ok(ServerCode::RoomMembershipGranted),
            140 => Ok(ServerCode::RoomMembershipRevoked),
            141 => Ok(ServerCode::EnableRoomInvitations),
            142 => Ok(ServerCode::ChangePassword),
            143 => Ok(ServerCode::AddRoomOperator),
            144 => Ok(ServerCode::RemoveRoomOperator),
            145 => Ok(ServerCode::RoomOperatorshipGranted),
            146 => Ok(ServerCode::RoomOperatorshipRevoked),
            148 => Ok(ServerCode::RoomOperators),
            149 => Ok(ServerCode::MessageUsers),
            150 => Ok(ServerCode::JoinGlobalRoom),
            151 => Ok(ServerCode::LeaveGlobalRoom),
            152 => Ok(ServerCode::GlobalRoomMessage),
            160 => Ok(ServerCode::ExcludedSearchPhrases),
            1001 => Ok(ServerCode::CantConnectToPeer),
            1003 => Ok(ServerCode::CantCreateRoom),
            _ => Err(Error::InvalidMessageCode(value)),
        }
    }
}

impl From<ServerCode> for u32 {
    fn from(code: ServerCode) -> Self {
        code as u32
    }
}

/// User statistics.
#[derive(Debug, Clone, Default)]
pub struct UserStats {
    pub avg_speed: u32,
    pub upload_num: u32,
    pub unknown: u32,
    pub files: u32,
    pub dirs: u32,
}

impl UserStats {
    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        Ok(UserStats {
            avg_speed: u32::read_from(buf)?,
            upload_num: u32::read_from(buf)?,
            unknown: u32::read_from(buf)?,
            files: u32::read_from(buf)?,
            dirs: u32::read_from(buf)?,
        })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        self.avg_speed.write_to(buf);
        self.upload_num.write_to(buf);
        self.unknown.write_to(buf);
        self.files.write_to(buf);
        self.dirs.write_to(buf);
    }
}

/// Room user info.
#[derive(Debug, Clone)]
pub struct RoomUser {
    pub username: String,
    pub status: UserStatus,
    pub stats: UserStats,
    pub slots_full: bool,
    pub country_code: String,
}

/// A possible parent for the distributed network.
#[derive(Debug, Clone)]
pub struct PossibleParent {
    pub username: String,
    pub ip: Ipv4Addr,
    pub port: u32,
}

/// Room ticker entry.
#[derive(Debug, Clone)]
pub struct RoomTicker {
    pub username: String,
    pub ticker: String,
}

/// Messages that can be sent to the server.
#[derive(Debug, Clone)]
pub enum ServerRequest {
    /// Login to the server.
    Login {
        username: String,
        password: String,
        version: u32,
        minor_version: u32,
    },
    /// Set the port we're listening on.
    SetWaitPort {
        port: u32,
        obfuscation_type: Option<ObfuscationType>,
        obfuscated_port: Option<u32>,
    },
    /// Get a peer's address.
    GetPeerAddress { username: String },
    /// Watch a user for status updates.
    WatchUser { username: String },
    /// Stop watching a user.
    UnwatchUser { username: String },
    /// Get a user's status.
    GetUserStatus { username: String },
    /// Say something in a chat room.
    SayChatroom { room: String, message: String },
    /// Join a room.
    JoinRoom { room: String, private: bool },
    /// Leave a room.
    LeaveRoom { room: String },
    /// Initiate indirect peer connection.
    ConnectToPeer {
        token: u32,
        username: String,
        connection_type: ConnectionType,
    },
    /// Send a private message.
    MessageUser { username: String, message: String },
    /// Acknowledge receipt of a private message.
    MessageAcked { message_id: u32 },
    /// Perform a file search.
    FileSearch { token: u32, query: String },
    /// Set our online status.
    SetStatus { status: UserStatus },
    /// Ping the server.
    ServerPing,
    /// Report shared folders and files count.
    SharedFoldersFiles { dirs: u32, files: u32 },
    /// Get a user's stats.
    GetUserStats { username: String },
    /// Search a specific user's files.
    UserSearch {
        username: String,
        token: u32,
        query: String,
    },
    /// Add an interest.
    InterestAdd { item: String },
    /// Remove an interest.
    InterestRemove { item: String },
    /// Get personal recommendations.
    GetRecommendations,
    /// Get global recommendations.
    GetGlobalRecommendations,
    /// Get a user's interests.
    GetUserInterests { username: String },
    /// Get the room list.
    RoomList,
    /// Report no parent in distributed network.
    HaveNoParent { no_parent: bool },
    /// Check our privileges.
    CheckPrivileges,
    /// Accept child connections in distributed network.
    AcceptChildren { accept: bool },
    /// Wishlist search.
    WishlistSearch { token: u32, query: String },
    /// Get similar users.
    GetSimilarUsers,
    /// Get item recommendations.
    GetItemRecommendations { item: String },
    /// Get similar users for an item.
    GetItemSimilarUsers { item: String },
    /// Set a room ticker.
    RoomTickerSet { room: String, ticker: String },
    /// Add a hated interest.
    HatedInterestAdd { item: String },
    /// Remove a hated interest.
    HatedInterestRemove { item: String },
    /// Search in a room.
    RoomSearch {
        room: String,
        token: u32,
        query: String,
    },
    /// Report upload speed.
    SendUploadSpeed { speed: u32 },
    /// Give privileges to another user.
    GivePrivileges { username: String, days: u32 },
    /// Report branch level.
    BranchLevel { level: u32 },
    /// Report branch root.
    BranchRoot { root: String },
    /// Add a member to a private room.
    AddRoomMember { room: String, username: String },
    /// Remove a member from a private room.
    RemoveRoomMember { room: String, username: String },
    /// Cancel our membership of a private room.
    CancelRoomMembership { room: String },
    /// Cancel ownership of a private room.
    CancelRoomOwnership { room: String },
    /// Enable/disable room invitations.
    EnableRoomInvitations { enable: bool },
    /// Change password.
    ChangePassword { password: String },
    /// Add a room operator.
    AddRoomOperator { room: String, username: String },
    /// Remove a room operator.
    RemoveRoomOperator { room: String, username: String },
    /// Send message to multiple users.
    MessageUsers {
        usernames: Vec<String>,
        message: String,
    },
    /// Join global room feed.
    JoinGlobalRoom,
    /// Leave global room feed.
    LeaveGlobalRoom,
    /// Report we can't connect to a peer.
    CantConnectToPeer { token: u32, username: String },
}

impl MessageWrite for ServerRequest {
    type Code = ServerCode;

    fn code(&self) -> ServerCode {
        match self {
            ServerRequest::Login { .. } => ServerCode::Login,
            ServerRequest::SetWaitPort { .. } => ServerCode::SetWaitPort,
            ServerRequest::GetPeerAddress { .. } => ServerCode::GetPeerAddress,
            ServerRequest::WatchUser { .. } => ServerCode::WatchUser,
            ServerRequest::UnwatchUser { .. } => ServerCode::UnwatchUser,
            ServerRequest::GetUserStatus { .. } => ServerCode::GetUserStatus,
            ServerRequest::SayChatroom { .. } => ServerCode::SayChatroom,
            ServerRequest::JoinRoom { .. } => ServerCode::JoinRoom,
            ServerRequest::LeaveRoom { .. } => ServerCode::LeaveRoom,
            ServerRequest::ConnectToPeer { .. } => ServerCode::ConnectToPeer,
            ServerRequest::MessageUser { .. } => ServerCode::MessageUser,
            ServerRequest::MessageAcked { .. } => ServerCode::MessageAcked,
            ServerRequest::FileSearch { .. } => ServerCode::FileSearch,
            ServerRequest::SetStatus { .. } => ServerCode::SetStatus,
            ServerRequest::ServerPing => ServerCode::ServerPing,
            ServerRequest::SharedFoldersFiles { .. } => ServerCode::SharedFoldersFiles,
            ServerRequest::GetUserStats { .. } => ServerCode::GetUserStats,
            ServerRequest::UserSearch { .. } => ServerCode::UserSearch,
            ServerRequest::InterestAdd { .. } => ServerCode::InterestAdd,
            ServerRequest::InterestRemove { .. } => ServerCode::InterestRemove,
            ServerRequest::GetRecommendations => ServerCode::GetRecommendations,
            ServerRequest::GetGlobalRecommendations => ServerCode::GetGlobalRecommendations,
            ServerRequest::GetUserInterests { .. } => ServerCode::GetUserInterests,
            ServerRequest::RoomList => ServerCode::RoomList,
            ServerRequest::HaveNoParent { .. } => ServerCode::HaveNoParent,
            ServerRequest::CheckPrivileges => ServerCode::CheckPrivileges,
            ServerRequest::AcceptChildren { .. } => ServerCode::AcceptChildren,
            ServerRequest::WishlistSearch { .. } => ServerCode::WishlistSearch,
            ServerRequest::GetSimilarUsers => ServerCode::GetSimilarUsers,
            ServerRequest::GetItemRecommendations { .. } => ServerCode::GetItemRecommendations,
            ServerRequest::GetItemSimilarUsers { .. } => ServerCode::GetItemSimilarUsers,
            ServerRequest::RoomTickerSet { .. } => ServerCode::RoomTickerSet,
            ServerRequest::HatedInterestAdd { .. } => ServerCode::HatedInterestAdd,
            ServerRequest::HatedInterestRemove { .. } => ServerCode::HatedInterestRemove,
            ServerRequest::RoomSearch { .. } => ServerCode::RoomSearch,
            ServerRequest::SendUploadSpeed { .. } => ServerCode::SendUploadSpeed,
            ServerRequest::GivePrivileges { .. } => ServerCode::GivePrivileges,
            ServerRequest::BranchLevel { .. } => ServerCode::BranchLevel,
            ServerRequest::BranchRoot { .. } => ServerCode::BranchRoot,
            ServerRequest::AddRoomMember { .. } => ServerCode::AddRoomMember,
            ServerRequest::RemoveRoomMember { .. } => ServerCode::RemoveRoomMember,
            ServerRequest::CancelRoomMembership { .. } => ServerCode::CancelRoomMembership,
            ServerRequest::CancelRoomOwnership { .. } => ServerCode::CancelRoomOwnership,
            ServerRequest::EnableRoomInvitations { .. } => ServerCode::EnableRoomInvitations,
            ServerRequest::ChangePassword { .. } => ServerCode::ChangePassword,
            ServerRequest::AddRoomOperator { .. } => ServerCode::AddRoomOperator,
            ServerRequest::RemoveRoomOperator { .. } => ServerCode::RemoveRoomOperator,
            ServerRequest::MessageUsers { .. } => ServerCode::MessageUsers,
            ServerRequest::JoinGlobalRoom => ServerCode::JoinGlobalRoom,
            ServerRequest::LeaveGlobalRoom => ServerCode::LeaveGlobalRoom,
            ServerRequest::CantConnectToPeer { .. } => ServerCode::CantConnectToPeer,
        }
    }

    fn write_payload<B: BufMut>(&self, buf: &mut B) {
        match self {
            ServerRequest::Login {
                username,
                password,
                version,
                minor_version,
            } => {
                username.write_to(buf);
                password.write_to(buf);
                version.write_to(buf);
                login_hash(username, password).write_to(buf);
                minor_version.write_to(buf);
            }
            ServerRequest::SetWaitPort {
                port,
                obfuscation_type,
                obfuscated_port,
            } => {
                port.write_to(buf);
                if let (Some(obs_type), Some(obs_port)) = (obfuscation_type, obfuscated_port) {
                    (*obs_type as u32).write_to(buf);
                    obs_port.write_to(buf);
                }
            }
            ServerRequest::GetPeerAddress { username } => username.write_to(buf),
            ServerRequest::WatchUser { username } => username.write_to(buf),
            ServerRequest::UnwatchUser { username } => username.write_to(buf),
            ServerRequest::GetUserStatus { username } => username.write_to(buf),
            ServerRequest::SayChatroom { room, message } => {
                room.write_to(buf);
                message.write_to(buf);
            }
            ServerRequest::JoinRoom { room, private } => {
                room.write_to(buf);
                (*private as u32).write_to(buf);
            }
            ServerRequest::LeaveRoom { room } => room.write_to(buf),
            ServerRequest::ConnectToPeer {
                token,
                username,
                connection_type,
            } => {
                token.write_to(buf);
                username.write_to(buf);
                connection_type.as_str().write_to(buf);
            }
            ServerRequest::MessageUser { username, message } => {
                username.write_to(buf);
                message.write_to(buf);
            }
            ServerRequest::MessageAcked { message_id } => message_id.write_to(buf),
            ServerRequest::FileSearch { token, query } => {
                token.write_to(buf);
                query.write_to(buf);
            }
            ServerRequest::SetStatus { status } => {
                (*status as u32 as i32).write_to(buf);
            }
            ServerRequest::ServerPing => {}
            ServerRequest::SharedFoldersFiles { dirs, files } => {
                dirs.write_to(buf);
                files.write_to(buf);
            }
            ServerRequest::GetUserStats { username } => username.write_to(buf),
            ServerRequest::UserSearch {
                username,
                token,
                query,
            } => {
                username.write_to(buf);
                token.write_to(buf);
                query.write_to(buf);
            }
            ServerRequest::InterestAdd { item } => item.write_to(buf),
            ServerRequest::InterestRemove { item } => item.write_to(buf),
            ServerRequest::GetRecommendations => {}
            ServerRequest::GetGlobalRecommendations => {}
            ServerRequest::GetUserInterests { username } => username.write_to(buf),
            ServerRequest::RoomList => {}
            ServerRequest::HaveNoParent { no_parent } => no_parent.write_to(buf),
            ServerRequest::CheckPrivileges => {}
            ServerRequest::AcceptChildren { accept } => accept.write_to(buf),
            ServerRequest::WishlistSearch { token, query } => {
                token.write_to(buf);
                query.write_to(buf);
            }
            ServerRequest::GetSimilarUsers => {}
            ServerRequest::GetItemRecommendations { item } => item.write_to(buf),
            ServerRequest::GetItemSimilarUsers { item } => item.write_to(buf),
            ServerRequest::RoomTickerSet { room, ticker } => {
                room.write_to(buf);
                ticker.write_to(buf);
            }
            ServerRequest::HatedInterestAdd { item } => item.write_to(buf),
            ServerRequest::HatedInterestRemove { item } => item.write_to(buf),
            ServerRequest::RoomSearch { room, token, query } => {
                room.write_to(buf);
                token.write_to(buf);
                query.write_to(buf);
            }
            ServerRequest::SendUploadSpeed { speed } => speed.write_to(buf),
            ServerRequest::GivePrivileges { username, days } => {
                username.write_to(buf);
                days.write_to(buf);
            }
            ServerRequest::BranchLevel { level } => level.write_to(buf),
            ServerRequest::BranchRoot { root } => root.write_to(buf),
            ServerRequest::AddRoomMember { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerRequest::RemoveRoomMember { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerRequest::CancelRoomMembership { room } => room.write_to(buf),
            ServerRequest::CancelRoomOwnership { room } => room.write_to(buf),
            ServerRequest::EnableRoomInvitations { enable } => enable.write_to(buf),
            ServerRequest::ChangePassword { password } => password.write_to(buf),
            ServerRequest::AddRoomOperator { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerRequest::RemoveRoomOperator { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerRequest::MessageUsers { usernames, message } => {
                write_list(buf, usernames, |b, u| u.write_to(b));
                message.write_to(buf);
            }
            ServerRequest::JoinGlobalRoom => {}
            ServerRequest::LeaveGlobalRoom => {}
            ServerRequest::CantConnectToPeer { token, username } => {
                token.write_to(buf);
                username.write_to(buf);
            }
        }
    }
}

/// Messages received from the server.
#[derive(Debug, Clone)]
pub enum ServerResponse {
    /// Login response.
    LoginSuccess {
        greet: String,
        own_ip: Ipv4Addr,
        password_hash: String,
        is_supporter: bool,
    },
    LoginFailure {
        reason: LoginRejectionReason,
        detail: Option<String>,
    },
    /// Peer address response.
    GetPeerAddress {
        username: String,
        ip: Ipv4Addr,
        port: u32,
        obfuscation_type: ObfuscationType,
        obfuscated_port: u16,
    },
    /// Watch user response.
    WatchUser {
        username: String,
        exists: bool,
        status: Option<UserStatus>,
        stats: Option<UserStats>,
        country_code: Option<String>,
    },
    /// User status update.
    GetUserStatus {
        username: String,
        status: UserStatus,
        privileged: bool,
    },
    /// Chat room message.
    SayChatroom {
        room: String,
        username: String,
        message: String,
    },
    /// Join room response.
    JoinRoom {
        room: String,
        users: Vec<RoomUser>,
        owner: Option<String>,
        operators: Vec<String>,
    },
    /// Leave room response.
    LeaveRoom { room: String },
    /// User joined a room we're in.
    UserJoinedRoom {
        room: String,
        username: String,
        status: UserStatus,
        stats: UserStats,
        slots_full: bool,
        country_code: String,
    },
    /// User left a room we're in.
    UserLeftRoom { room: String, username: String },
    /// Indirect peer connection request.
    ConnectToPeer {
        username: String,
        connection_type: ConnectionType,
        ip: Ipv4Addr,
        port: u32,
        token: u32,
        privileged: bool,
        obfuscation_type: ObfuscationType,
        obfuscated_port: u32,
    },
    /// Private message received.
    MessageUser {
        id: u32,
        timestamp: u32,
        username: String,
        message: String,
        new_message: bool,
    },
    /// File search request from another user (via server).
    FileSearch {
        username: String,
        token: u32,
        query: String,
    },
    /// User stats update.
    GetUserStats { username: String, stats: UserStats },
    /// We were logged in elsewhere.
    Relogged,
    /// Recommendations response.
    Recommendations {
        recommendations: Vec<(String, i32)>,
        unrecommendations: Vec<(String, i32)>,
    },
    /// Global recommendations response.
    GlobalRecommendations {
        recommendations: Vec<(String, i32)>,
        unrecommendations: Vec<(String, i32)>,
    },
    /// User interests response.
    UserInterests {
        username: String,
        likes: Vec<String>,
        hates: Vec<String>,
    },
    /// Room list response.
    RoomList {
        rooms: Vec<(String, u32)>,
        owned_private_rooms: Vec<(String, u32)>,
        private_rooms: Vec<(String, u32)>,
        operated_private_rooms: Vec<String>,
    },
    /// Admin/global message.
    AdminMessage { message: String },
    /// List of privileged users.
    PrivilegedUsers { users: Vec<String> },
    /// Minimum upload speed to be a parent.
    ParentMinSpeed { speed: u32 },
    /// Speed ratio for determining number of children.
    ParentSpeedRatio { ratio: u32 },
    /// Privileges check response.
    CheckPrivileges { time_left: u32 },
    /// Embedded distributed message.
    EmbeddedMessage { code: u8, data: Vec<u8> },
    /// Possible parents for distributed network.
    PossibleParents { parents: Vec<PossibleParent> },
    /// Wishlist search interval.
    WishlistInterval { interval: u32 },
    /// Similar users response.
    SimilarUsers { users: Vec<(String, u32)> },
    /// Item recommendations response.
    ItemRecommendations {
        item: String,
        recommendations: Vec<(String, i32)>,
    },
    /// Item similar users response.
    ItemSimilarUsers { item: String, users: Vec<String> },
    /// Room ticker state.
    RoomTickerState {
        room: String,
        tickers: Vec<RoomTicker>,
    },
    /// Room ticker added.
    RoomTickerAdd {
        room: String,
        username: String,
        ticker: String,
    },
    /// Room ticker removed.
    RoomTickerRemove { room: String, username: String },
    /// Room invitations enabled/disabled.
    EnableRoomInvitations { enable: bool },
    /// Password changed.
    ChangePassword { password: String },
    /// Room operator added.
    AddRoomOperator { room: String, username: String },
    /// Room operator removed.
    RemoveRoomOperator { room: String, username: String },
    /// We were granted room operatorship.
    RoomOperatorshipGranted { room: String },
    /// Our room operatorship was revoked.
    RoomOperatorshipRevoked { room: String },
    /// List of room operators.
    RoomOperators {
        room: String,
        operators: Vec<String>,
    },
    /// Room membership granted.
    RoomMembershipGranted { room: String },
    /// Room membership revoked.
    RoomMembershipRevoked { room: String },
    /// Room members list.
    RoomMembers { room: String, members: Vec<String> },
    /// Room member added.
    AddRoomMember { room: String, username: String },
    /// Room member removed.
    RemoveRoomMember { room: String, username: String },
    /// Reset distributed network.
    ResetDistributed,
    /// Global room message.
    GlobalRoomMessage {
        room: String,
        username: String,
        message: String,
    },
    /// Excluded search phrases.
    ExcludedSearchPhrases { phrases: Vec<String> },
    /// Can't connect to peer.
    CantConnectToPeer { token: u32, username: String },
    /// Can't create room.
    CantCreateRoom { room: String },
}

impl MessageRead for ServerResponse {
    type Code = ServerCode;

    fn read_with_code<B: Buf>(code: ServerCode, buf: &mut B) -> Result<Self> {
        match code {
            ServerCode::Login => {
                let success = bool::read_from(buf)?;
                if success {
                    let greet = String::read_from(buf)?;
                    let own_ip = Ipv4Addr::read_from(buf)?;
                    let password_hash = String::read_from(buf)?;
                    let is_supporter = bool::read_from(buf)?;
                    Ok(ServerResponse::LoginSuccess {
                        greet,
                        own_ip,
                        password_hash,
                        is_supporter,
                    })
                } else {
                    let reason_str = String::read_from(buf)?;
                    let reason = LoginRejectionReason::from_string(reason_str.clone());
                    let detail = if matches!(reason, LoginRejectionReason::InvalidUsername)
                        && buf.has_remaining()
                    {
                        Some(String::read_from(buf)?)
                    } else {
                        None
                    };
                    Ok(ServerResponse::LoginFailure { reason, detail })
                }
            }
            ServerCode::GetPeerAddress => {
                let username = String::read_from(buf)?;
                let ip = Ipv4Addr::read_from(buf)?;
                let port = u32::read_from(buf)?;
                let obfuscation_type = ObfuscationType::try_from(u32::read_from(buf)?)?;
                let obfuscated_port = u16::read_from(buf)?;
                Ok(ServerResponse::GetPeerAddress {
                    username,
                    ip,
                    port,
                    obfuscation_type,
                    obfuscated_port,
                })
            }
            ServerCode::WatchUser => {
                let username = String::read_from(buf)?;
                let exists = bool::read_from(buf)?;
                if exists {
                    let status = UserStatus::try_from(u32::read_from(buf)?)?;
                    let stats = UserStats::read_from(buf)?;
                    let country_code = if status != UserStatus::Offline && buf.has_remaining() {
                        Some(String::read_from(buf)?)
                    } else {
                        None
                    };
                    Ok(ServerResponse::WatchUser {
                        username,
                        exists: true,
                        status: Some(status),
                        stats: Some(stats),
                        country_code,
                    })
                } else {
                    Ok(ServerResponse::WatchUser {
                        username,
                        exists: false,
                        status: None,
                        stats: None,
                        country_code: None,
                    })
                }
            }
            ServerCode::GetUserStatus => {
                let username = String::read_from(buf)?;
                let status = UserStatus::try_from(u32::read_from(buf)?)?;
                let privileged = bool::read_from(buf)?;
                Ok(ServerResponse::GetUserStatus {
                    username,
                    status,
                    privileged,
                })
            }
            ServerCode::SayChatroom => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                let message = String::read_from(buf)?;
                Ok(ServerResponse::SayChatroom {
                    room,
                    username,
                    message,
                })
            }
            ServerCode::JoinRoom => {
                let room = String::read_from(buf)?;
                let usernames: Vec<String> = read_list(buf, String::read_from)?;
                let statuses: Vec<u32> = read_list(buf, u32::read_from)?;
                let stats_list: Vec<UserStats> = read_list(buf, UserStats::read_from)?;
                let slots_full_list: Vec<u32> = read_list(buf, u32::read_from)?;
                let countries: Vec<String> = read_list(buf, String::read_from)?;

                let mut users = Vec::with_capacity(usernames.len());
                for (i, username) in usernames.into_iter().enumerate() {
                    users.push(RoomUser {
                        username,
                        status: UserStatus::try_from(statuses.get(i).copied().unwrap_or(0))?,
                        stats: stats_list.get(i).cloned().unwrap_or_default(),
                        slots_full: slots_full_list.get(i).copied().unwrap_or(0) != 0,
                        country_code: countries.get(i).cloned().unwrap_or_default(),
                    });
                }

                // Check for private room info
                let (owner, operators) = if buf.has_remaining() {
                    let owner = String::read_from(buf)?;
                    let operators = read_list(buf, String::read_from)?;
                    (Some(owner), operators)
                } else {
                    (None, vec![])
                };

                Ok(ServerResponse::JoinRoom {
                    room,
                    users,
                    owner,
                    operators,
                })
            }
            ServerCode::LeaveRoom => {
                let room = String::read_from(buf)?;
                Ok(ServerResponse::LeaveRoom { room })
            }
            ServerCode::UserJoinedRoom => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                let status = UserStatus::try_from(u32::read_from(buf)?)?;
                let stats = UserStats::read_from(buf)?;
                let slots_full = u32::read_from(buf)? != 0;
                let country_code = String::read_from(buf)?;
                Ok(ServerResponse::UserJoinedRoom {
                    room,
                    username,
                    status,
                    stats,
                    slots_full,
                    country_code,
                })
            }
            ServerCode::UserLeftRoom => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::UserLeftRoom { room, username })
            }
            ServerCode::ConnectToPeer => {
                let username = String::read_from(buf)?;
                let conn_type_str = String::read_from(buf)?;
                let connection_type = ConnectionType::parse(&conn_type_str)?;
                let ip = Ipv4Addr::read_from(buf)?;
                let port = u32::read_from(buf)?;
                let token = u32::read_from(buf)?;
                let privileged = bool::read_from(buf)?;
                let obfuscation_type = ObfuscationType::try_from(u32::read_from(buf)?)?;
                let obfuscated_port = u32::read_from(buf)?;
                Ok(ServerResponse::ConnectToPeer {
                    username,
                    connection_type,
                    ip,
                    port,
                    token,
                    privileged,
                    obfuscation_type,
                    obfuscated_port,
                })
            }
            ServerCode::MessageUser => {
                let id = u32::read_from(buf)?;
                let timestamp = u32::read_from(buf)?;
                let username = String::read_from(buf)?;
                let message = String::read_from(buf)?;
                let new_message = bool::read_from(buf)?;
                Ok(ServerResponse::MessageUser {
                    id,
                    timestamp,
                    username,
                    message,
                    new_message,
                })
            }
            ServerCode::FileSearch => {
                let username = String::read_from(buf)?;
                let token = u32::read_from(buf)?;
                let query = String::read_from(buf)?;
                Ok(ServerResponse::FileSearch {
                    username,
                    token,
                    query,
                })
            }
            ServerCode::GetUserStats => {
                let username = String::read_from(buf)?;
                let stats = UserStats::read_from(buf)?;
                Ok(ServerResponse::GetUserStats { username, stats })
            }
            ServerCode::Relogged => Ok(ServerResponse::Relogged),
            ServerCode::GetRecommendations => {
                let recommendations = read_list(buf, |b| {
                    let name = String::read_from(b)?;
                    let count = i32::read_from(b)?;
                    Ok((name, count))
                })?;
                let unrecommendations = read_list(buf, |b| {
                    let name = String::read_from(b)?;
                    let count = i32::read_from(b)?;
                    Ok((name, count))
                })?;
                Ok(ServerResponse::Recommendations {
                    recommendations,
                    unrecommendations,
                })
            }
            ServerCode::GetGlobalRecommendations => {
                let recommendations = read_list(buf, |b| {
                    let name = String::read_from(b)?;
                    let count = i32::read_from(b)?;
                    Ok((name, count))
                })?;
                let unrecommendations = read_list(buf, |b| {
                    let name = String::read_from(b)?;
                    let count = i32::read_from(b)?;
                    Ok((name, count))
                })?;
                Ok(ServerResponse::GlobalRecommendations {
                    recommendations,
                    unrecommendations,
                })
            }
            ServerCode::GetUserInterests => {
                let username = String::read_from(buf)?;
                let likes = read_list(buf, String::read_from)?;
                let hates = read_list(buf, String::read_from)?;
                Ok(ServerResponse::UserInterests {
                    username,
                    likes,
                    hates,
                })
            }
            ServerCode::RoomList => {
                let room_names: Vec<String> = read_list(buf, String::read_from)?;
                let room_counts: Vec<u32> = read_list(buf, u32::read_from)?;
                let rooms: Vec<_> = room_names.into_iter().zip(room_counts).collect();

                let owned_names: Vec<String> = read_list(buf, String::read_from)?;
                let owned_counts: Vec<u32> = read_list(buf, u32::read_from)?;
                let owned_private_rooms: Vec<_> =
                    owned_names.into_iter().zip(owned_counts).collect();

                let private_names: Vec<String> = read_list(buf, String::read_from)?;
                let private_counts: Vec<u32> = read_list(buf, u32::read_from)?;
                let private_rooms: Vec<_> = private_names.into_iter().zip(private_counts).collect();

                let operated_private_rooms = read_list(buf, String::read_from)?;

                Ok(ServerResponse::RoomList {
                    rooms,
                    owned_private_rooms,
                    private_rooms,
                    operated_private_rooms,
                })
            }
            ServerCode::AdminMessage => {
                let message = String::read_from(buf)?;
                Ok(ServerResponse::AdminMessage { message })
            }
            ServerCode::PrivilegedUsers => {
                let users = read_list(buf, String::read_from)?;
                Ok(ServerResponse::PrivilegedUsers { users })
            }
            ServerCode::ParentMinSpeed => {
                let speed = u32::read_from(buf)?;
                Ok(ServerResponse::ParentMinSpeed { speed })
            }
            ServerCode::ParentSpeedRatio => {
                let ratio = u32::read_from(buf)?;
                Ok(ServerResponse::ParentSpeedRatio { ratio })
            }
            ServerCode::CheckPrivileges => {
                let time_left = u32::read_from(buf)?;
                Ok(ServerResponse::CheckPrivileges { time_left })
            }
            ServerCode::EmbeddedMessage => {
                let code = u8::read_from(buf)?;
                let mut data = vec![0u8; buf.remaining()];
                buf.copy_to_slice(&mut data);
                Ok(ServerResponse::EmbeddedMessage { code, data })
            }
            ServerCode::PossibleParents => {
                let parents = read_list(buf, |b| {
                    let username = String::read_from(b)?;
                    let ip = Ipv4Addr::read_from(b)?;
                    let port = u32::read_from(b)?;
                    Ok(PossibleParent { username, ip, port })
                })?;
                Ok(ServerResponse::PossibleParents { parents })
            }
            ServerCode::WishlistInterval => {
                let interval = u32::read_from(buf)?;
                Ok(ServerResponse::WishlistInterval { interval })
            }
            ServerCode::GetSimilarUsers => {
                let users = read_list(buf, |b| {
                    let username = String::read_from(b)?;
                    let rating = u32::read_from(b)?;
                    Ok((username, rating))
                })?;
                Ok(ServerResponse::SimilarUsers { users })
            }
            ServerCode::GetItemRecommendations => {
                let item = String::read_from(buf)?;
                let recommendations = read_list(buf, |b| {
                    let name = String::read_from(b)?;
                    let count = i32::read_from(b)?;
                    Ok((name, count))
                })?;
                Ok(ServerResponse::ItemRecommendations {
                    item,
                    recommendations,
                })
            }
            ServerCode::GetItemSimilarUsers => {
                let item = String::read_from(buf)?;
                let users = read_list(buf, String::read_from)?;
                Ok(ServerResponse::ItemSimilarUsers { item, users })
            }
            ServerCode::RoomTickerState => {
                let room = String::read_from(buf)?;
                let tickers = read_list(buf, |b| {
                    let username = String::read_from(b)?;
                    let ticker = String::read_from(b)?;
                    Ok(RoomTicker { username, ticker })
                })?;
                Ok(ServerResponse::RoomTickerState { room, tickers })
            }
            ServerCode::RoomTickerAdd => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                let ticker = String::read_from(buf)?;
                Ok(ServerResponse::RoomTickerAdd {
                    room,
                    username,
                    ticker,
                })
            }
            ServerCode::RoomTickerRemove => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::RoomTickerRemove { room, username })
            }
            ServerCode::EnableRoomInvitations => {
                let enable = bool::read_from(buf)?;
                Ok(ServerResponse::EnableRoomInvitations { enable })
            }
            ServerCode::ChangePassword => {
                let password = String::read_from(buf)?;
                Ok(ServerResponse::ChangePassword { password })
            }
            ServerCode::AddRoomOperator => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::AddRoomOperator { room, username })
            }
            ServerCode::RemoveRoomOperator => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::RemoveRoomOperator { room, username })
            }
            ServerCode::RoomOperatorshipGranted => {
                let room = String::read_from(buf)?;
                Ok(ServerResponse::RoomOperatorshipGranted { room })
            }
            ServerCode::RoomOperatorshipRevoked => {
                let room = String::read_from(buf)?;
                Ok(ServerResponse::RoomOperatorshipRevoked { room })
            }
            ServerCode::RoomOperators => {
                let room = String::read_from(buf)?;
                let operators = read_list(buf, String::read_from)?;
                Ok(ServerResponse::RoomOperators { room, operators })
            }
            ServerCode::RoomMembershipGranted => {
                let room = String::read_from(buf)?;
                Ok(ServerResponse::RoomMembershipGranted { room })
            }
            ServerCode::RoomMembershipRevoked => {
                let room = String::read_from(buf)?;
                Ok(ServerResponse::RoomMembershipRevoked { room })
            }
            ServerCode::RoomMembers => {
                let room = String::read_from(buf)?;
                let members = read_list(buf, String::read_from)?;
                Ok(ServerResponse::RoomMembers { room, members })
            }
            ServerCode::AddRoomMember => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::AddRoomMember { room, username })
            }
            ServerCode::RemoveRoomMember => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::RemoveRoomMember { room, username })
            }
            ServerCode::ResetDistributed => Ok(ServerResponse::ResetDistributed),
            ServerCode::GlobalRoomMessage => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                let message = String::read_from(buf)?;
                Ok(ServerResponse::GlobalRoomMessage {
                    room,
                    username,
                    message,
                })
            }
            ServerCode::ExcludedSearchPhrases => {
                let phrases = read_list(buf, String::read_from)?;
                Ok(ServerResponse::ExcludedSearchPhrases { phrases })
            }
            ServerCode::CantConnectToPeer => {
                let token = u32::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerResponse::CantConnectToPeer { token, username })
            }
            ServerCode::CantCreateRoom => {
                let room = String::read_from(buf)?;
                Ok(ServerResponse::CantCreateRoom { room })
            }
            // Codes that are only for sending, not receiving
            ServerCode::SetWaitPort
            | ServerCode::UnwatchUser
            | ServerCode::SetStatus
            | ServerCode::ServerPing
            | ServerCode::SharedFoldersFiles
            | ServerCode::UserSearch
            | ServerCode::InterestAdd
            | ServerCode::InterestRemove
            | ServerCode::HaveNoParent
            | ServerCode::AcceptChildren
            | ServerCode::WishlistSearch
            | ServerCode::RoomTickerSet
            | ServerCode::HatedInterestAdd
            | ServerCode::HatedInterestRemove
            | ServerCode::RoomSearch
            | ServerCode::SendUploadSpeed
            | ServerCode::GivePrivileges
            | ServerCode::BranchLevel
            | ServerCode::BranchRoot
            | ServerCode::CancelRoomMembership
            | ServerCode::CancelRoomOwnership
            | ServerCode::MessageUsers
            | ServerCode::JoinGlobalRoom
            | ServerCode::LeaveGlobalRoom
            | ServerCode::MessageAcked => Err(Error::Protocol(format!(
                "Server code {:?} is send-only, not expected in response",
                code
            ))),
        }
    }
}

/// Read a server message from a buffer (including length prefix).
pub fn read_server_message<B: Buf>(buf: &mut B) -> Result<ServerResponse> {
    let _len = u32::read_from(buf)?;
    let code = ServerCode::try_from(u32::read_from(buf)?)?;
    ServerResponse::read_with_code(code, buf)
}

/// Read a server request from a buffer (including length prefix).
/// Used by server implementations to parse client messages.
pub fn read_server_request<B: Buf>(buf: &mut B) -> Result<ServerRequest> {
    let _len = u32::read_from(buf)?;
    let code = ServerCode::try_from(u32::read_from(buf)?)?;
    ServerRequest::read_with_code(code, buf)
}

impl MessageRead for ServerRequest {
    type Code = ServerCode;

    fn read_with_code<B: Buf>(code: ServerCode, buf: &mut B) -> Result<Self> {
        match code {
            ServerCode::Login => {
                let username = String::read_from(buf)?;
                let password = String::read_from(buf)?;
                let version = u32::read_from(buf)?;
                let _hash = String::read_from(buf)?; // MD5 hash, we don't need it
                let minor_version = u32::read_from(buf)?;
                Ok(ServerRequest::Login {
                    username,
                    password,
                    version,
                    minor_version,
                })
            }
            ServerCode::SetWaitPort => {
                let port = u32::read_from(buf)?;
                let (obfuscation_type, obfuscated_port) = if buf.has_remaining() {
                    let obs = ObfuscationType::try_from(u32::read_from(buf)?)?;
                    let obs_port = u32::read_from(buf)?;
                    (Some(obs), Some(obs_port))
                } else {
                    (None, None)
                };
                Ok(ServerRequest::SetWaitPort {
                    port,
                    obfuscation_type,
                    obfuscated_port,
                })
            }
            ServerCode::GetPeerAddress => {
                let username = String::read_from(buf)?;
                Ok(ServerRequest::GetPeerAddress { username })
            }
            ServerCode::WatchUser => {
                let username = String::read_from(buf)?;
                Ok(ServerRequest::WatchUser { username })
            }
            ServerCode::UnwatchUser => {
                let username = String::read_from(buf)?;
                Ok(ServerRequest::UnwatchUser { username })
            }
            ServerCode::GetUserStatus => {
                let username = String::read_from(buf)?;
                Ok(ServerRequest::GetUserStatus { username })
            }
            ServerCode::SayChatroom => {
                let room = String::read_from(buf)?;
                let message = String::read_from(buf)?;
                Ok(ServerRequest::SayChatroom { room, message })
            }
            ServerCode::JoinRoom => {
                let room = String::read_from(buf)?;
                let private = if buf.has_remaining() {
                    u32::read_from(buf)? != 0
                } else {
                    false
                };
                Ok(ServerRequest::JoinRoom { room, private })
            }
            ServerCode::LeaveRoom => {
                let room = String::read_from(buf)?;
                Ok(ServerRequest::LeaveRoom { room })
            }
            ServerCode::ConnectToPeer => {
                let token = u32::read_from(buf)?;
                let username = String::read_from(buf)?;
                let conn_type_str = String::read_from(buf)?;
                let connection_type = ConnectionType::parse(&conn_type_str)?;
                Ok(ServerRequest::ConnectToPeer {
                    token,
                    username,
                    connection_type,
                })
            }
            ServerCode::MessageUser => {
                let username = String::read_from(buf)?;
                let message = String::read_from(buf)?;
                Ok(ServerRequest::MessageUser { username, message })
            }
            ServerCode::MessageAcked => {
                let message_id = u32::read_from(buf)?;
                Ok(ServerRequest::MessageAcked { message_id })
            }
            ServerCode::FileSearch => {
                let token = u32::read_from(buf)?;
                let query = String::read_from(buf)?;
                Ok(ServerRequest::FileSearch { token, query })
            }
            ServerCode::SetStatus => {
                let status_val = i32::read_from(buf)?;
                let status = UserStatus::try_from(status_val as u32)?;
                Ok(ServerRequest::SetStatus { status })
            }
            ServerCode::ServerPing => Ok(ServerRequest::ServerPing),
            ServerCode::SharedFoldersFiles => {
                let dirs = u32::read_from(buf)?;
                let files = u32::read_from(buf)?;
                Ok(ServerRequest::SharedFoldersFiles { dirs, files })
            }
            ServerCode::GetUserStats => {
                let username = String::read_from(buf)?;
                Ok(ServerRequest::GetUserStats { username })
            }
            ServerCode::UserSearch => {
                let username = String::read_from(buf)?;
                let token = u32::read_from(buf)?;
                let query = String::read_from(buf)?;
                Ok(ServerRequest::UserSearch {
                    username,
                    token,
                    query,
                })
            }
            ServerCode::InterestAdd => {
                let item = String::read_from(buf)?;
                Ok(ServerRequest::InterestAdd { item })
            }
            ServerCode::InterestRemove => {
                let item = String::read_from(buf)?;
                Ok(ServerRequest::InterestRemove { item })
            }
            ServerCode::GetRecommendations => Ok(ServerRequest::GetRecommendations),
            ServerCode::GetGlobalRecommendations => Ok(ServerRequest::GetGlobalRecommendations),
            ServerCode::GetUserInterests => {
                let username = String::read_from(buf)?;
                Ok(ServerRequest::GetUserInterests { username })
            }
            ServerCode::RoomList => Ok(ServerRequest::RoomList),
            ServerCode::HaveNoParent => {
                let no_parent = bool::read_from(buf)?;
                Ok(ServerRequest::HaveNoParent { no_parent })
            }
            ServerCode::CheckPrivileges => Ok(ServerRequest::CheckPrivileges),
            ServerCode::AcceptChildren => {
                let accept = bool::read_from(buf)?;
                Ok(ServerRequest::AcceptChildren { accept })
            }
            ServerCode::WishlistSearch => {
                let token = u32::read_from(buf)?;
                let query = String::read_from(buf)?;
                Ok(ServerRequest::WishlistSearch { token, query })
            }
            ServerCode::GetSimilarUsers => Ok(ServerRequest::GetSimilarUsers),
            ServerCode::GetItemRecommendations => {
                let item = String::read_from(buf)?;
                Ok(ServerRequest::GetItemRecommendations { item })
            }
            ServerCode::GetItemSimilarUsers => {
                let item = String::read_from(buf)?;
                Ok(ServerRequest::GetItemSimilarUsers { item })
            }
            ServerCode::RoomTickerSet => {
                let room = String::read_from(buf)?;
                let ticker = String::read_from(buf)?;
                Ok(ServerRequest::RoomTickerSet { room, ticker })
            }
            ServerCode::HatedInterestAdd => {
                let item = String::read_from(buf)?;
                Ok(ServerRequest::HatedInterestAdd { item })
            }
            ServerCode::HatedInterestRemove => {
                let item = String::read_from(buf)?;
                Ok(ServerRequest::HatedInterestRemove { item })
            }
            ServerCode::RoomSearch => {
                let room = String::read_from(buf)?;
                let token = u32::read_from(buf)?;
                let query = String::read_from(buf)?;
                Ok(ServerRequest::RoomSearch { room, token, query })
            }
            ServerCode::SendUploadSpeed => {
                let speed = u32::read_from(buf)?;
                Ok(ServerRequest::SendUploadSpeed { speed })
            }
            ServerCode::GivePrivileges => {
                let username = String::read_from(buf)?;
                let days = u32::read_from(buf)?;
                Ok(ServerRequest::GivePrivileges { username, days })
            }
            ServerCode::BranchLevel => {
                let level = u32::read_from(buf)?;
                Ok(ServerRequest::BranchLevel { level })
            }
            ServerCode::BranchRoot => {
                let root = String::read_from(buf)?;
                Ok(ServerRequest::BranchRoot { root })
            }
            ServerCode::AddRoomMember => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerRequest::AddRoomMember { room, username })
            }
            ServerCode::RemoveRoomMember => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerRequest::RemoveRoomMember { room, username })
            }
            ServerCode::CancelRoomMembership => {
                let room = String::read_from(buf)?;
                Ok(ServerRequest::CancelRoomMembership { room })
            }
            ServerCode::CancelRoomOwnership => {
                let room = String::read_from(buf)?;
                Ok(ServerRequest::CancelRoomOwnership { room })
            }
            ServerCode::EnableRoomInvitations => {
                let enable = bool::read_from(buf)?;
                Ok(ServerRequest::EnableRoomInvitations { enable })
            }
            ServerCode::ChangePassword => {
                let password = String::read_from(buf)?;
                Ok(ServerRequest::ChangePassword { password })
            }
            ServerCode::AddRoomOperator => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerRequest::AddRoomOperator { room, username })
            }
            ServerCode::RemoveRoomOperator => {
                let room = String::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerRequest::RemoveRoomOperator { room, username })
            }
            ServerCode::MessageUsers => {
                let usernames = read_list(buf, String::read_from)?;
                let message = String::read_from(buf)?;
                Ok(ServerRequest::MessageUsers { usernames, message })
            }
            ServerCode::JoinGlobalRoom => Ok(ServerRequest::JoinGlobalRoom),
            ServerCode::LeaveGlobalRoom => Ok(ServerRequest::LeaveGlobalRoom),
            ServerCode::CantConnectToPeer => {
                let token = u32::read_from(buf)?;
                let username = String::read_from(buf)?;
                Ok(ServerRequest::CantConnectToPeer { token, username })
            }
            // Response-only codes
            _ => Err(Error::Protocol(format!(
                "Server code {:?} is response-only, not expected in request",
                code
            ))),
        }
    }
}

impl MessageWrite for ServerResponse {
    type Code = ServerCode;

    fn code(&self) -> ServerCode {
        match self {
            ServerResponse::LoginSuccess { .. } => ServerCode::Login,
            ServerResponse::LoginFailure { .. } => ServerCode::Login,
            ServerResponse::GetPeerAddress { .. } => ServerCode::GetPeerAddress,
            ServerResponse::WatchUser { .. } => ServerCode::WatchUser,
            ServerResponse::GetUserStatus { .. } => ServerCode::GetUserStatus,
            ServerResponse::SayChatroom { .. } => ServerCode::SayChatroom,
            ServerResponse::JoinRoom { .. } => ServerCode::JoinRoom,
            ServerResponse::LeaveRoom { .. } => ServerCode::LeaveRoom,
            ServerResponse::UserJoinedRoom { .. } => ServerCode::UserJoinedRoom,
            ServerResponse::UserLeftRoom { .. } => ServerCode::UserLeftRoom,
            ServerResponse::ConnectToPeer { .. } => ServerCode::ConnectToPeer,
            ServerResponse::MessageUser { .. } => ServerCode::MessageUser,
            ServerResponse::FileSearch { .. } => ServerCode::FileSearch,
            ServerResponse::GetUserStats { .. } => ServerCode::GetUserStats,
            ServerResponse::Relogged => ServerCode::Relogged,
            ServerResponse::Recommendations { .. } => ServerCode::GetRecommendations,
            ServerResponse::GlobalRecommendations { .. } => ServerCode::GetGlobalRecommendations,
            ServerResponse::UserInterests { .. } => ServerCode::GetUserInterests,
            ServerResponse::RoomList { .. } => ServerCode::RoomList,
            ServerResponse::AdminMessage { .. } => ServerCode::AdminMessage,
            ServerResponse::PrivilegedUsers { .. } => ServerCode::PrivilegedUsers,
            ServerResponse::ParentMinSpeed { .. } => ServerCode::ParentMinSpeed,
            ServerResponse::ParentSpeedRatio { .. } => ServerCode::ParentSpeedRatio,
            ServerResponse::CheckPrivileges { .. } => ServerCode::CheckPrivileges,
            ServerResponse::EmbeddedMessage { .. } => ServerCode::EmbeddedMessage,
            ServerResponse::PossibleParents { .. } => ServerCode::PossibleParents,
            ServerResponse::WishlistInterval { .. } => ServerCode::WishlistInterval,
            ServerResponse::SimilarUsers { .. } => ServerCode::GetSimilarUsers,
            ServerResponse::ItemRecommendations { .. } => ServerCode::GetItemRecommendations,
            ServerResponse::ItemSimilarUsers { .. } => ServerCode::GetItemSimilarUsers,
            ServerResponse::RoomTickerState { .. } => ServerCode::RoomTickerState,
            ServerResponse::RoomTickerAdd { .. } => ServerCode::RoomTickerAdd,
            ServerResponse::RoomTickerRemove { .. } => ServerCode::RoomTickerRemove,
            ServerResponse::EnableRoomInvitations { .. } => ServerCode::EnableRoomInvitations,
            ServerResponse::ChangePassword { .. } => ServerCode::ChangePassword,
            ServerResponse::AddRoomOperator { .. } => ServerCode::AddRoomOperator,
            ServerResponse::RemoveRoomOperator { .. } => ServerCode::RemoveRoomOperator,
            ServerResponse::RoomOperatorshipGranted { .. } => ServerCode::RoomOperatorshipGranted,
            ServerResponse::RoomOperatorshipRevoked { .. } => ServerCode::RoomOperatorshipRevoked,
            ServerResponse::RoomOperators { .. } => ServerCode::RoomOperators,
            ServerResponse::RoomMembershipGranted { .. } => ServerCode::RoomMembershipGranted,
            ServerResponse::RoomMembershipRevoked { .. } => ServerCode::RoomMembershipRevoked,
            ServerResponse::RoomMembers { .. } => ServerCode::RoomMembers,
            ServerResponse::AddRoomMember { .. } => ServerCode::AddRoomMember,
            ServerResponse::RemoveRoomMember { .. } => ServerCode::RemoveRoomMember,
            ServerResponse::ResetDistributed => ServerCode::ResetDistributed,
            ServerResponse::GlobalRoomMessage { .. } => ServerCode::GlobalRoomMessage,
            ServerResponse::ExcludedSearchPhrases { .. } => ServerCode::ExcludedSearchPhrases,
            ServerResponse::CantConnectToPeer { .. } => ServerCode::CantConnectToPeer,
            ServerResponse::CantCreateRoom { .. } => ServerCode::CantCreateRoom,
        }
    }

    fn write_payload<B: BufMut>(&self, buf: &mut B) {
        match self {
            ServerResponse::LoginSuccess { greet, own_ip, password_hash, is_supporter } => {
                true.write_to(buf);
                greet.write_to(buf);
                own_ip.write_to(buf);
                password_hash.write_to(buf);
                is_supporter.write_to(buf);
            }
            ServerResponse::LoginFailure { reason, detail } => {
                false.write_to(buf);
                match reason {
                    LoginRejectionReason::InvalidUsername => "INVALIDUSERNAME".to_string().write_to(buf),
                    LoginRejectionReason::EmptyPassword => "EMPTYPASSWORD".to_string().write_to(buf),
                    LoginRejectionReason::InvalidPassword => "INVALIDPASS".to_string().write_to(buf),
                    LoginRejectionReason::InvalidVersion => "INVALIDVERSION".to_string().write_to(buf),
                    LoginRejectionReason::ServerFull => "SVRFULL".to_string().write_to(buf),
                    LoginRejectionReason::ServerPrivate => "SVRPRIVATE".to_string().write_to(buf),
                    LoginRejectionReason::Other(s) => s.write_to(buf),
                }
                if let Some(d) = detail {
                    d.write_to(buf);
                }
            }
            ServerResponse::GetPeerAddress { username, ip, port, obfuscation_type, obfuscated_port } => {
                username.write_to(buf);
                ip.write_to(buf);
                port.write_to(buf);
                (*obfuscation_type as u32).write_to(buf);
                obfuscated_port.write_to(buf);
            }
            ServerResponse::WatchUser { username, exists, status, stats, country_code } => {
                username.write_to(buf);
                exists.write_to(buf);
                if *exists {
                    if let Some(s) = status {
                        (*s as u32).write_to(buf);
                    }
                    if let Some(st) = stats {
                        st.write_to(buf);
                    }
                    if let Some(cc) = country_code {
                        cc.write_to(buf);
                    }
                }
            }
            ServerResponse::GetUserStatus { username, status, privileged } => {
                username.write_to(buf);
                (*status as u32).write_to(buf);
                privileged.write_to(buf);
            }
            ServerResponse::SayChatroom { room, username, message } => {
                room.write_to(buf);
                username.write_to(buf);
                message.write_to(buf);
            }
            ServerResponse::JoinRoom { room, users, owner, operators } => {
                room.write_to(buf);
                write_list(buf, users, |b, u| u.username.write_to(b));
                write_list(buf, users, |b, u| (u.status as u32).write_to(b));
                write_list(buf, users, |b, u| u.stats.write_to(b));
                write_list(buf, users, |b, u| (u.slots_full as u32).write_to(b));
                write_list(buf, users, |b, u| u.country_code.write_to(b));
                if let Some(o) = owner {
                    o.write_to(buf);
                    write_list(buf, operators, |b, op| op.write_to(b));
                }
            }
            ServerResponse::LeaveRoom { room } => {
                room.write_to(buf);
            }
            ServerResponse::UserJoinedRoom { room, username, status, stats, slots_full, country_code } => {
                room.write_to(buf);
                username.write_to(buf);
                (*status as u32).write_to(buf);
                stats.write_to(buf);
                (*slots_full as u32).write_to(buf);
                country_code.write_to(buf);
            }
            ServerResponse::UserLeftRoom { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::ConnectToPeer { username, connection_type, ip, port, token, privileged, obfuscation_type, obfuscated_port } => {
                username.write_to(buf);
                connection_type.as_str().to_string().write_to(buf);
                ip.write_to(buf);
                port.write_to(buf);
                token.write_to(buf);
                privileged.write_to(buf);
                (*obfuscation_type as u32).write_to(buf);
                obfuscated_port.write_to(buf);
            }
            ServerResponse::MessageUser { id, timestamp, username, message, new_message } => {
                id.write_to(buf);
                timestamp.write_to(buf);
                username.write_to(buf);
                message.write_to(buf);
                new_message.write_to(buf);
            }
            ServerResponse::FileSearch { username, token, query } => {
                username.write_to(buf);
                token.write_to(buf);
                query.write_to(buf);
            }
            ServerResponse::GetUserStats { username, stats } => {
                username.write_to(buf);
                stats.write_to(buf);
            }
            ServerResponse::Relogged => {}
            ServerResponse::Recommendations { recommendations, unrecommendations } => {
                write_list(buf, recommendations, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
                write_list(buf, unrecommendations, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
            }
            ServerResponse::GlobalRecommendations { recommendations, unrecommendations } => {
                write_list(buf, recommendations, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
                write_list(buf, unrecommendations, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
            }
            ServerResponse::UserInterests { username, likes, hates } => {
                username.write_to(buf);
                write_list(buf, likes, |b, l| l.write_to(b));
                write_list(buf, hates, |b, h| h.write_to(b));
            }
            ServerResponse::RoomList { rooms, owned_private_rooms, private_rooms, operated_private_rooms } => {
                write_list(buf, rooms, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
                write_list(buf, owned_private_rooms, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
                write_list(buf, private_rooms, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
                write_list(buf, operated_private_rooms, |b, name| name.write_to(b));
            }
            ServerResponse::AdminMessage { message } => {
                message.write_to(buf);
            }
            ServerResponse::PrivilegedUsers { users } => {
                write_list(buf, users, |b, u| u.write_to(b));
            }
            ServerResponse::ParentMinSpeed { speed } => {
                speed.write_to(buf);
            }
            ServerResponse::ParentSpeedRatio { ratio } => {
                ratio.write_to(buf);
            }
            ServerResponse::CheckPrivileges { time_left } => {
                time_left.write_to(buf);
            }
            ServerResponse::EmbeddedMessage { code, data } => {
                code.write_to(buf);
                buf.put_slice(data);
            }
            ServerResponse::PossibleParents { parents } => {
                write_list(buf, parents, |b, p| {
                    p.username.write_to(b);
                    p.ip.write_to(b);
                    p.port.write_to(b);
                });
            }
            ServerResponse::WishlistInterval { interval } => {
                interval.write_to(buf);
            }
            ServerResponse::SimilarUsers { users } => {
                write_list(buf, users, |b, (name, rating)| {
                    name.write_to(b);
                    rating.write_to(b);
                });
            }
            ServerResponse::ItemRecommendations { item, recommendations } => {
                item.write_to(buf);
                write_list(buf, recommendations, |b, (name, count)| {
                    name.write_to(b);
                    count.write_to(b);
                });
            }
            ServerResponse::ItemSimilarUsers { item, users } => {
                item.write_to(buf);
                write_list(buf, users, |b, u| u.write_to(b));
            }
            ServerResponse::RoomTickerState { room, tickers } => {
                room.write_to(buf);
                write_list(buf, tickers, |b, t| {
                    t.username.write_to(b);
                    t.ticker.write_to(b);
                });
            }
            ServerResponse::RoomTickerAdd { room, username, ticker } => {
                room.write_to(buf);
                username.write_to(buf);
                ticker.write_to(buf);
            }
            ServerResponse::RoomTickerRemove { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::EnableRoomInvitations { enable } => {
                enable.write_to(buf);
            }
            ServerResponse::ChangePassword { password } => {
                password.write_to(buf);
            }
            ServerResponse::AddRoomOperator { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::RemoveRoomOperator { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::RoomOperatorshipGranted { room } => {
                room.write_to(buf);
            }
            ServerResponse::RoomOperatorshipRevoked { room } => {
                room.write_to(buf);
            }
            ServerResponse::RoomOperators { room, operators } => {
                room.write_to(buf);
                write_list(buf, operators, |b, o| o.write_to(b));
            }
            ServerResponse::RoomMembershipGranted { room } => {
                room.write_to(buf);
            }
            ServerResponse::RoomMembershipRevoked { room } => {
                room.write_to(buf);
            }
            ServerResponse::RoomMembers { room, members } => {
                room.write_to(buf);
                write_list(buf, members, |b, m| m.write_to(b));
            }
            ServerResponse::AddRoomMember { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::RemoveRoomMember { room, username } => {
                room.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::ResetDistributed => {}
            ServerResponse::GlobalRoomMessage { room, username, message } => {
                room.write_to(buf);
                username.write_to(buf);
                message.write_to(buf);
            }
            ServerResponse::ExcludedSearchPhrases { phrases } => {
                write_list(buf, phrases, |b, p| p.write_to(b));
            }
            ServerResponse::CantConnectToPeer { token, username } => {
                token.write_to(buf);
                username.write_to(buf);
            }
            ServerResponse::CantCreateRoom { room } => {
                room.write_to(buf);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_login_request() {
        let req = ServerRequest::Login {
            username: "testuser".to_string(),
            password: "testpass".to_string(),
            version: 160,
            minor_version: 1,
        };

        let mut buf = BytesMut::new();
        req.write_message(&mut buf);

        // Verify it can be written without panic
        assert!(buf.len() > 8);
    }

    #[test]
    fn test_file_search_request() {
        let req = ServerRequest::FileSearch {
            token: 12345,
            query: "test query".to_string(),
        };

        let mut buf = BytesMut::new();
        req.write_message(&mut buf);
        assert!(buf.len() > 8);
    }
}
