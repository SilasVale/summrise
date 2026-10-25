//! What machine is this: the facts an operator needs before a device's report means anything.
//!
//! The console shows a device as "online, version N", which answers "is the app running" and not
//! "what am I looking at". The facts here are the other half: the operating system and uptime, the
//! memory and disk the device is sharing, and the sessions — including the one the helper runs in,
//! because "the desktop cannot be captured" is a property of a session, not of a device.
//!
//! Everything is best-effort. A fact that cannot be read is reported as zero or an empty list rather
//! than failing the call: a device that answers with five of six facts is more useful than one that
//! refuses to say anything.

/// The machine facts, as the wire sees them.
pub fn facts() -> serde_json::Value {
    let mut document = platform_facts();
    if let Some(helper) = crate::helper::current() {
        let pid = helper.pid();
        let registered = helper.port().is_some();
        let session = pid.and_then(session_of_process);
        document["helper"] = serde_json::json!({
            "registered": registered,
            "pid": pid,
            "session": session,
        });
    } else {
        document["helper"] = serde_json::json!({ "registered": false });
    }
    document
}

/// The session a process is in — where the helper landed, and what a capture would target.
pub fn session_of_process(pid: u32) -> Option<u32> {
    #[cfg(windows)]
    {
        summrise_winsys::session_of_process(pid)
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        None
    }
}

#[cfg(windows)]
fn platform_facts() -> serde_json::Value {
    use serde_json::json;

    match summrise_winsys::host_facts() {
        Ok(facts) => json!({
            "os": {
                "name": "Windows",
                "version": facts.os_version,
                "arch": std::env::consts::ARCH,
            },
            "machine": facts.machine,
            "uptime_s": facts.uptime_s,
            "cpu": { "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0) },
            "memory": {
                "total_bytes": facts.memory_total_bytes,
                "available_bytes": facts.memory_available_bytes,
            },
            "disks": facts.volumes.iter().map(|volume| json!({
                "name": volume.name,
                "total_bytes": volume.total_bytes,
                "free_bytes": volume.free_bytes,
            })).collect::<Vec<_>>(),
            "sessions": facts.sessions.iter().map(|session| json!({
                "id": session.id,
                "user": session.user,
                "state": session.state,
            })).collect::<Vec<_>>(),
        }),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

/// The same document where `/proc` is the only source. It exists so the shape and the tool can be
/// tested on the machines CI runs on; the device only ships on Windows.
#[cfg(not(windows))]
fn platform_facts() -> serde_json::Value {
    use serde_json::json;

    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map(|seconds| seconds as u64)
        .unwrap_or(0);
    let memory = std::fs::read_to_string("/proc/meminfo").ok().map(|text| {
        let field = |name: &str| -> u64 {
            text.lines()
                .find(|line| line.starts_with(name))
                .and_then(|line| line.split_whitespace().nth(1))
                .and_then(|value| value.parse::<u64>().ok())
                .map(|kilobytes| kilobytes * 1024)
                .unwrap_or(0)
        };
        (field("MemTotal:"), field("MemAvailable:"))
    });
    json!({
        "os": {
            "name": std::env::consts::OS,
            "version": std::fs::read_to_string("/proc/sys/kernel/osrelease")
                .map(|text| text.trim().to_owned())
                .unwrap_or_default(),
            "arch": std::env::consts::ARCH,
        },
        "machine": std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|text| text.trim().to_owned())
            .unwrap_or_default(),
        "uptime_s": uptime,
        "cpu": { "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0) },
        "memory": {
            "total_bytes": memory.map(|(total, _)| total).unwrap_or(0),
            "available_bytes": memory.map(|(_, available)| available).unwrap_or(0),
        },
        "disks": [],
        "sessions": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_facts_are_a_document_a_console_can_render() {
        let facts = facts();
        assert!(facts["os"]["name"].is_string(), "{facts}");
        assert!(facts["os"]["arch"].is_string(), "{facts}");
        assert!(facts["uptime_s"].is_u64(), "{facts}");
        assert!(facts["cpu"]["cores"].is_u64(), "{facts}");
        assert!(facts["memory"]["total_bytes"].is_u64(), "{facts}");
        assert!(facts["disks"].is_array(), "{facts}");
        assert!(facts["sessions"].is_array(), "{facts}");
        // Without a helper in this process, the document says so rather than omitting the key.
        assert_eq!(facts["helper"]["registered"], serde_json::json!(false));
    }

    #[test]
    fn the_uptime_is_a_positive_number_on_any_real_machine() {
        let facts = facts();
        let uptime = facts["uptime_s"].as_u64().unwrap_or(0);
        assert!(uptime > 0, "{facts}");
    }
}
