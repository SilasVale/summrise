//! Serial backend — raw byte I/O on a port taken from SerialPool (`terminal` feature).

use super::{TermBackend, TermOutput};
use crate::tools::serial::SerialPool;
use std::sync::Arc;
use vale_agent_core::DeviceError;

/// Serial link config (port + framing) — captured at open so an
/// auto-reconnect can reopen the SAME port with the SAME parameters
/// (P4b: unplug/reboot → session survives, port is re-opened when it
/// reappears).
#[derive(Clone)]
struct SerialCfg {
    port: String,
    baud: u32,
    data_bits: Option<u8>,
    parity: Option<String>,
    stop_bits: Option<u8>,
}

/// Open a port through the pool and return its (port, port_id). Blocking —
/// callers run this via spawn_blocking.
fn pool_open(
    pool: &Arc<SerialPool>,
    cfg: &SerialCfg,
) -> Result<(Box<dyn serialport::SerialPort>, String), DeviceError> {
    let (port_id, _) = pool.open(
        cfg.port.clone(),
        Some(cfg.baud),
        cfg.data_bits,
        cfg.parity.clone(),
        cfg.stop_bits,
    )?;
    match pool.borrow_port(&port_id) {
        Some(port) => Ok((port, port_id)),
        None => {
            pool.release_port(&port_id);
            Err(DeviceError::SerialPortNotOpen { id: port_id })
        }
    }
}

/// Reader-loop body: read one chunk (short timeout so close/reconnect
/// signals are checked frequently). Returns Err on a real I/O error.
fn read_chunk(port: &mut dyn serialport::SerialPort, buf: &mut [u8]) -> std::io::Result<Vec<u8>> {
    port.set_timeout(std::time::Duration::from_millis(50)).ok();
    match port.read(buf) {
        Ok(n) if n > 0 => Ok(buf[..n].to_vec()),
        Ok(_) => Ok(Vec::new()),
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// THE POOL ENTRY THIS SESSION HOLDS — a guard that FOLLOWS the session across
/// auto-reconnects, and releases the entry it currently holds when it is dropped or
/// explicitly released.
///
/// WHY IT EXISTS (round 259, measured on d1). `SerialPool::open` mints a NEW entry id on
/// every open, INCLUDING the reopen an auto-reconnect performs. The backend used to keep
/// the id it was born with, so the reconnect released the new entry nowhere: closing the
/// session called `release_port(stale_id)` — a no-op — and the live entry, WITH ITS OPEN
/// OS HANDLE, stayed in the pool for the life of the process. **The symptom is exactly
/// what an operator reported: a serial session that closed normally, a COM port that
/// stayed locked (`Access to the path 'COM4' is denied`) for every later open — ours and
/// every other program's — until the agent was restarted.**
///
/// The id therefore lives HERE, behind one lock, and both writers (the reader thread on a
/// reconnect, `close`) go through it: a reconnect REPOINTS the guard (releasing the old
/// entry), and closing RELEASES it immediately rather than waiting for every `Arc<dyn
/// TermBackend>` clone in flight to drop. Released is idempotent — `take()` makes the
/// second call a no-op — so the close path, the reader's exit and `Drop` can all run.
struct PoolEntry {
    pool: Arc<SerialPool>,
    id: Option<String>,
}

impl PoolEntry {
    fn new(pool: Arc<SerialPool>, id: String) -> Self {
        Self { pool, id: Some(id) }
    }
    /// Drop the entry we currently hold (idempotent).
    fn release(&mut self) {
        if let Some(id) = self.id.take() {
            self.pool.release_port(&id);
        }
    }
    /// Follow a reconnect: release the old entry, hold the new one.
    fn repoint(&mut self, new_id: String) {
        self.release();
        self.id = Some(new_id);
    }
}

impl Drop for PoolEntry {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct SerialBackend {
    write_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    close_tx: std::sync::mpsc::Sender<()>,
    /// The pool entry, shared with the reader thread (which repoints it on reconnect).
    /// `None` only if the pool was never acquired (a construction failure path).
    entry: Option<Arc<std::sync::Mutex<PoolEntry>>>,
}

impl SerialBackend {
    /// `data_bits`/`parity`/`stop_bits` override the target-string framing
    /// (round-54: serial framing like 8E1/7N2 was never reachable from
    /// terminal_open — SerialPool supported it, nothing passed it through).
    #[allow(clippy::too_many_arguments)]
    pub async fn open(
        pool: Arc<SerialPool>,
        target: &str,
        data_bits: Option<u8>,
        parity: Option<String>,
        stop_bits: Option<u8>,
        auto_reconnect: bool,
        tx: tokio::sync::mpsc::Sender<TermOutput>,
        sid: String,
    ) -> Result<Self, DeviceError> {
        let cfg = super::parse_serial_config(target);
        let port_name = cfg.port;
        let baud = cfg.baud;
        // Explicit parameters win over the target string's framing.
        let data_bits = data_bits.or(cfg.data_bits);
        let parity = parity.or(cfg.parity);
        let stop_bits = stop_bits.or(cfg.stop_bits);
        let link = SerialCfg {
            port: port_name.clone(),
            baud,
            data_bits,
            parity,
            stop_bits,
        };
        tracing::debug!("[vale-agent] Serial: opening {port_name} at {baud} baud (data_bits={:?} parity={:?} stop_bits={:?} auto_reconnect={})", link.data_bits, link.parity, link.stop_bits, auto_reconnect);

        // Open in pool, then BORROW the handle out (round-118: the old
        // take_port REMOVED the pool entry, defeating open()'s exclusivity
        // check — a second open of the same port passed and double-opened
        // the device). The pool entry stays as the guard; the session owns
        // its cloned handle and reads/writes without pool-lock contention.
        // release_port drops the entry on close.
        let (port, port_id) = {
            let pool = pool.clone();
            let link = link.clone();
            tokio::task::spawn_blocking(move || pool_open(&pool, &link))
                .await
                .map_err(|e| DeviceError::Internal {
                    message: format!("serial open task failed: {e}"),
                })?
        }?;
        let port = Arc::new(tokio::sync::Mutex::new(port));
        // The entry guard, shared with the reader thread so a reconnect can follow it.
        let entry = Arc::new(std::sync::Mutex::new(PoolEntry::new(pool.clone(), port_id)));

        // Bounded write queue (was unbounded — a stalled device could buffer
        // keystrokes without limit). sync_channel(1024) + try_send below drops
        // keystrokes when full, matching the documented channel policy.
        let (write_tx, write_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1024);
        let (close_tx, close_rx) = std::sync::mpsc::channel::<()>();
        // review #4: ONE shared slot for the live port handle. The old code
        // swapped only the reader's local Arc on auto-reconnect — the writer
        // thread kept the DEAD handle, its write errored, the loop broke,
        // and TX was permanently lost after any unplug/replug.
        let port_shared: std::sync::Arc<std::sync::Mutex<_>> =
            std::sync::Arc::new(std::sync::Mutex::new(port.clone()));

        // Reader thread — owns its own Arc clone, no pool lock needed.
        // P4b: with auto_reconnect, a read error (unplug / device reboot)
        // releases the pool entry and retries the SAME link config until the
        // port reappears; status lines are emitted so the user/AI sees the
        // session is alive and waiting.
        let pool_r = pool.clone();
        let link_r = link.clone();
        let tx_r = tx.clone();
        let sid_r = sid.clone();
        let entry_r = entry.clone();
        let port_shared_r = port_shared.clone();
        std::thread::spawn(move || {
            loop {
                if close_rx.try_recv().is_ok() {
                    break;
                }
                let cur = port_shared_r
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                let result = {
                    let mut p = cur.blocking_lock();
                    let mut buf = vec![0u8; 4096];
                    read_chunk(p.as_mut(), &mut buf)
                };
                match result {
                    Ok(data) if !data.is_empty() => match tx_r.blocking_send(TermOutput {
                        session_id: sid_r.clone(),
                        data,
                    }) {
                        Ok(()) => {}
                        Err(_) => break,
                    },
                    Ok(_) => {}
                    Err(_) if !auto_reconnect => break,
                    Err(_) => {
                        // Port died — try to reopen with the same config.
                        tracing::info!(
                            "[vale-agent] Serial {port_name}: link lost, auto-reconnecting…"
                        );
                        let _ = tx_r.blocking_send(TermOutput {
                            session_id: sid_r.clone(),
                            data: format!("\r\n\x1b[33m[serial] {port_name}: link lost — waiting for the port to reappear (auto-reconnect)…\x1b[0m\r\n").into_bytes(),
                        });
                        // Release the dead entry so open() can succeed again — through the
                        // guard, so the entry we later follow is the one we actually hold.
                        {
                            let e = entry_r.clone();
                            let _ = std::thread::spawn(move || {
                                e.lock().unwrap_or_else(|p| p.into_inner()).release()
                            })
                            .join();
                        }
                        // Retry loop until the port comes back or the session
                        // is closed. pool_open is synchronous (serialport open
                        // with short timeout) — direct call is fine.
                        let mut attempts = 0u32;
                        loop {
                            if close_rx.try_recv().is_ok() {
                                return;
                            }
                            match pool_open(&pool_r, &link_r) {
                                Ok((new_port, new_id)) => {
                                    tracing::info!(
                                        "[vale-agent] Serial {port_name}: reconnected (attempt {})",
                                        attempts + 1
                                    );
                                    let _ = tx_r.blocking_send(TermOutput {
                                        session_id: sid_r.clone(),
                                        data: format!("\r\n\x1b[32m[serial] {port_name}: reconnected\x1b[0m\r\n").into_bytes(),
                                    });
                                    *port_shared_r.lock().unwrap_or_else(|p| p.into_inner()) =
                                        Arc::new(tokio::sync::Mutex::new(new_port));
                                    // FOLLOW THE NEW ENTRY: the id this session holds is now
                                    // the reconnected one, so closing releases THAT, not the
                                    // dead id — the leak this guard exists to prevent.
                                    entry_r
                                        .lock()
                                        .unwrap_or_else(|p| p.into_inner())
                                        .repoint(new_id);
                                    break;
                                }
                                Err(_) => {
                                    attempts += 1;
                                    // Probe cadence: fast at first, then 2s.
                                    std::thread::sleep(std::time::Duration::from_millis(
                                        if attempts < 5 { 500 } else { 2000 },
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            tracing::debug!("[vale-agent] Serial reader ended: {sid_r}");
        });

        // Writer thread — direct byte write, no hex encoding. review #4:
        // resolves the CURRENT handle from the shared slot on every attempt
        // and retries a dropped write for ~10 s (the reconnect loop swaps in
        // a live handle meanwhile) instead of dying on first error.
        let port_shared_w = port_shared.clone();
        std::thread::spawn(move || {
            let mut pending: Option<Vec<u8>> = None;
            let mut attempts = 0u32;
            loop {
                let data = match pending.take() {
                    Some(d) => d,
                    None => match write_rx.recv() {
                        Ok(d) => d,
                        Err(_) => return, // backend dropped: session gone
                    },
                };
                let cur = port_shared_w
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                let ok = {
                    let mut p = cur.blocking_lock();
                    p.write_all(&data).is_ok() && p.flush().is_ok()
                };
                if ok {
                    attempts = 0;
                    continue;
                }
                attempts += 1;
                if attempts > 200 {
                    // ~10 s of dead port: drop THIS frame (matching the old
                    // break's outcome) but stay alive for the next one.
                    attempts = 0;
                    continue;
                }
                pending = Some(data);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        Ok(SerialBackend {
            write_tx,
            close_tx,
            entry: Some(entry),
        })
    }
}

impl TermBackend for SerialBackend {
    fn write(&self, data: &[u8]) {
        // try_send: drop-on-full (never block the caller on a stalled device)
        let _ = self.write_tx.try_send(data.to_vec());
    }
    fn write_async<'a>(
        &'a self,
        data: &'a [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
        // round-107: the spawn_blocking + timeout ABANDONED a blocked task
        // per timeout (thread pile-up) AND the queued command was delivered
        // to the device later (the caller was told it failed, then it ran —
        // possibly twice). try_send + a short bounded retry: a full channel
        // means the writer is mid-block; failing fast (without enqueueing)
        // is the honest answer.
        let tx = self.write_tx.clone();
        let d = data.to_vec();
        Box::pin(async move {
            for _ in 0..10 {
                if tx.try_send(d.clone()).is_ok() {
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
            Err("serial write timed out (device not draining)".into())
        })
    }
    fn resize(&self, _rows: u16, _cols: u16) {}
    fn close(&self) {
        let _ = self.close_tx.send(());
        // RELEASE NOW, not when the last Arc drops: an in-flight tool call may still be
        // holding a clone of this backend, and a port nobody is reading must not stay
        // locked against every other program on the device (round 259).
        if let Some(entry) = &self.entry {
            entry.lock().unwrap_or_else(|p| p.into_inner()).release();
        }
    }
    // A serial port has no process to abort — a timed-out write to a device
    // cannot be "killed". Nothing to do (the session stays open).
    fn terminate(&self) {}
}

impl Drop for SerialBackend {
    fn drop(&mut self) {
        let _ = self.close_tx.send(());
        // The guard releases whatever entry this session currently holds (round-118's rule,
        // now correct across reconnects — see `PoolEntry`).
        self.entry = None;
    }
}

#[cfg(test)]
mod tx_tests {
    use super::*;

    /// The serial TX path, proven to reach the WIRE (SOLID R130).
    ///
    /// WHY THIS EXISTS: `serial` had no round-trip coverage at all — the only
    /// tests were `serial_pool_new` and "list_ports does not panic", neither of
    /// which proves a byte can leave the device. That gap is why a report of
    /// "serial can't input" could not be answered from the suite.
    ///
    /// It needs a real tty: the test opens `VALE_TEST_SERIAL_PORT` through the
    /// PRODUCTION path (`SerialBackend::open` → `write_async`, exactly what
    /// `terminal_write` drives) and the caller verifies the bytes arrive on the
    /// other end. Skips when the env var is unset, so CI is unaffected.
    ///
    /// RUN IT (creates a pty pair, runs this test, checks the master side):
    ///
    /// ```text
    /// python3 - <<'EOF'
    /// import pty, os, subprocess, threading, time
    /// m, s = pty.openpty(); sp = os.ttyname(s); got = []
    /// def rd():
    ///     os.set_blocking(m, False); end = time.time() + 12
    ///     while time.time() < end:
    ///         try:
    ///             d = os.read(m, 4096)
    ///             if d:
    ///                 got.append(d)
    ///                 if b"TX-PROBE" in b"".join(got): return
    ///         except BlockingIOError: time.sleep(0.05)
    /// t = threading.Thread(target=rd); t.start()
    /// env = dict(os.environ, VALE_TEST_SERIAL_PORT=sp)
    /// env.pop("CC", None); env.pop("CXX", None)
    /// subprocess.run(["cargo","test","--features","terminal,keyring","--lib","--",
    ///                 "serial_tx_reaches_the_wire","--nocapture","--test-threads=1"],
    ///                cwd="agent", env=env)
    /// t.join(timeout=3)
    /// print("WIRE:", b"".join(got))
    /// EOF
    /// ```
    ///
    /// A CLOSED SERIAL SESSION MUST RELEASE THE PORT IT HOLDS (round 259).
    ///
    /// Measured on d1 before this test existed: an operator's serial session closed
    /// normally (its audit trail ends with `status: closed`) and COM4 stayed locked —
    /// `Access to the path 'COM4' is denied` for every later open, OURS AND EVERY OTHER
    /// PROGRAM'S — until the agent process was restarted. Two defects produced that, and
    /// this test covers the first: the entry guard released the id the session was BORN
    /// with, so an entry minted by an auto-reconnect was never released at all.
    ///
    /// Needs a real tty (`VALE_TEST_SERIAL_PORT`, the harness in the file header), because
    /// the pool's entries ARE OS handles; skips without one, so CI is unaffected.
    #[tokio::test]
    async fn a_serial_session_releases_its_pool_entry_on_close() {
        let Ok(port) = std::env::var("VALE_TEST_SERIAL_PORT") else {
            return; // no tty provided — skip (CI)
        };
        let pool = std::sync::Arc::new(SerialPool::new(115200, 200));
        let (tx, _rx) = tokio::sync::mpsc::channel::<TermOutput>(64);
        let be = SerialBackend::open(
            pool.clone(),
            &port,
            None,
            None,
            None,
            false,
            tx,
            "close-probe".into(),
        )
        .await
        .expect("opening the provided tty must succeed");
        assert_eq!(
            pool.list_open_ports().len(),
            1,
            "an open session holds exactly one pool entry"
        );
        be.close();
        // The entry is released by close itself, NOT by the Arc count reaching zero: a tool
        // call in flight may still hold a clone of this backend, and the port must be free
        // the moment the session is closed.
        assert!(
            pool.list_open_ports().is_empty(),
            "closing the session must release the pool entry: {:?}",
            pool.list_open_ports()
        );
    }

    /// THE RECONNECT HALF: `SerialPool::open` mints a NEW entry id on every open, including
    /// the reopen an auto-reconnect performs — so the guard must FOLLOW the new id. Before
    /// round 259 the backend kept the id it was born with, `release_port(stale_id)` was a
    /// no-op, and the entry minted by the reconnect (with its open handle) stayed in the
    /// pool for the life of the process: the d1 symptom, exactly.
    ///
    /// Driven through the real pool, not a fake: the property under test IS about the ids
    /// real opens mint. Needs a tty, so it skips without one like its sibling above.
    #[tokio::test]
    async fn the_entry_guard_follows_a_reconnect_to_the_new_entry() {
        let Ok(port) = std::env::var("VALE_TEST_SERIAL_PORT") else {
            return; // no tty provided — skip (CI)
        };
        let pool = std::sync::Arc::new(SerialPool::new(115200, 200));
        // The entry a session is born with. BOUNDED RETRY: the harness reuses ONE tty for
        // every test in this module, and a previous test's reader thread holds the handle for
        // up to its 50 ms read window after that session closed — so a single attempt would
        // measure the harness's timing, not this test's subject.
        let opened =
            (0..40).find_map(
                |_| match pool.open(port.clone(), Some(115200), None, None, None) {
                    Ok(v) => Some(v),
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        None
                    }
                },
            );
        let (first_id, _) = opened.expect("the tty must become openable within 2 s");
        let guard = std::sync::Arc::new(std::sync::Mutex::new(PoolEntry::new(
            pool.clone(),
            first_id,
        )));
        assert_eq!(pool.list_open_ports().len(), 1);
        // The reconnect: the old entry is released, a NEW one is minted and held.
        pool.release_port(&pool.list_open_ports()[0].id.clone());
        let (second_id, _) = pool
            .open(port.clone(), Some(115200), None, None, None)
            .expect("reopen");
        guard.lock().unwrap().repoint(second_id.clone());
        let open = pool.list_open_ports();
        assert_eq!(
            open.len(),
            1,
            "exactly the reconnected entry is held: {open:?}"
        );
        assert_eq!(open[0].id, second_id, "and it is the NEW one: {open:?}");
        // And closing releases that one — the leak this pins.
        drop(guard);
        assert!(
            pool.list_open_ports().is_empty(),
            "the guard must release the entry it followed: {:?}",
            pool.list_open_ports()
        );
    }

    /// THE WAIT AT THE END IS LOAD-BEARING, and it is a lesson from writing
    /// this: the writer is a separate std thread, so a test that writes and
    /// returns immediately exits the process before the frame is sent — which
    /// looks exactly like a silently-dropped write. The first version of this
    /// test "reproduced" a TX bug that did not exist for that reason.
    #[tokio::test]
    async fn serial_tx_reaches_the_wire() {
        let Ok(port) = std::env::var("VALE_TEST_SERIAL_PORT") else {
            return; // no tty provided — skip (CI)
        };
        let pool = std::sync::Arc::new(SerialPool::new(115200, 200));
        let (tx, _rx) = tokio::sync::mpsc::channel::<TermOutput>(64);
        let be = SerialBackend::open(pool, &port, None, None, None, false, tx, "tx-probe".into())
            .await
            .expect("opening the provided tty must succeed");
        be.write_async(b"TX-PROBE-12345\n")
            .await
            .expect("write_async must accept the frame");
        // Let the writer thread actually run before the process exits.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
}
