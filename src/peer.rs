//! Peer messages sent over P connections.
//!
//! These messages are sent to peers for file browsing, searching, transfers, etc.

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::constants::{TransferDirection, TransferRejectionReason, UploadPermission};
use crate::protocol::{
    MessageRead, MessageWrite, ProtocolRead, ProtocolWrite, read_list, write_list, zlib_compress,
    zlib_decompress,
};
use crate::{Error, Result};

/// Peer message codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum PeerCode {
    SharedFileListRequest = 4,
    SharedFileListResponse = 5,
    FileSearchResponse = 9,
    UserInfoRequest = 15,
    UserInfoResponse = 16,
    FolderContentsRequest = 36,
    FolderContentsResponse = 37,
    TransferRequest = 40,
    TransferResponse = 41,
    QueueUpload = 43,
    PlaceInQueueResponse = 44,
    UploadFailed = 46,
    UploadDenied = 50,
    PlaceInQueueRequest = 51,
    UploadQueueNotification = 52,
}

impl TryFrom<u32> for PeerCode {
    type Error = Error;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            4 => Ok(PeerCode::SharedFileListRequest),
            5 => Ok(PeerCode::SharedFileListResponse),
            9 => Ok(PeerCode::FileSearchResponse),
            15 => Ok(PeerCode::UserInfoRequest),
            16 => Ok(PeerCode::UserInfoResponse),
            36 => Ok(PeerCode::FolderContentsRequest),
            37 => Ok(PeerCode::FolderContentsResponse),
            40 => Ok(PeerCode::TransferRequest),
            41 => Ok(PeerCode::TransferResponse),
            43 => Ok(PeerCode::QueueUpload),
            44 => Ok(PeerCode::PlaceInQueueResponse),
            46 => Ok(PeerCode::UploadFailed),
            50 => Ok(PeerCode::UploadDenied),
            51 => Ok(PeerCode::PlaceInQueueRequest),
            52 => Ok(PeerCode::UploadQueueNotification),
            _ => Err(Error::InvalidMessageCode(value)),
        }
    }
}

impl From<PeerCode> for u32 {
    fn from(code: PeerCode) -> Self {
        code as u32
    }
}

/// File attribute (e.g., bitrate, duration).
#[derive(Debug, Clone)]
pub struct FileAttribute {
    pub code: u32,
    pub value: u32,
}

impl FileAttribute {
    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        Ok(FileAttribute {
            code: u32::read_from(buf)?,
            value: u32::read_from(buf)?,
        })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        self.code.write_to(buf);
        self.value.write_to(buf);
    }
}

/// Shared file entry.
#[derive(Debug, Clone)]
pub struct SharedFile {
    pub filename: String,
    pub size: u64,
    pub extension: String,
    pub attributes: Vec<FileAttribute>,
}

impl SharedFile {
    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let _code = u8::read_from(buf)?; // Always 1
        let filename = String::read_from(buf)?;
        let size = u64::read_from(buf)?;
        let extension = String::read_from(buf)?;
        let attributes = read_list(buf, FileAttribute::read_from)?;
        Ok(SharedFile {
            filename,
            size,
            extension,
            attributes,
        })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        1u8.write_to(buf); // Code always 1
        self.filename.write_to(buf);
        self.size.write_to(buf);
        self.extension.write_to(buf);
        write_list(buf, &self.attributes, |b, a| a.write_to(b));
    }
}

/// Directory with files.
#[derive(Debug, Clone)]
pub struct SharedDirectory {
    pub path: String,
    pub files: Vec<SharedFile>,
}

impl SharedDirectory {
    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let path = String::read_from(buf)?;
        let files = read_list(buf, SharedFile::read_from)?;
        Ok(SharedDirectory { path, files })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        self.path.write_to(buf);
        write_list(buf, &self.files, |b, f| f.write_to(b));
    }
}

/// Search result file.
#[derive(Debug, Clone)]
pub struct SearchResultFile {
    pub filename: String,
    pub size: u64,
    pub extension: String,
    pub attributes: Vec<FileAttribute>,
}

impl SearchResultFile {
    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let _code = u8::read_from(buf)?; // Always 1
        let filename = String::read_from(buf)?;
        let size = u64::read_from(buf)?;
        let extension = String::read_from(buf)?;
        let attributes = read_list(buf, FileAttribute::read_from)?;
        Ok(SearchResultFile {
            filename,
            size,
            extension,
            attributes,
        })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        1u8.write_to(buf);
        self.filename.write_to(buf);
        self.size.write_to(buf);
        self.extension.write_to(buf);
        write_list(buf, &self.attributes, |b, a| a.write_to(b));
    }
}

/// Peer messages.
#[derive(Debug, Clone)]
pub enum PeerMessage {
    /// Request shared file list.
    SharedFileListRequest,

    /// Response with shared file list.
    SharedFileListResponse {
        directories: Vec<SharedDirectory>,
        private_directories: Vec<SharedDirectory>,
    },

    /// File search response.
    FileSearchResponse {
        username: String,
        token: u32,
        results: Vec<SearchResultFile>,
        slot_free: bool,
        avg_speed: u32,
        queue_length: u32,
        private_results: Vec<SearchResultFile>,
    },

    /// Request user info.
    UserInfoRequest,

    /// Response with user info.
    UserInfoResponse {
        description: String,
        picture: Option<Vec<u8>>,
        total_uploads: u32,
        queue_size: u32,
        slots_free: bool,
        upload_permitted: Option<UploadPermission>,
    },

    /// Request folder contents.
    FolderContentsRequest { token: u32, folder: String },

    /// Response with folder contents.
    FolderContentsResponse {
        token: u32,
        folder: String,
        directories: Vec<SharedDirectory>,
    },

    /// Transfer request (upload ready).
    TransferRequest {
        direction: TransferDirection,
        token: u32,
        filename: String,
        file_size: Option<u64>,
    },

    /// Transfer response (accept/reject).
    TransferResponse {
        token: u32,
        allowed: bool,
        file_size: Option<u64>,
        reason: Option<TransferRejectionReason>,
    },

    /// Queue an upload.
    QueueUpload { filename: String },

    /// Place in queue response.
    PlaceInQueueResponse { filename: String, place: u32 },

    /// Upload failed notification.
    UploadFailed { filename: String },

    /// Upload denied.
    UploadDenied {
        filename: String,
        reason: TransferRejectionReason,
    },

    /// Request place in queue.
    PlaceInQueueRequest { filename: String },

    /// Upload queue notification (deprecated).
    UploadQueueNotification,
}

impl MessageWrite for PeerMessage {
    type Code = PeerCode;

    fn code(&self) -> PeerCode {
        match self {
            PeerMessage::SharedFileListRequest => PeerCode::SharedFileListRequest,
            PeerMessage::SharedFileListResponse { .. } => PeerCode::SharedFileListResponse,
            PeerMessage::FileSearchResponse { .. } => PeerCode::FileSearchResponse,
            PeerMessage::UserInfoRequest => PeerCode::UserInfoRequest,
            PeerMessage::UserInfoResponse { .. } => PeerCode::UserInfoResponse,
            PeerMessage::FolderContentsRequest { .. } => PeerCode::FolderContentsRequest,
            PeerMessage::FolderContentsResponse { .. } => PeerCode::FolderContentsResponse,
            PeerMessage::TransferRequest { .. } => PeerCode::TransferRequest,
            PeerMessage::TransferResponse { .. } => PeerCode::TransferResponse,
            PeerMessage::QueueUpload { .. } => PeerCode::QueueUpload,
            PeerMessage::PlaceInQueueResponse { .. } => PeerCode::PlaceInQueueResponse,
            PeerMessage::UploadFailed { .. } => PeerCode::UploadFailed,
            PeerMessage::UploadDenied { .. } => PeerCode::UploadDenied,
            PeerMessage::PlaceInQueueRequest { .. } => PeerCode::PlaceInQueueRequest,
            PeerMessage::UploadQueueNotification => PeerCode::UploadQueueNotification,
        }
    }

    fn write_payload<B: BufMut>(&self, buf: &mut B) {
        match self {
            PeerMessage::SharedFileListRequest => {}
            PeerMessage::SharedFileListResponse {
                directories,
                private_directories,
            } => {
                let mut uncompressed = BytesMut::new();
                write_list(&mut uncompressed, directories, |b, d| d.write_to(b));
                0u32.write_to(&mut uncompressed); // Unknown field
                write_list(&mut uncompressed, private_directories, |b, d| d.write_to(b));

                let compressed = zlib_compress(&uncompressed).unwrap_or_default();
                buf.put_slice(&compressed);
            }
            PeerMessage::FileSearchResponse {
                username,
                token,
                results,
                slot_free,
                avg_speed,
                queue_length,
                private_results,
            } => {
                let mut uncompressed = BytesMut::new();
                username.write_to(&mut uncompressed);
                token.write_to(&mut uncompressed);
                write_list(&mut uncompressed, results, |b, f| f.write_to(b));
                slot_free.write_to(&mut uncompressed);
                avg_speed.write_to(&mut uncompressed);
                queue_length.write_to(&mut uncompressed);
                0u32.write_to(&mut uncompressed); // Unknown field
                write_list(&mut uncompressed, private_results, |b, f| f.write_to(b));

                let compressed = zlib_compress(&uncompressed).unwrap_or_default();
                buf.put_slice(&compressed);
            }
            PeerMessage::UserInfoRequest => {}
            PeerMessage::UserInfoResponse {
                description,
                picture,
                total_uploads,
                queue_size,
                slots_free,
                upload_permitted,
            } => {
                description.write_to(buf);
                if let Some(pic) = picture {
                    true.write_to(buf);
                    (pic.len() as u32).write_to(buf);
                    buf.put_slice(pic);
                } else {
                    false.write_to(buf);
                }
                total_uploads.write_to(buf);
                queue_size.write_to(buf);
                slots_free.write_to(buf);
                if let Some(perm) = upload_permitted {
                    (*perm as u32).write_to(buf);
                }
            }
            PeerMessage::FolderContentsRequest { token, folder } => {
                token.write_to(buf);
                folder.write_to(buf);
            }
            PeerMessage::FolderContentsResponse {
                token,
                folder,
                directories,
            } => {
                let mut uncompressed = BytesMut::new();
                token.write_to(&mut uncompressed);
                folder.write_to(&mut uncompressed);
                write_list(&mut uncompressed, directories, |b, d| d.write_to(b));

                let compressed = zlib_compress(&uncompressed).unwrap_or_default();
                buf.put_slice(&compressed);
            }
            PeerMessage::TransferRequest {
                direction,
                token,
                filename,
                file_size,
            } => {
                (*direction as u32).write_to(buf);
                token.write_to(buf);
                filename.write_to(buf);
                if let Some(size) = file_size {
                    size.write_to(buf);
                }
            }
            PeerMessage::TransferResponse {
                token,
                allowed,
                file_size,
                reason,
            } => {
                token.write_to(buf);
                allowed.write_to(buf);
                if *allowed {
                    if let Some(size) = file_size {
                        size.write_to(buf);
                    }
                } else if let Some(r) = reason {
                    r.as_str().write_to(buf);
                }
            }
            PeerMessage::QueueUpload { filename } => {
                filename.write_to(buf);
            }
            PeerMessage::PlaceInQueueResponse { filename, place } => {
                filename.write_to(buf);
                place.write_to(buf);
            }
            PeerMessage::UploadFailed { filename } => {
                filename.write_to(buf);
            }
            PeerMessage::UploadDenied { filename, reason } => {
                filename.write_to(buf);
                reason.as_str().write_to(buf);
            }
            PeerMessage::PlaceInQueueRequest { filename } => {
                filename.write_to(buf);
            }
            PeerMessage::UploadQueueNotification => {}
        }
    }
}

impl MessageRead for PeerMessage {
    type Code = PeerCode;

    fn read_with_code<B: Buf>(code: PeerCode, buf: &mut B) -> Result<Self> {
        match code {
            PeerCode::SharedFileListRequest => Ok(PeerMessage::SharedFileListRequest),
            PeerCode::SharedFileListResponse => {
                let compressed: Vec<u8> = buf.chunk().to_vec();
                buf.advance(compressed.len());
                let decompressed = zlib_decompress(&compressed)?;
                let mut dbuf = Bytes::from(decompressed);

                let directories = read_list(&mut dbuf, SharedDirectory::read_from)?;
                let _unknown = u32::read_from(&mut dbuf)?;
                let private_directories = if dbuf.has_remaining() {
                    read_list(&mut dbuf, SharedDirectory::read_from)?
                } else {
                    vec![]
                };

                Ok(PeerMessage::SharedFileListResponse {
                    directories,
                    private_directories,
                })
            }
            PeerCode::FileSearchResponse => {
                let compressed: Vec<u8> = buf.chunk().to_vec();
                buf.advance(compressed.len());
                let decompressed = zlib_decompress(&compressed)?;
                let mut dbuf = Bytes::from(decompressed);

                let username = String::read_from(&mut dbuf)?;
                let token = u32::read_from(&mut dbuf)?;
                let results = read_list(&mut dbuf, SearchResultFile::read_from)?;
                let slot_free = bool::read_from(&mut dbuf)?;
                let avg_speed = u32::read_from(&mut dbuf)?;
                let queue_length = u32::read_from(&mut dbuf)?;
                let _unknown = u32::read_from(&mut dbuf)?;
                let private_results = if dbuf.has_remaining() {
                    read_list(&mut dbuf, SearchResultFile::read_from)?
                } else {
                    vec![]
                };

                Ok(PeerMessage::FileSearchResponse {
                    username,
                    token,
                    results,
                    slot_free,
                    avg_speed,
                    queue_length,
                    private_results,
                })
            }
            PeerCode::UserInfoRequest => Ok(PeerMessage::UserInfoRequest),
            PeerCode::UserInfoResponse => {
                let description = String::read_from(buf)?;
                let has_picture = bool::read_from(buf)?;
                let picture = if has_picture {
                    let len = u32::read_from(buf)? as usize;
                    let mut pic = vec![0u8; len];
                    buf.copy_to_slice(&mut pic);
                    Some(pic)
                } else {
                    None
                };
                let total_uploads = u32::read_from(buf)?;
                let queue_size = u32::read_from(buf)?;
                let slots_free = bool::read_from(buf)?;
                let upload_permitted = if buf.has_remaining() {
                    Some(UploadPermission::try_from(u32::read_from(buf)?)?)
                } else {
                    None
                };

                Ok(PeerMessage::UserInfoResponse {
                    description,
                    picture,
                    total_uploads,
                    queue_size,
                    slots_free,
                    upload_permitted,
                })
            }
            PeerCode::FolderContentsRequest => {
                let token = u32::read_from(buf)?;
                let folder = String::read_from(buf)?;
                Ok(PeerMessage::FolderContentsRequest { token, folder })
            }
            PeerCode::FolderContentsResponse => {
                let compressed: Vec<u8> = buf.chunk().to_vec();
                buf.advance(compressed.len());
                let decompressed = zlib_decompress(&compressed)?;
                let mut dbuf = Bytes::from(decompressed);

                let token = u32::read_from(&mut dbuf)?;
                let folder = String::read_from(&mut dbuf)?;
                let directories = read_list(&mut dbuf, SharedDirectory::read_from)?;

                Ok(PeerMessage::FolderContentsResponse {
                    token,
                    folder,
                    directories,
                })
            }
            PeerCode::TransferRequest => {
                let direction = TransferDirection::try_from(u32::read_from(buf)?)?;
                let token = u32::read_from(buf)?;
                let filename = String::read_from(buf)?;
                let file_size = if direction == TransferDirection::Upload && buf.has_remaining() {
                    Some(u64::read_from(buf)?)
                } else {
                    None
                };

                Ok(PeerMessage::TransferRequest {
                    direction,
                    token,
                    filename,
                    file_size,
                })
            }
            PeerCode::TransferResponse => {
                let token = u32::read_from(buf)?;
                let allowed = bool::read_from(buf)?;
                let (file_size, reason) = if allowed {
                    if buf.has_remaining() {
                        (Some(u64::read_from(buf)?), None)
                    } else {
                        (None, None)
                    }
                } else if buf.has_remaining() {
                    let reason_str = String::read_from(buf)?;
                    (None, Some(TransferRejectionReason::from_string(reason_str)))
                } else {
                    (None, None)
                };

                Ok(PeerMessage::TransferResponse {
                    token,
                    allowed,
                    file_size,
                    reason,
                })
            }
            PeerCode::QueueUpload => {
                let filename = String::read_from(buf)?;
                Ok(PeerMessage::QueueUpload { filename })
            }
            PeerCode::PlaceInQueueResponse => {
                let filename = String::read_from(buf)?;
                let place = u32::read_from(buf)?;
                Ok(PeerMessage::PlaceInQueueResponse { filename, place })
            }
            PeerCode::UploadFailed => {
                let filename = String::read_from(buf)?;
                Ok(PeerMessage::UploadFailed { filename })
            }
            PeerCode::UploadDenied => {
                let filename = String::read_from(buf)?;
                let reason_str = String::read_from(buf)?;
                Ok(PeerMessage::UploadDenied {
                    filename,
                    reason: TransferRejectionReason::from_string(reason_str),
                })
            }
            PeerCode::PlaceInQueueRequest => {
                let filename = String::read_from(buf)?;
                Ok(PeerMessage::PlaceInQueueRequest { filename })
            }
            PeerCode::UploadQueueNotification => Ok(PeerMessage::UploadQueueNotification),
        }
    }
}

/// Read a peer message from a buffer (including length prefix).
pub fn read_peer_message<B: Buf>(buf: &mut B) -> Result<PeerMessage> {
    let _len = u32::read_from(buf)?;
    let code = PeerCode::try_from(u32::read_from(buf)?)?;
    PeerMessage::read_with_code(code, buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_queue_upload_roundtrip() {
        let msg = PeerMessage::QueueUpload {
            filename: "Music/test.mp3".to_string(),
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);

        let parsed = read_peer_message(&mut buf.freeze()).unwrap();
        match parsed {
            PeerMessage::QueueUpload { filename } => {
                assert_eq!(filename, "Music/test.mp3");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_transfer_request_roundtrip() {
        let msg = PeerMessage::TransferRequest {
            direction: TransferDirection::Upload,
            token: 12345,
            filename: "test.mp3".to_string(),
            file_size: Some(1024),
        };
        let mut buf = BytesMut::new();
        msg.write_message(&mut buf);

        let parsed = read_peer_message(&mut buf.freeze()).unwrap();
        match parsed {
            PeerMessage::TransferRequest {
                direction,
                token,
                filename,
                file_size,
            } => {
                assert_eq!(direction, TransferDirection::Upload);
                assert_eq!(token, 12345);
                assert_eq!(filename, "test.mp3");
                assert_eq!(file_size, Some(1024));
            }
            _ => panic!("Wrong message type"),
        }
    }
}
