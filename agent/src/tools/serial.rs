//! Serial port pool — enumeration and port lifecycle.
//!
//! Terminal sessions take ports out of the pool via `take_port` and do raw
//! byte I/O on their own threads (no pool lock contention). The hex-encoded
//! read/write API that used to live here is gone — MCP speaks bytes directly.
//!
//! The pool's internal lock is a plain `std::sync::Mutex` — critical sections
//! are microseconds. Blocking work (device enumeration, port open) happens in
//! the callers' `spawn_blocking`, never under the lock.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use summrise_agent_core::{recover_guard, DeviceError};

/// Monotonic port-id counter (uuid was overkill for session labels).
static NEXT_PORT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
pub struct SerialPortInfo {
    /// OS port name, e.g. /dev/ttyUSB0 or COM3.
    pub port_name: String,
    /// Hardware description if available.
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenPortInfo {
    pub id: String,
    pub port_name: String,
    pub baud_rate: u32,
}

// open()/take_port() only have callers in terminal-feature builds; the struct/fields
// exist in both so list_open_ports() works headless.
#[cfg_attr(not(feature = "terminal"), allow(dead_code))]
struct OpenPort {
    port: Box<dyn serialport::SerialPort>,
    port_name: String,
    baud_rate: u32,
}

pub struct SerialPool {
    ports: Mutex<HashMap<String, OpenPort>>,
    default_baud_rate: u32,
    default_timeout: Duration,
}

/// WHICH FAILURE IS THIS? A port the OS lists but will not open is BUSY (another program, or
/// a Summrise session whose handle outlived it); a port it does not list is MISSING. Both facts
/// are handed in, so the rule is pure and unit-pinned — see the call site for why presence,
/// and not the error kind or its text, is what decides.
#[cfg_attr(not(feature = "terminal"), allow(dead_code))]
fn classify_open_failure(port_name: &str, present: bool, os_error: &str) -> DeviceError {
    if present {
        DeviceError::Internal {
            message: format!(
                "{port_name} is in use by another program or an open Summrise session \
                 ({os_error}) — close that program or session and retry"
            ),
        }
    } else {
        DeviceError::SerialPortNotFound {
            port: format!("{port_name}: {os_error}"),
        }
    }
}

impl SerialPool {
    pub fn new(default_baud_rate: u32, default_timeout_ms: u64) -> Self {
        Self {
            ports: Mutex::new(HashMap::new()),
            default_baud_rate,
            default_timeout: Duration::from_millis(default_timeout_ms),
        }
    }

    /// Enumerate available serial ports. Blocking (device enumeration) —
    /// callers should run this via `spawn_blocking`.
    pub fn list_ports(&self) -> Result<Vec<SerialPortInfo>, DeviceError> {
        let ports = serialport::available_ports().map_err(|e| DeviceError::Internal {
            message: e.to_string(),
        })?;
        Ok(ports
            .into_iter()
            .map(|p| SerialPortInfo {
                port_name: p.port_name,
                description: None,
            })
            .collect())
    }

    /// Open a port and register it in the pool. Returns (port_id, baud_rate).
    /// Desktop terminal sessions immediately `take_port` it for exclusive use.
    /// Blocking (serialport open can stall on flaky hardware) — callers should
    /// run this via `spawn_blocking`.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    pub fn open(
        &self,
        port_name: String,
        baud_rate: Option<u32>,
        data_bits: Option<u8>,
        parity: Option<String>,
        stop_bits: Option<u8>,
    ) -> Result<(String, u32), DeviceError> {
        let baud = baud_rate.unwrap_or(self.default_baud_rate);

        let mut builder = serialport::new(&port_name, baud).timeout(self.default_timeout);

        // round-118: invalid framing values previously SILENTLY defaulted
        // to 8N1 — a typo'd parity/data_bits/stop_bits opened the port with
        // the wrong wire format and every frame was corrupted with no error
        // (on a device needing 7E1, the exact channel round-54 built for
        // wire-format correctness). Reject unrecognized values instead.
        if let Some(db) = data_bits {
            let db = match db {
                5 => serialport::DataBits::Five,
                6 => serialport::DataBits::Six,
                7 => serialport::DataBits::Seven,
                8 => serialport::DataBits::Eight,
                _ => {
                    return Err(DeviceError::Internal {
                        message: format!("invalid data_bits: {db} (5/6/7/8)"),
                    })
                }
            };
            builder = builder.data_bits(db);
        }

        // Parity
        if let Some(ref p) = parity {
            let parity = match p.to_lowercase().as_str() {
                "odd" => serialport::Parity::Odd,
                "even" => serialport::Parity::Even,
                "none" => serialport::Parity::None,
                _ => {
                    return Err(DeviceError::Internal {
                        message: format!("invalid parity: {p} (odd/even/none)"),
                    })
                }
            };
            builder = builder.parity(parity);
        }

        // Stop bits
        if let Some(sb) = stop_bits {
            let sb = match sb {
                1 => serialport::StopBits::One,
                2 => serialport::StopBits::Two,
                _ => {
                    return Err(DeviceError::Internal {
                        message: format!("invalid stop_bits: {sb} (1/2)"),
                    })
                }
            };
            builder = builder.stop_bits(sb);
        }

        // round-87: no per-port exclusivity — two opens of the same port
        // both succeeded (Linux: interleaved/garbled reads + termios races;
        // Windows: misleading "not found" for what is really an in-use port).
        // Reject a second open of an already-open port.
        {
            let guard = recover_guard(&self.ports);
            if guard.values().any(|p| p.port_name == port_name) {
                return Err(DeviceError::Internal {
                    message: format!(
                        "{port_name} is already open in another Summrise session — use that \
                         session, or close it and retry"
                    ),
                });
            }
        }

        // AN IN-USE PORT IS NOT A MISSING ONE, and the OS message is the only thing that
        // knows the difference: `serialport` folds ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND
        // and ERROR_ACCESS_DENIED into ONE kind (`ErrorKind::NoDevice`, windows/error.rs), so
        // a kind-based classification cannot work and the message is localised (`拒绝访问`).
        // THE DISCRIMINATOR IS THEREFORE PRESENCE: a port the OS still LISTS but will not
        // open is busy; one it does not list is missing. Measured on d1 (round 259): the
        // leaked handle produced "Serial port not found: COM4: 拒绝访问" while `COM4` was
        // listed by Windows and simply held — an operator sent looking for a cable that was
        // plugged in.
        let port = builder.open().map_err(|e| {
            let present = serialport::available_ports()
                .map(|ps| {
                    ps.iter()
                        .any(|p| p.port_name.eq_ignore_ascii_case(&port_name))
                })
                .unwrap_or(false);
            classify_open_failure(&port_name, present, &e.to_string())
        })?;

        // round-99: the exclusivity check above was check-then-act — two
        // concurrent opens of the same port BOTH passed the check (lock
        // released before the blocking open), then the loser's insert
        // clobbered the winner's entry, and the loser returned a port it no
        // longer owned. Re-check UNDER THE LOCK after the open: the loser
        // closes its just-opened handle and reports the real "in use" error
        // (what round-87 intended).
        let id = format!("port-{}", NEXT_PORT_ID.fetch_add(1, Ordering::Relaxed));
        {
            let mut guard = recover_guard(&self.ports);
            if guard.values().any(|p| p.port_name == port_name) {
                drop(port);
                return Err(DeviceError::Internal {
                    message: format!(
                        "{port_name} is already open in another Summrise session — use that \
                         session, or close it and retry"
                    ),
                });
            }
            guard.insert(
                id.clone(),
                OpenPort {
                    port,
                    port_name: port_name.clone(),
                    baud_rate: baud,
                },
            );
        }

        Ok((id, baud))
    }

    /// Borrow a port for exclusive use WITHOUT removing the pool entry.
    /// round-118: the old take_port REMOVED the entry — the exclusivity
    /// checks in open() (scan the ports map) then saw an empty map and a
    /// second open of the same port passed, double-opening the device
    /// (Linux: interleaved reads + termios clobber; Windows: misleading
    /// "not found" for an in-use port). Keeping the entry makes the map the
    /// single source of exclusivity; the session holds its own handle and
    /// reads/writes without pool-lock contention (the lock is only held to
    /// clone the handle). close() calls release_port to drop the entry.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    pub fn borrow_port(&self, port_id: &str) -> Option<Box<dyn serialport::SerialPort>> {
        // OpenPort holds Box<dyn SerialPort>; hand out a CLONE (the session
        // owns its handle from here — the pool entry stays as the
        // exclusivity guard, see round-118 note above).
        recover_guard(&self.ports)
            .get(port_id)
            .and_then(|entry| entry.port.try_clone().ok())
    }

    /// Drop a port's pool entry (session closed). Idempotent.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    pub fn release_port(&self, port_id: &str) {
        recover_guard(&self.ports).remove(port_id);
    }

    pub fn list_open_ports(&self) -> Vec<OpenPortInfo> {
        recover_guard(&self.ports)
            .iter()
            .map(|(id, p)| OpenPortInfo {
                id: id.clone(),
                port_name: p.port_name.clone(),
                baud_rate: p.baud_rate,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE MESSAGE AN OPERATOR READS WHEN A PORT WILL NOT OPEN (round 259). d1's report was
    /// "COM4 拒绝访问", and what the agent said was **"Serial port not found: COM4: 拒绝访问"** —
    /// a missing-cable claim about a cable that was plugged in and merely held. The two cases
    /// are told apart by PRESENCE, because `serialport` folds ERROR_FILE_NOT_FOUND and
    /// ERROR_ACCESS_DENIED into ONE kind and the message text is localised.
    #[test]
    fn an_in_use_port_is_not_reported_as_a_missing_one() {
        let busy = classify_open_failure("COM4", true, "拒绝访问。").to_string();
        assert!(busy.contains("in use"), "{busy}");
        assert!(!busy.contains("not found"), "{busy}");

        let missing =
            classify_open_failure("COM2", false, "The system cannot find the file").to_string();
        assert!(missing.contains("not found"), "{missing}");
        assert!(!missing.contains("in use"), "{missing}");
    }
}
