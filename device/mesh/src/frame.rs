//! The mesh's wire unit: a length-prefixed JSON frame.
//!
//! `{kind, id, payload}` with a four-byte big-endian length in front. The cap
//! is one mebibyte and it is enforced **before** the body buffer exists: a
//! peer cannot make this side allocate a length it announced but would never
//! send.
//!
//! Phase 2 carries frames, not MCP calls. The server opens with `challenge`
//! and the dialer answers with the signed `hello` (verified by
//! [`crate::endpoint`]); `req`/`res` correlate by `id`, `stream` is a chunk of
//! a longer answer, and `ping` is liveness.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The largest JSON body the mesh will read or write, in bytes.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// The big-endian `u32` length in front of every body.
pub const LENGTH_PREFIX_BYTES: usize = 4;

/// What a frame is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// The server's first frame: the fresh challenge the dialer must answer.
    Challenge,
    /// The signed answer to the challenge. It must be the first frame a
    /// client sends on the stream; the server refuses the connection
    /// otherwise.
    Hello,
    /// A request; `id` names it and the matching `res` carries the answer.
    Req,
    /// A response to the `req` with the same `id`.
    Res,
    /// One chunk of a longer response with the same `id`.
    Stream,
    /// Liveness; a peer answers with `res`.
    Ping,
}

/// One mesh frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// What this frame is for.
    pub kind: Kind,
    /// The request id that ties a response to its request.
    pub id: u64,
    /// The body. Phase 2 treats it as opaque JSON.
    pub payload: Value,
}

/// Why a frame could not be read, written or parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// The announced length is above [`MAX_FRAME_BYTES`]; nothing was read.
    Oversize {
        /// The length the peer announced.
        len: usize,
        /// The cap this build enforces.
        max: usize,
    },
    /// The stream ended cleanly at a frame boundary: there is no next frame.
    Closed,
    /// The input ended before the frame did.
    Truncated,
    /// The body was not JSON of the `{kind, id, payload}` shape.
    InvalidJson(String),
    /// Bytes followed a complete frame where only one was expected.
    Trailing,
    /// The underlying stream failed.
    Io(std::io::ErrorKind),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Oversize { len, max } => {
                write!(
                    f,
                    "a mesh frame is at most {max} bytes; this one announced {len}"
                )
            }
            Self::Closed => f.write_str("the mesh stream ended at a frame boundary"),
            Self::Truncated => f.write_str("the mesh frame ended before it was complete"),
            Self::InvalidJson(message) => write!(f, "not a mesh frame: {message}"),
            Self::Trailing => f.write_str("bytes follow the mesh frame"),
            Self::Io(kind) => write!(f, "the mesh stream failed: {kind:?}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<std::io::Error> for FrameError {
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::UnexpectedEof => Self::Truncated,
            kind => Self::Io(kind),
        }
    }
}

/// Serialize `frame` with its length prefix.
pub fn encode(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    let body =
        serde_json::to_vec(frame).map_err(|error| FrameError::InvalidJson(error.to_string()))?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(FrameError::Oversize {
            len: body.len(),
            max: MAX_FRAME_BYTES,
        });
    }
    let mut encoded = Vec::with_capacity(LENGTH_PREFIX_BYTES + body.len());
    encoded.extend_from_slice(&(body.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&body);
    Ok(encoded)
}

/// Parse exactly one complete framed frame.
pub fn decode(bytes: &[u8]) -> Result<Frame, FrameError> {
    if bytes.is_empty() {
        return Err(FrameError::Closed);
    }
    if bytes.len() < LENGTH_PREFIX_BYTES {
        return Err(FrameError::Truncated);
    }
    let length = u32::from_be_bytes(
        bytes[..LENGTH_PREFIX_BYTES]
            .try_into()
            .expect("the prefix is four bytes"),
    ) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::Oversize {
            len: length,
            max: MAX_FRAME_BYTES,
        });
    }
    let end = LENGTH_PREFIX_BYTES + length;
    if bytes.len() < end {
        return Err(FrameError::Truncated);
    }
    if bytes.len() > end {
        return Err(FrameError::Trailing);
    }
    serde_json::from_slice(&bytes[LENGTH_PREFIX_BYTES..end])
        .map_err(|error| FrameError::InvalidJson(error.to_string()))
}

/// Write one frame to `writer`.
pub async fn write<W>(writer: &mut W, frame: &Frame) -> Result<(), FrameError>
where
    W: AsyncWrite + Unpin,
{
    let encoded = encode(frame)?;
    writer.write_all(&encoded).await?;
    writer.flush().await?;
    Ok(())
}

/// Read exactly one frame from `reader`.
///
/// The length is checked against [`MAX_FRAME_BYTES`] **before** the body is
/// allocated, so an announced 4 GiB frame costs a typed error and no memory.
pub async fn read<R>(reader: &mut R) -> Result<Frame, FrameError>
where
    R: AsyncRead + Unpin,
{
    // `read_exact` cannot distinguish "no frame at all" from "half a prefix";
    // a frame boundary is exactly what `Closed` names.
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    let mut filled = 0;
    while filled < prefix.len() {
        match reader.read(&mut prefix[filled..]).await {
            Ok(0) if filled == 0 => return Err(FrameError::Closed),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(FrameError::Oversize {
            len: length,
            max: MAX_FRAME_BYTES,
        });
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(|error| FrameError::InvalidJson(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: Kind, id: u64) -> Frame {
        Frame {
            kind,
            id,
            payload: serde_json::json!({"echo": "payload", "id": id}),
        }
    }

    #[test]
    fn every_kind_round_trips() {
        for kind in [
            Kind::Challenge,
            Kind::Hello,
            Kind::Req,
            Kind::Res,
            Kind::Stream,
            Kind::Ping,
        ] {
            let original = frame(kind, 7);
            let encoded = encode(&original).expect("encode");
            let decoded = decode(&encoded).expect("decode");
            assert_eq!(decoded, original, "{kind:?} must round-trip");
        }
    }

    #[test]
    fn the_wire_form_is_length_prefixed_json() {
        // The expected field names and the four-byte big-endian prefix are
        // the contract other peers code against, so they are asserted from
        // outside the type, not derived from it.
        let original = frame(Kind::Ping, 1);
        let encoded = encode(&original).expect("encode");
        let body = &encoded[LENGTH_PREFIX_BYTES..];

        assert_eq!(
            u32::from_be_bytes(encoded[..LENGTH_PREFIX_BYTES].try_into().unwrap()) as usize,
            body.len()
        );
        let json: Value = serde_json::from_slice(body).expect("the body is JSON");
        assert_eq!(json["kind"], "ping");
        assert_eq!(json["id"], 1);
        assert_eq!(json["payload"]["echo"], "payload");
    }

    #[tokio::test]
    async fn an_oversize_frame_is_refused_before_its_body_is_read() {
        // The header alone says the body is over the cap. A reader that tried
        // to fill the announced body would report Truncated instead, because
        // no body follows — so this doubles as the before-allocation check.
        let announced = (MAX_FRAME_BYTES + 1) as u32;
        let bytes = announced.to_be_bytes();

        assert_eq!(
            read(&mut &bytes[..]).await,
            Err(FrameError::Oversize {
                len: MAX_FRAME_BYTES + 1,
                max: MAX_FRAME_BYTES
            })
        );
        assert_eq!(
            decode(&bytes),
            Err(FrameError::Oversize {
                len: MAX_FRAME_BYTES + 1,
                max: MAX_FRAME_BYTES
            })
        );
    }

    #[test]
    fn a_frame_over_the_cap_cannot_be_encoded() {
        let huge = Frame {
            kind: Kind::Stream,
            id: 9,
            payload: Value::String("x".repeat(MAX_FRAME_BYTES)),
        };
        assert!(matches!(encode(&huge), Err(FrameError::Oversize { .. })));
    }

    #[tokio::test]
    async fn an_empty_stream_is_a_clean_close() {
        let empty: &[u8] = &[];
        assert_eq!(read(&mut &empty[..]).await, Err(FrameError::Closed));
        assert_eq!(decode(empty), Err(FrameError::Closed));
    }

    #[tokio::test]
    async fn a_stream_that_ends_mid_length_prefix_is_truncated() {
        let partial: &[u8] = &[0, 0];
        assert_eq!(read(&mut &partial[..]).await, Err(FrameError::Truncated));
        assert_eq!(decode(partial), Err(FrameError::Truncated));
    }

    #[tokio::test]
    async fn a_truncated_frame_is_refused() {
        let encoded = encode(&frame(Kind::Ping, 3)).expect("encode");

        let mut missing_a_body_byte = &encoded[..encoded.len() - 1];
        assert_eq!(
            read(&mut missing_a_body_byte).await,
            Err(FrameError::Truncated)
        );

        let mut missing_the_header = &encoded[..LENGTH_PREFIX_BYTES - 1];
        assert_eq!(
            read(&mut missing_the_header).await,
            Err(FrameError::Truncated)
        );

        assert_eq!(
            decode(&encoded[..encoded.len() - 1]),
            Err(FrameError::Truncated)
        );
    }

    #[test]
    fn a_body_that_is_not_a_frame_is_refused() {
        let mut bytes = 2u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        assert!(matches!(decode(&bytes), Err(FrameError::InvalidJson(_))));
    }

    #[test]
    fn a_second_frame_after_the_first_is_not_silently_dropped() {
        let mut bytes = encode(&frame(Kind::Ping, 1)).expect("encode");
        bytes.extend_from_slice(&encode(&frame(Kind::Res, 1)).expect("encode"));
        assert_eq!(decode(&bytes), Err(FrameError::Trailing));
    }
}
