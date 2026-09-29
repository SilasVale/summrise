//! Execute an argv that arrived out of band.
//!
//! # Why this exists
//!
//! A remote subprocess seam promises its caller that **no shell layer exists** — every element of an
//! argv is passed through as an unquoted value, including values the model chose. SSH cannot honour
//! that promise on its own: an `exec` request carries a **string**, and the server hands that string
//! to the login shell. Quoting an argv into such a string is precisely the hazard the seam was built
//! to remove, and it fails on exactly the inputs that matter (spaces, quotes, `$(…)`, newlines).
//!
//! So the argv travels beside the command rather than inside it: this helper is what SSH executes,
//! it reads the real argv from its own stdin as a length-prefixed frame, and it `execve`s the target
//! with the caller's descriptors untouched. The exec string then contains only this helper's path
//! and fixed arguments — nothing the model supplied.
//!
//! # The frame
//!
//! Big-endian, read exactly and never buffered past its end:
//!
//! ```text
//! u32  count                       number of argv elements, 1..=MAX_ARGS
//! count × { u32 len; len bytes }   one element each, in order
//! ```
//!
//! Length-prefixed rather than NUL-terminated on purpose: an **empty** argv element is legal in
//! POSIX, and a `\0\0` terminator cannot be told apart from one.
//!
//! Anything after the frame belongs to the child: the helper stops reading at the frame's last byte,
//! so the target process inherits a descriptor positioned at its own stdin.

#![forbid(unsafe_op_in_unsafe_fn)]

use std::fmt;
use std::io::Read;

/// Largest argv this helper will accept. A frame claiming more is refused before allocating for it,
/// so a hostile or corrupt stream cannot ask for an unbounded vector.
pub const MAX_ARGS: u32 = 4096;

/// Largest single argv element, for the same reason.
pub const MAX_ARG_LEN: u32 = 1 << 20;

/// A malformed frame. Every variant is a statement about the bytes actually read.
#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The stream ended before the element count could be read.
    TruncatedCount,
    /// The count was zero — an argv always has at least `argv[0]`.
    EmptyArgv { count: u32 },
    /// The count exceeded [`MAX_ARGS`].
    TooManyArgs { count: u32 },
    /// A length prefix could not be read.
    TruncatedLength { index: u32 },
    /// An element's declared length exceeded [`MAX_ARG_LEN`].
    ArgTooLong { index: u32, len: u32 },
    /// The stream ended inside an element.
    TruncatedArg { index: u32, declared: u32, got: u32 },
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedCount => write!(f, "stream ended before the argv element count"),
            Self::EmptyArgv { count } => write!(
                f,
                "argv frame declares {count} elements; an argv needs at least one"
            ),
            Self::TooManyArgs { count } => write!(
                f,
                "argv frame declares {count} elements; the limit is {MAX_ARGS}"
            ),
            Self::TruncatedLength { index } => {
                write!(f, "stream ended before the length of element {index}")
            }
            Self::ArgTooLong { index, len } => {
                write!(
                    f,
                    "element {index} declares {len} bytes; the limit is {MAX_ARG_LEN}"
                )
            }
            Self::TruncatedArg {
                index,
                declared,
                got,
            } => {
                write!(
                    f,
                    "element {index} declares {declared} bytes but only {got} arrived"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}

fn read_exact_counted<R: Read>(r: &mut R, buf: &mut [u8]) -> u32 {
    // Returns how many bytes actually arrived. `read_exact` is deliberately NOT used here: a short
    // read must be reported as the number received, because that number is the whole point of the
    // diagnostic (it distinguishes "the stream ended" from "the frame lied").
    let mut got = 0usize;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    got as u32
}

/// Read one argv frame from `r`.
///
/// On success the reader is positioned at the first byte **after** the frame — which is the child's
/// stdin, not this helper's. No byte past the frame is consumed, and no buffering layer sits between
/// `r` and the caller, so that property holds for a pipe or a socket as well as for a slice.
pub fn read_frame<R: Read>(r: &mut R) -> Result<Vec<Vec<u8>>, FrameError> {
    let mut count_buf = [0u8; 4];
    if read_exact_counted(r, &mut count_buf) != 4 {
        return Err(FrameError::TruncatedCount);
    }
    let count = u32::from_be_bytes(count_buf);
    if count == 0 {
        return Err(FrameError::EmptyArgv { count });
    }
    if count > MAX_ARGS {
        return Err(FrameError::TooManyArgs { count });
    }

    let mut argv: Vec<Vec<u8>> = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut len_buf = [0u8; 4];
        if read_exact_counted(r, &mut len_buf) != 4 {
            return Err(FrameError::TruncatedLength { index });
        }
        let len = u32::from_be_bytes(len_buf);
        if len > MAX_ARG_LEN {
            return Err(FrameError::ArgTooLong { index, len });
        }
        let mut element = vec![0u8; len as usize];
        let got = read_exact_counted(r, &mut element);
        if got != len {
            return Err(FrameError::TruncatedArg {
                index,
                declared: len,
                got,
            });
        }
        argv.push(element);
    }
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a frame the way a caller would: count, then each element length-prefixed.
    pub fn frame(elements: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(elements.len() as u32).to_be_bytes());
        for e in elements {
            out.extend_from_slice(&(e.len() as u32).to_be_bytes());
            out.extend_from_slice(e);
        }
        out
    }

    #[test]
    fn reads_a_simple_argv() {
        let bytes = frame(&[b"/usr/bin/rg", b"--no-config", b"needle"]);
        let mut cursor: &[u8] = &bytes;
        let argv = read_frame(&mut cursor).expect("frame parses");
        assert_eq!(
            argv,
            vec![
                b"/usr/bin/rg".to_vec(),
                b"--no-config".to_vec(),
                b"needle".to_vec()
            ]
        );
    }

    #[test]
    fn an_empty_element_survives_the_round_trip() {
        // The reason the frame is length-prefixed: `""` is a legal argv element, and a NUL
        // terminator would read it as the end of the argv.
        let bytes = frame(&[b"prog", b"", b"tail"]);
        let mut cursor: &[u8] = &bytes;
        let argv = read_frame(&mut cursor).expect("frame parses");
        assert_eq!(argv, vec![b"prog".to_vec(), Vec::new(), b"tail".to_vec()]);
    }

    #[test]
    fn elements_that_would_need_quoting_are_passed_through_verbatim() {
        // The whole point. Under a shell these four are one argument or four, or a command
        // substitution; here they are four bytes-identical elements.
        let hostile: &[&[u8]] = &[b"prog", b"two words", b"$(id)", b"it's \"quoted\""];
        let bytes = frame(hostile);
        let mut cursor: &[u8] = &bytes;
        let argv = read_frame(&mut cursor).expect("frame parses");
        assert_eq!(argv[1], b"two words".to_vec());
        assert_eq!(argv[2], b"$(id)".to_vec());
        assert_eq!(argv[3], b"it's \"quoted\"".to_vec());
    }

    #[test]
    fn the_childs_stdin_is_left_untouched() {
        // Everything after the frame belongs to the target process, and the reader must stop
        // exactly at the frame's last byte to leave it there.
        let mut bytes = frame(&[b"prog", b"arg"]);
        let payload = b"the child's own stdin\n";
        bytes.extend_from_slice(payload);

        let mut cursor: &[u8] = &bytes;
        let argv = read_frame(&mut cursor).expect("frame parses");
        assert_eq!(argv, vec![b"prog".to_vec(), b"arg".to_vec()]);
        assert_eq!(cursor, payload, "the reader advanced past the frame");
    }

    #[test]
    fn a_zero_count_is_refused() {
        let bytes = 0u32.to_be_bytes();
        let mut cursor: &[u8] = &bytes;
        assert_eq!(
            read_frame(&mut cursor),
            Err(FrameError::EmptyArgv { count: 0 })
        );
    }

    #[test]
    fn a_count_above_the_limit_is_refused_before_allocating() {
        let bytes = (MAX_ARGS + 1).to_be_bytes();
        let mut cursor: &[u8] = &bytes;
        assert_eq!(
            read_frame(&mut cursor),
            Err(FrameError::TooManyArgs {
                count: MAX_ARGS + 1
            })
        );
    }

    #[test]
    fn a_truncated_count_is_reported_as_such() {
        let mut cursor: &[u8] = &[0u8, 0u8];
        assert_eq!(read_frame(&mut cursor), Err(FrameError::TruncatedCount));
    }

    #[test]
    fn a_truncated_length_is_reported_with_its_index() {
        let mut bytes = 2u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&3u32.to_be_bytes());
        bytes.extend_from_slice(b"abc");
        bytes.extend_from_slice(&[0u8, 0u8]); // half of element 1's length prefix
        let mut cursor: &[u8] = &bytes;
        assert_eq!(
            read_frame(&mut cursor),
            Err(FrameError::TruncatedLength { index: 1 })
        );
    }

    #[test]
    fn a_short_element_reports_declared_against_received() {
        let mut bytes = 1u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&10u32.to_be_bytes());
        bytes.extend_from_slice(b"only4");
        let mut cursor: &[u8] = &bytes;
        assert_eq!(
            read_frame(&mut cursor),
            Err(FrameError::TruncatedArg {
                index: 0,
                declared: 10,
                got: 5
            })
        );
    }

    #[test]
    fn an_element_above_the_limit_is_refused() {
        let mut bytes = 1u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(&(MAX_ARG_LEN + 1).to_be_bytes());
        let mut cursor: &[u8] = &bytes;
        assert_eq!(
            read_frame(&mut cursor),
            Err(FrameError::ArgTooLong {
                index: 0,
                len: MAX_ARG_LEN + 1
            })
        );
    }
}
