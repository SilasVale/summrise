//! The system capability: what this machine is running, and whether it can reach a host.
//!
//! These are the two questions an operator asks a device that misbehaves — "what is eating this
//! machine" and "can it actually reach the thing it says it cannot" — and both are answered from the
//! device's own kernel rather than by asking another machine.
//!
//! **Processes are the platform's, so there are two implementations and one contract.** The list is
//! a snapshot: pid, executable name, resident memory in KiB. A process the platform will not let us
//! open reports `0` memory rather than being omitted, because a list that hides a protected process
//! answers a different question than "what is running".
//!
//! **Killing is a decision with a blast radius**, so the caller names exactly one target — a pid or
//! an exact name — and the result says which pids were signalled and which could not be. The
//! device's own process is refused by name: a tool that can end the caller should not be a way to
//! end the device by accident.
//!
//! **The network test is TCP, not ICMP.** A raw socket needs elevation the device does not have and
//! should not ask for; a TCP connect is the question that matters to a device ("can I reach this
//! port") and it is the same probe `monitor` makes, which means a failure is comparable across the
//! two rather than being two different kinds of "down". A name that does not resolve is an error; a
//! connection that is refused is data — the addresses resolved and the reason it failed.

use std::io;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// The most processes one listing returns. A desktop with more is unusual; a list is for a person.
pub const MAX_PROCESSES: usize = 4096;

/// The longest a `system.net_test` may wait: one port, not a sweep.
pub const MAX_TIMEOUT_MS: u64 = 60_000;

/// One process, as the device can see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    /// The process id.
    pub pid: u32,
    /// The executable's file name; on Unix the kernel's comm, which is at most 15 bytes.
    pub name: String,
    /// Resident memory in KiB, or `0` when the platform refused to report it.
    pub memory_kb: u64,
}

/// What a kill did, target by target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KillReport {
    /// The pids that were signalled successfully.
    pub killed: Vec<u32>,
    /// The pids that were not, with the platform's reason.
    pub failed: Vec<(u32, String)>,
}

/// The facts a TCP test established.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetReport {
    /// Every address the name resolved to, in the resolver's order.
    pub addresses: Vec<String>,
    /// The address the connection reached, or `None` when none answered.
    pub connected: Option<String>,
    /// How long the successful connection took, in milliseconds.
    pub latency_ms: Option<u64>,
    /// Why the last attempt failed, when nothing connected.
    pub error: Option<String>,
}

/// Why a system call was refused or failed.
#[derive(Debug)]
pub enum SystemError {
    /// Neither a pid nor a name was given.
    NoTarget,
    /// Both a pid and a name were given; one target, one answer.
    TwoTargets,
    /// No running process has that exact name.
    NoMatch(String),
    /// The name resolved to no address at all.
    Unresolved(String),
    /// The host is empty or not a name/IP.
    BadHost(String),
    /// The platform refused the call.
    Io(io::Error),
}

impl std::fmt::Display for SystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemError::NoTarget => f.write_str("name a pid or a process name"),
            SystemError::TwoTargets => f.write_str("name a pid or a process name, not both"),
            SystemError::NoMatch(name) => write!(f, "no running process is named {name}"),
            SystemError::Unresolved(host) => write!(f, "{host} did not resolve to an address"),
            SystemError::BadHost(host) => write!(f, "{host:?} is not a host name or address"),
            SystemError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl From<io::Error> for SystemError {
    fn from(error: io::Error) -> SystemError {
        SystemError::Io(error)
    }
}

/// Running processes, filtered by a case-insensitive substring of the executable name.
pub fn processes(filter: Option<&str>) -> Result<Vec<Process>, SystemError> {
    let needle = filter
        .map(str::trim)
        .filter(|needle| !needle.is_empty())
        .map(str::to_lowercase);
    let mut found = platform::processes()?;
    if let Some(needle) = needle {
        found.retain(|process| process.name.to_lowercase().contains(&needle));
    }
    found.sort_by_key(|process| process.pid);
    found.truncate(MAX_PROCESSES);
    Ok(found)
}

/// End processes by pid or by exact name (case-insensitive).
pub fn kill(pid: Option<u32>, name: Option<&str>) -> Result<KillReport, SystemError> {
    let name = name.map(str::trim).filter(|name| !name.is_empty());
    let targets = match (pid, name) {
        (Some(pid), None) => vec![pid],
        (None, Some(name)) => {
            let matches: Vec<u32> = platform::processes()?
                .into_iter()
                .filter(|process| process.name.eq_ignore_ascii_case(name))
                .map(|process| process.pid)
                .collect();
            if matches.is_empty() {
                return Err(SystemError::NoMatch(name.to_owned()));
            }
            matches
        }
        (None, None) => return Err(SystemError::NoTarget),
        (Some(_), Some(_)) => return Err(SystemError::TwoTargets),
    };

    let mut report = KillReport {
        killed: Vec::new(),
        failed: Vec::new(),
    };
    for pid in targets {
        match platform::kill(pid) {
            Ok(()) => report.killed.push(pid),
            Err(reason) => report.failed.push((pid, reason)),
        }
    }
    Ok(report)
}

/// Resolve `host` and connect to `port`, waiting at most `timeout` in total.
pub fn net_test(host: &str, port: u16, timeout: Duration) -> Result<NetReport, SystemError> {
    let host = host.trim();
    if host.is_empty() {
        return Err(SystemError::BadHost(host.to_owned()));
    }
    let started = Instant::now();
    let addresses: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| SystemError::Unresolved(host.to_owned()))?
        .collect();
    if addresses.is_empty() {
        return Err(SystemError::Unresolved(host.to_owned()));
    }

    let mut last_error = None;
    for address in &addresses {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(address, remaining) {
            Ok(_stream) => {
                return Ok(NetReport {
                    addresses: addresses.iter().map(SocketAddr::to_string).collect(),
                    connected: Some(address.to_string()),
                    latency_ms: Some(started.elapsed().as_millis() as u64),
                    error: None,
                })
            }
            Err(error) => last_error = Some(error.to_string()),
        }
    }

    Ok(NetReport {
        addresses: addresses.iter().map(SocketAddr::to_string).collect(),
        connected: None,
        latency_ms: None,
        error: last_error.or_else(|| Some("the timeout expired before an attempt".to_owned())),
    })
}

/// The platform half: one implementation per target, both safe (`nix` on Unix, `summrise-winsys`
/// on Windows), so this module stays inside the crate's `#![forbid(unsafe_code)]`.
#[cfg(unix)]
mod platform {
    use super::{Process, SystemError};

    /// `/proc/<pid>/stat`, the field order the kernel documents: the comm sits in parentheses and
    /// the resident set size is field 24 (1-based) — index 21 after the closing parenthesis.
    pub(super) fn processes() -> Result<Vec<Process>, SystemError> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc").map_err(SystemError::Io)? {
            let entry = entry.map_err(SystemError::Io)?;
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|name| name.parse::<u32>().ok())
            else {
                continue;
            };
            if let Some(process) = one(pid) {
                found.push(process);
            }
        }
        Ok(found)
    }

    fn one(pid: u32) -> Option<Process> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat[open + 1..close].to_owned();
        let fields: Vec<&str> = stat[close + 2..].split_whitespace().collect();
        let resident_pages: u64 = fields.get(21)?.parse().ok()?;
        Some(Process {
            pid,
            name,
            // Linux's page is 4 KiB; the field is in pages, and KiB is what an operator reads.
            memory_kb: resident_pages * 4,
        })
    }

    pub(super) fn kill(pid: u32) -> Result<(), String> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        if pid == std::process::id() {
            return Err("the device's own process".to_owned());
        }
        kill(Pid::from_raw(pid as i32), Signal::SIGKILL).map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
mod platform {
    use super::{Process, SystemError};

    pub(super) fn processes() -> Result<Vec<Process>, SystemError> {
        let found = summrise_winsys::processes().map_err(SystemError::Io)?;
        Ok(found
            .into_iter()
            .map(|process| Process {
                pid: process.pid,
                name: process.name,
                memory_kb: process.memory_kb,
            })
            .collect())
    }

    pub(super) fn kill(pid: u32) -> Result<(), String> {
        if pid == std::process::id() {
            return Err("the device's own process".to_owned());
        }
        summrise_winsys::terminate(pid).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn the_list_names_this_test_process() {
        let found = processes(None).expect("the platform can list its own processes");
        let me = std::process::id();
        assert!(
            found.iter().any(|process| process.pid == me),
            "the test's own pid is in the list"
        );
    }

    #[test]
    fn a_filter_that_matches_nothing_lists_nothing() {
        let found = processes(Some("summrise-no-such-executable")).expect("a list");
        assert!(found.is_empty());
    }

    #[test]
    fn tests_support_a_kill_by_pid_and_by_name() {
        let (mut child, name) = long_child();
        let pid = child.id();
        let report = kill(Some(pid), None).expect("the child can be ended");
        assert_eq!(report.killed, vec![pid], "{report:?}");

        let (mut second, _) = long_child();
        let report = kill(None, Some(&name)).expect("the name resolves to the child");
        assert!(
            report.killed.contains(&second.id()),
            "the name killed the pid it names: {report:?}"
        );
        for child in [&mut child, &mut second] {
            wait_until_gone(child);
        }
    }

    #[test]
    fn a_kill_with_no_target_is_refused() {
        assert!(matches!(kill(None, None), Err(SystemError::NoTarget)));
        assert!(matches!(
            kill(Some(1), Some("x")),
            Err(SystemError::TwoTargets)
        ));
        assert!(matches!(
            kill(None, Some("summrise-no-such-executable")),
            Err(SystemError::NoMatch(_))
        ));
    }

    #[test]
    fn a_connection_to_a_listening_port_is_reported_with_its_latency() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let port = listener.local_addr().expect("the port").port();
        let accept = std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let report = net_test("127.0.0.1", port, Duration::from_secs(5)).expect("a test runs");
        assert!(report.connected.is_some(), "{report:?}");
        assert!(report.latency_ms.is_some());
        assert_eq!(report.addresses.len(), 1);
        accept.join().expect("the acceptor finishes");
    }

    #[test]
    fn a_refused_port_is_data_not_an_error() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a listener");
            listener.local_addr().expect("the port").port()
        };

        let report = net_test("127.0.0.1", port, Duration::from_secs(2)).expect("a test runs");
        assert!(report.connected.is_none(), "{report:?}");
        assert!(report.error.is_some(), "a failure names its reason");
    }

    #[test]
    fn a_name_that_does_not_resolve_is_an_error() {
        assert!(matches!(
            net_test("summrise.invalid", 80, Duration::from_secs(1)),
            Err(SystemError::Unresolved(_))
        ));
        assert!(matches!(
            net_test("", 80, Duration::from_secs(1)),
            Err(SystemError::BadHost(_))
        ));
    }

    /// A child that lives long enough to be ended on purpose, and the name it will appear under.
    #[cfg(unix)]
    fn long_child() -> (std::process::Child, String) {
        let child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("sleep starts");
        // The kernel's comm is the executable's name, which is what the name filter matches.
        (child, "sleep".to_owned())
    }

    #[cfg(windows)]
    fn long_child() -> (std::process::Child, String) {
        // `ping` directly rather than through `cmd /C`: the child's pid is then the process the
        // name filter will find, which is what makes "kill by name killed this pid" a real assert.
        let child = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("ping starts");
        (child, "ping.exe".to_owned())
    }

    /// Wait for a child that was signalled to leave the table, so the test does not leak it.
    fn wait_until_gone(child: &mut std::process::Child) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.kill();
        panic!("a killed child did not exit");
    }
}
