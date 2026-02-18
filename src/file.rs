//! File transfer messages sent over F connections.
//!
//! File messages don't have message codes - they are raw token/offset values.

use bytes::{Buf, BufMut};

use crate::Result;
use crate::protocol::{ProtocolRead, ProtocolWrite};

/// File transfer initialization.
///
/// Sent at the start of an F connection to identify the transfer.
#[derive(Debug, Clone)]
pub struct FileTransferInit {
    /// Token from the TransferRequest message.
    pub token: u32,
}

impl FileTransferInit {
    pub fn new(token: u32) -> Self {
        FileTransferInit { token }
    }

    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let token = u32::read_from(buf)?;
        Ok(FileTransferInit { token })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        self.token.write_to(buf);
    }
}

/// File offset message.
///
/// Sent by the downloader to indicate where to resume from.
#[derive(Debug, Clone)]
pub struct FileOffset {
    /// Byte offset to resume from (0 for new downloads).
    pub offset: u64,
}

impl FileOffset {
    pub fn new(offset: u64) -> Self {
        FileOffset { offset }
    }

    pub fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let offset = u64::read_from(buf)?;
        Ok(FileOffset { offset })
    }

    pub fn write_to<B: BufMut>(&self, buf: &mut B) {
        self.offset.write_to(buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_file_transfer_init_roundtrip() {
        let init = FileTransferInit::new(12345);
        let mut buf = BytesMut::new();
        init.write_to(&mut buf);

        let parsed = FileTransferInit::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.token, 12345);
    }

    #[test]
    fn test_file_offset_roundtrip() {
        let offset = FileOffset::new(1024 * 1024 * 500); // 500MB
        let mut buf = BytesMut::new();
        offset.write_to(&mut buf);

        let parsed = FileOffset::read_from(&mut buf.freeze()).unwrap();
        assert_eq!(parsed.offset, 1024 * 1024 * 500);
    }
}
