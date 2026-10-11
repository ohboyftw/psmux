//! Frame codec: a 4-byte big-endian length, then that many bytes of a
//! protobuf `Envelope`.
//!
//! Two deliberate differences from the C++ server:
//! - An oversized length prefix is [`FrameError::TooLarge`]. The stream can
//!   no longer be trusted, so the caller must close the connection (C++
//!   discarded its buffer and kept reading garbage).
//! - An undecodable body is [`FrameError::Decode`]. The body was consumed,
//!   so the stream is still in sync and the caller may skip the frame.

use std::fmt;
use std::io::{self, Read, Write};

use prost::Message;

use super::proto::Envelope;

/// Largest allowed frame body, in bytes. A body of exactly this size is
/// accepted.
pub const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Why a frame could not be read or written.
#[derive(Debug)]
pub enum FrameError {
    /// The socket failed, or closed in the middle of a frame.
    Io(io::Error),
    /// The length prefix (or an outgoing body) exceeds [`MAX_FRAME_SIZE`].
    /// The connection must be closed.
    TooLarge(usize),
    /// The body is not a valid `Envelope`. The stream is still framed.
    Decode(prost::DecodeError),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "frame i/o error: {e}"),
            FrameError::TooLarge(n) => {
                write!(f, "frame of {n} bytes exceeds {MAX_FRAME_SIZE}")
            }
            FrameError::Decode(e) => write!(f, "invalid envelope: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        FrameError::Io(e)
    }
}

/// Encode an envelope into one complete frame (prefix included), so a
/// fan-out can encode once and write the same bytes to every subscriber.
pub fn encode_frame(env: &Envelope) -> Result<Vec<u8>, FrameError> {
    let len = env.encoded_len();
    if len > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(len));
    }
    let mut buf = Vec::with_capacity(4 + len);
    buf.extend_from_slice(&(len as u32).to_be_bytes());
    env.encode(&mut buf)
        .expect("Vec<u8> grows, so encoding cannot run out of space");
    Ok(buf)
}

/// Encode and write one frame.
pub fn write_frame<W: Write>(w: &mut W, env: &Envelope) -> Result<(), FrameError> {
    w.write_all(&encode_frame(env)?)?;
    Ok(())
}

/// Read one frame. Returns `Ok(None)` on a clean end of stream at a frame
/// boundary; a stream that ends inside a frame is an `Io` error.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Option<Envelope>, FrameError> {
    let mut prefix = [0u8; 4];
    if !read_prefix(r, &mut prefix)? {
        return Ok(None);
    }
    let len = u32::from_be_bytes(prefix) as usize;
    if len > MAX_FRAME_SIZE {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Envelope::decode(body.as_slice())
        .map(Some)
        .map_err(FrameError::Decode)
}

/// Fill `prefix`, telling a clean EOF before the first byte (`false`) apart
/// from an EOF part-way through (an error).
fn read_prefix<R: Read>(r: &mut R, prefix: &mut [u8; 4]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < prefix.len() {
        match r.read(&mut prefix[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

#[cfg(test)]
#[path = "../../tests-rs/test_bus_codec.rs"]
mod tests;
