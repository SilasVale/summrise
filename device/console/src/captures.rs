//! The recent screen captures, in memory only.
//!
//! A screenshot is evidence a person wants to see, not a sentence to read: the activity row names a
//! path, and this store is what answers it. It is deliberately not a file. The audit index must never
//! carry a body, and a restart forgetting every image is the honest behaviour of a console that did
//! not promise to keep one.
//!
//! The store is bounded twice — four frames per device and thirty-two in total — and evicts the least
//! recently used frame first. Reading a frame counts as using it, so an operator who keeps one row
//! open keeps its picture alive while older captures fall away.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::auth::random_32;
use crate::hex;

/// The most frames one device may keep.
const PER_DEVICE: usize = 4;

/// The most frames every device together may keep.
const TOTAL: usize = 32;

/// The eight bytes every PNG starts with.
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// One remembered frame. The bytes are shared, not copied per read.
struct Shot {
    /// The short random id the event's path names.
    id: String,
    device_id: String,
    png: Arc<[u8]>,
}

/// The frames, least recently used first.
pub(crate) struct Captures {
    shots: Mutex<VecDeque<Shot>>,
}

impl Captures {
    pub(crate) fn new() -> Self {
        Self {
            shots: Mutex::new(VecDeque::new()),
        }
    }

    /// Remember one frame and return the id its route names.
    ///
    /// `encoded` is the device's `screen.capture` base64. It is decoded and checked for the PNG
    /// signature here, before it has an id or a URL: a device answer that is not a PNG is not
    /// evidence, and the caller publishes no `shot` for it.
    pub(crate) fn remember(&self, device_id: &str, encoded: &str) -> Option<String> {
        let png = decode_png(encoded)?;
        let mut shots = lock(&self.shots);
        while shots
            .iter()
            .filter(|shot| shot.device_id == device_id)
            .count()
            >= PER_DEVICE
        {
            if !evict_oldest_for(&mut shots, device_id) {
                break;
            }
        }
        while shots.len() >= TOTAL {
            shots.pop_front();
        }
        let id = fresh_id(&shots);
        shots.push_back(Shot {
            id: id.clone(),
            device_id: device_id.to_owned(),
            png: Arc::from(png),
        });
        Some(id)
    }

    /// The exact bytes behind one id, or `None` when it was never stored, has been evicted, or
    /// belongs to another device. A hit is the newest use, which is what keeps it from being next.
    pub(crate) fn get(&self, device_id: &str, id: &str) -> Option<Arc<[u8]>> {
        let mut shots = lock(&self.shots);
        let index = shots
            .iter()
            .position(|shot| shot.id == id && shot.device_id == device_id)?;
        let shot = shots.remove(index)?;
        let png = Arc::clone(&shot.png);
        shots.push_back(shot);
        Some(png)
    }
}

/// The console's mutexes do not let a panic in one request poison the whole store: the frames are
/// plain data and the value behind a poisoned lock is still valid.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Drop the least recently used frame this device owns; `false` when it owns none.
fn evict_oldest_for(shots: &mut VecDeque<Shot>, device_id: &str) -> bool {
    let Some(index) = shots.iter().position(|shot| shot.device_id == device_id) else {
        return false;
    };
    shots.remove(index);
    true
}

/// A short id from the OS CSPRNG: eight bytes, sixteen hex characters. A collision is not a security
/// event, so retry the one-in-2^64 case instead of assuming it away.
fn fresh_id(shots: &VecDeque<Shot>) -> String {
    loop {
        let id = hex::encode(&random_32()[..8]);
        if !shots.iter().any(|shot| shot.id == id) {
            return id;
        }
    }
}

/// Decode the device's base64 and insist on the PNG signature.
fn decode_png(encoded: &str) -> Option<Vec<u8>> {
    let bytes = decode_base64(encoded)?;
    bytes.starts_with(PNG_SIGNATURE).then_some(bytes)
}

/// Standard, padded base64: what the device's `screen.capture` encodes with.
///
/// The workspace already carries a base64 crate for the device's own surfaces; the console needs
/// exactly one direction and one alphabet, so the dozen lines that decode it live here instead of a
/// new dependency.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let chunks = bytes.len() / 4;
    let mut out = Vec::with_capacity(chunks * 3);
    for (index, chunk) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let last = index + 1 == chunks;
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (slot, byte) in chunk.iter().copied().enumerate() {
            values[slot] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                _ => return None,
            };
        }
        // At most two characters are padding, and only the final chunk may carry any: `=AAA` and
        // `AA=A` are not base64, and neither is a `=` in the middle of a screen-long answer.
        if padding > 2 || (padding > 0 && !last) || chunk[..4 - padding].contains(&b'=') {
            return None;
        }
        out.push((values[0] << 2) | (values[1] >> 4));
        if padding < 2 {
            out.push((values[1] << 4) | (values[2] >> 2));
        }
        if padding < 1 {
            out.push((values[2] << 6) | values[3]);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 PNG: 68 bytes, three valid chunks.
    const ONE_PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";

    #[test]
    fn a_png_is_stored_and_served_byte_for_byte() {
        let captures = Captures::new();
        let id = captures
            .remember("device-a", ONE_PIXEL)
            .expect("a real PNG is evidence");
        assert_eq!(id.len(), 16);
        let bytes = captures.get("device-a", &id).expect("the frame is there");
        assert_eq!(&*bytes, decode_base64(ONE_PIXEL).unwrap().as_slice());
    }

    #[test]
    fn a_shot_is_scoped_to_the_device_that_took_it() {
        let captures = Captures::new();
        let id = captures.remember("device-a", ONE_PIXEL).unwrap();
        assert!(captures.get("device-b", &id).is_none());
    }

    #[test]
    fn something_that_is_not_a_png_is_not_remembered() {
        let captures = Captures::new();
        // Valid base64, not a PNG.
        assert_eq!(captures.remember("device-a", "QUJD"), None);
        // A PNG-shaped prefix is not enough if the base64 is not base64.
        assert_eq!(captures.remember("device-a", "not base64"), None);
        assert_eq!(captures.remember("device-a", ""), None);
        // Padding in the middle of a chunk is not base64 either.
        assert_eq!(captures.remember("device-a", "AA=A"), None);
    }

    #[test]
    fn five_captures_on_one_device_keep_the_four_newest() {
        let captures = Captures::new();
        let ids: Vec<String> = (0..5)
            .map(|_| {
                captures
                    .remember("device-a", ONE_PIXEL)
                    .expect("every capture is a PNG")
            })
            .collect();
        assert!(captures.get("device-a", &ids[0]).is_none());
        for id in &ids[1..] {
            assert!(captures.get("device-a", id).is_some(), "{id}");
        }
    }

    #[test]
    fn reading_a_frame_makes_it_the_newest_not_the_next_evicted() {
        let captures = Captures::new();
        let ids: Vec<String> = (0..4)
            .map(|_| captures.remember("device-a", ONE_PIXEL).unwrap())
            .collect();
        // Touch the oldest, then add a fifth: the untouched second frame is now the oldest.
        assert!(captures.get("device-a", &ids[0]).is_some());
        let fifth = captures.remember("device-a", ONE_PIXEL).unwrap();
        assert!(captures.get("device-a", &ids[1]).is_none());
        assert!(captures.get("device-a", &ids[0]).is_some());
        assert!(captures.get("device-a", &fifth).is_some());
    }

    #[test]
    fn the_whole_store_never_holds_more_than_thirty_two_frames() {
        let captures = Captures::new();
        let first = captures.remember("device-0", ONE_PIXEL).unwrap();
        for device in 0..8 {
            for frame in 0..4 {
                if device == 0 && frame == 0 {
                    continue;
                }
                captures
                    .remember(&format!("device-{device}"), ONE_PIXEL)
                    .unwrap();
            }
        }
        // The thirty-third evicts the oldest, which is the first frame: it has not been read since
        // it was stored, while every frame after it is more recent.
        let newest = captures.remember("device-8", ONE_PIXEL).unwrap();
        assert!(captures.get("device-0", &first).is_none());
        assert!(captures.get("device-8", &newest).is_some());
    }
}
