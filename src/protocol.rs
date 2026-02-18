//! Protocol primitives for reading and writing SoulSeek messages.
//!
//! All integers are little-endian. Strings are prefixed with a u32 length.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::{Read, Write};
use std::net::Ipv4Addr;

use crate::{Error, Result};

/// Trait for reading protocol primitives from a buffer.
pub trait ProtocolRead: Sized {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self>;
}

/// Trait for writing protocol primitives to a buffer.
pub trait ProtocolWrite {
    fn write_to<B: BufMut>(&self, buf: &mut B);

    fn to_bytes(&self) -> Bytes {
        let mut buf = BytesMut::new();
        self.write_to(&mut buf);
        buf.freeze()
    }
}

/// Trait for reading complete messages with a code prefix.
pub trait MessageRead: Sized {
    /// The code type (u32 for server/peer messages, u8 for peer init/distributed).
    type Code;

    /// Read a message from the buffer, given its code.
    fn read_with_code<B: Buf>(code: Self::Code, buf: &mut B) -> Result<Self>;
}

/// Trait for writing complete messages with a code prefix.
pub trait MessageWrite {
    /// The code type.
    type Code;

    /// Get the message code.
    fn code(&self) -> Self::Code;

    /// Write the message contents (without length prefix or code).
    fn write_payload<B: BufMut>(&self, buf: &mut B);

    /// Write a complete message with length prefix and code.
    fn write_message<B: BufMut>(&self, buf: &mut B)
    where
        Self::Code: Into<u32> + Copy,
    {
        let mut payload = BytesMut::new();
        self.write_payload(&mut payload);

        let code: u32 = self.code().into();
        let total_len = 4 + payload.len(); // code (4 bytes) + payload
        buf.put_u32_le(total_len as u32);
        buf.put_u32_le(code);
        buf.put_slice(&payload);
    }

    /// Write a complete message with u8 code (for peer init/distributed).
    fn write_message_u8<B: BufMut>(&self, buf: &mut B)
    where
        Self::Code: Into<u8> + Copy,
    {
        let mut payload = BytesMut::new();
        self.write_payload(&mut payload);

        let code: u8 = self.code().into();
        let total_len = 1 + payload.len(); // code (1 byte) + payload
        buf.put_u32_le(total_len as u32);
        buf.put_u8(code);
        buf.put_slice(&payload);
    }
}

// Primitive implementations

impl ProtocolRead for u8 {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 1 {
            return Err(Error::BufferUnderflow {
                needed: 1,
                available: buf.remaining(),
            });
        }
        Ok(buf.get_u8())
    }
}

impl ProtocolWrite for u8 {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u8(*self);
    }
}

impl ProtocolRead for u16 {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 2 {
            return Err(Error::BufferUnderflow {
                needed: 2,
                available: buf.remaining(),
            });
        }
        Ok(buf.get_u16_le())
    }
}

impl ProtocolWrite for u16 {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u16_le(*self);
    }
}

impl ProtocolRead for u32 {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 4 {
            return Err(Error::BufferUnderflow {
                needed: 4,
                available: buf.remaining(),
            });
        }
        Ok(buf.get_u32_le())
    }
}

impl ProtocolWrite for u32 {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u32_le(*self);
    }
}

impl ProtocolRead for i32 {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 4 {
            return Err(Error::BufferUnderflow {
                needed: 4,
                available: buf.remaining(),
            });
        }
        Ok(buf.get_i32_le())
    }
}

impl ProtocolWrite for i32 {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_i32_le(*self);
    }
}

impl ProtocolRead for u64 {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 8 {
            return Err(Error::BufferUnderflow {
                needed: 8,
                available: buf.remaining(),
            });
        }
        Ok(buf.get_u64_le())
    }
}

impl ProtocolWrite for u64 {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u64_le(*self);
    }
}

impl ProtocolRead for bool {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let byte = u8::read_from(buf)?;
        Ok(byte != 0)
    }
}

impl ProtocolWrite for bool {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u8(if *self { 1 } else { 0 });
    }
}

impl ProtocolRead for String {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        let len = u32::read_from(buf)? as usize;
        if buf.remaining() < len {
            return Err(Error::BufferUnderflow {
                needed: len,
                available: buf.remaining(),
            });
        }
        let mut bytes = vec![0u8; len];
        buf.copy_to_slice(&mut bytes);
        Ok(String::from_utf8(bytes)?)
    }
}

impl ProtocolWrite for String {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u32_le(self.len() as u32);
        buf.put_slice(self.as_bytes());
    }
}

impl ProtocolWrite for &str {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        buf.put_u32_le(self.len() as u32);
        buf.put_slice(self.as_bytes());
    }
}

/// Read raw bytes with a u32 length prefix.
pub fn read_bytes<B: Buf>(buf: &mut B) -> Result<Vec<u8>> {
    let len = u32::read_from(buf)? as usize;
    if buf.remaining() < len {
        return Err(Error::BufferUnderflow {
            needed: len,
            available: buf.remaining(),
        });
    }
    let mut bytes = vec![0u8; len];
    buf.copy_to_slice(&mut bytes);
    Ok(bytes)
}

/// Write raw bytes with a u32 length prefix.
pub fn write_bytes<B: BufMut>(buf: &mut B, data: &[u8]) {
    buf.put_u32_le(data.len() as u32);
    buf.put_slice(data);
}

impl ProtocolRead for Ipv4Addr {
    fn read_from<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 4 {
            return Err(Error::BufferUnderflow {
                needed: 4,
                available: buf.remaining(),
            });
        }
        // IP is stored as a 32-bit little-endian integer in the Soulseek protocol
        // The bytes are in reverse order: [d, c, b, a] represents a.b.c.d
        let d = buf.get_u8();
        let c = buf.get_u8();
        let b = buf.get_u8();
        let a = buf.get_u8();
        Ok(Ipv4Addr::new(a, b, c, d))
    }
}

impl ProtocolWrite for Ipv4Addr {
    fn write_to<B: BufMut>(&self, buf: &mut B) {
        // Write IP as little-endian 32-bit integer
        let octets = self.octets();
        buf.put_u8(octets[3]);
        buf.put_u8(octets[2]);
        buf.put_u8(octets[1]);
        buf.put_u8(octets[0]);
    }
}

/// Compress data using zlib.
pub fn zlib_compress(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| Error::Compression(e.to_string()))?;
    encoder
        .finish()
        .map_err(|e| Error::Compression(e.to_string()))
}

/// Decompress zlib data.
pub fn zlib_decompress(data: &[u8]) -> Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;

    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| Error::Decompression(e.to_string()))?;
    Ok(decompressed)
}

/// Generate MD5 hash of username + password for login.
pub fn login_hash(username: &str, password: &str) -> String {
    let input = format!("{}{}", username, password);
    let digest = md5::compute(input.as_bytes());
    format!("{:x}", digest)
}

/// Read a list of items from a buffer.
pub fn read_list<B, T, F>(buf: &mut B, read_fn: F) -> Result<Vec<T>>
where
    B: Buf,
    F: Fn(&mut B) -> Result<T>,
{
    let count = u32::read_from(buf)? as usize;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(read_fn(buf)?);
    }
    Ok(items)
}

/// Write a list of items to a buffer.
pub fn write_list<B, T, F>(buf: &mut B, items: &[T], write_fn: F)
where
    B: BufMut,
    F: Fn(&mut B, &T),
{
    buf.put_u32_le(items.len() as u32);
    for item in items {
        write_fn(buf, item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn test_u32_roundtrip() {
        let mut buf = BytesMut::new();
        42u32.write_to(&mut buf);
        assert_eq!(u32::read_from(&mut buf.freeze()).unwrap(), 42);
    }

    #[test]
    fn test_string_roundtrip() {
        let mut buf = BytesMut::new();
        "hello".write_to(&mut buf);
        assert_eq!(String::read_from(&mut buf.freeze()).unwrap(), "hello");
    }

    #[test]
    fn test_ip_roundtrip() {
        let mut buf = BytesMut::new();
        let ip = Ipv4Addr::new(192, 168, 1, 1);
        ip.write_to(&mut buf);
        assert_eq!(Ipv4Addr::read_from(&mut buf.freeze()).unwrap(), ip);
    }

    #[test]
    fn test_login_hash() {
        // Example from protocol docs
        let hash = login_hash("username", "password");
        assert_eq!(hash, "d51c9a7e9353746a6020f9602d452929");
    }

    #[test]
    fn test_zlib_roundtrip() {
        let original = b"hello world, this is a test of compression";
        let compressed = zlib_compress(original).unwrap();
        let decompressed = zlib_decompress(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }
}
