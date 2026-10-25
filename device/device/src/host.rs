//! What machine is this: the facts an operator needs before a device's report means anything.
//!
//! The console shows a device as "online, version N", which answers "is the app running" and not
//! "what am I looking at". The facts here are the other half: the operating system and uptime, the
//! memory and disk the device is sharing, and the sessions — including the one the helper runs in,
//! because "the desktop cannot be captured" is a property of a session, not of a device.
//!
//! Everything is best-effort. A fact that cannot be read is reported as zero, an empty list or null
//! rather than failing the call: a device that answers with five of six facts is more useful than one
//! that refuses to say anything.

use std::path::Path;

/// The launcher's baseline layout, relative to the install root. The writer's spelling of these
/// names is `device/launch/src/paths.rs`.
const BASELINE_DIR: &str = "baseline";
const BASELINE_SIDECAR: &str = "baseline.json";

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
    // The install root is where this executable was started from: an installed app lives at
    // `<install>/summrise-device.exe`. An executable that cannot name its directory reports the
    // three generations as unknown rather than failing the facts.
    let install = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    document["update"] = match install.as_deref() {
        Some(install) => update_state(install),
        None => serde_json::json!({
            "current": null,
            "previous": null,
            "pending": null,
        }),
    };
    document
}

/// The frame the device sends on its channel: the console reads `host` and stores it per device.
pub fn frame() -> String {
    serde_json::json!({ "host": facts() }).to_string()
}

/// What the launcher's baseline generations say, for display.
///
/// `current` is the generation the launcher boots, `pending` a pair staged and not yet proven, and
/// `previous` the pair a failed update returns to. Only `version` and `channel` are read from each
/// `<install>/baseline/<generation>/baseline.json`.
///
/// This does not verify the sidecar's signature. The launcher verified it before it wrote the pair;
/// this reports what the launcher wrote, a fact for the console and not a trust decision. A missing,
/// unreadable or malformed sidecar is `null` for that generation, and one bad file never hides the
/// other two.
fn update_state(install: &Path) -> serde_json::Value {
    serde_json::json!({
        "current": generation_state(install, "current"),
        "previous": generation_state(install, "previous"),
        "pending": generation_state(install, "pending"),
    })
}

/// The two fields of a generation's sidecar that the console shows.
///
/// The launcher writes `sha256` and `signature` beside them; a document that names only these two
/// leaves the rest unread rather than copying the launcher's evidence into the facts.
#[derive(serde::Deserialize)]
struct SidecarVersion {
    version: String,
    channel: String,
}

/// One generation's `{version, channel}`, or `null` when the sidecar cannot be read or parsed.
fn generation_state(install: &Path, generation: &str) -> serde_json::Value {
    let path = install
        .join(BASELINE_DIR)
        .join(generation)
        .join(BASELINE_SIDECAR);
    let Ok(text) = std::fs::read_to_string(path) else {
        return serde_json::Value::Null;
    };
    let Ok(sidecar) = serde_json::from_str::<SidecarVersion>(&text) else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "version": sidecar.version,
        "channel": sidecar.channel,
    })
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

    use crate::test_support::TempDir;

    /// Write one generation's sidecar under a temp install.
    fn write_sidecar(install: &Path, generation: &str, sidecar: &str) {
        let directory = install.join("baseline").join(generation);
        std::fs::create_dir_all(&directory).expect("the generation directory is creatable");
        std::fs::write(directory.join("baseline.json"), sidecar).expect("the sidecar is writable");
    }

    #[test]
    fn the_three_generations_report_their_version_and_channel() {
        let dir = TempDir::new("update-state-generations");
        let install = dir.join("install");
        write_sidecar(
            &install,
            "current",
            r#"{"version":"0.3.29","channel":"stable","sha256":"00","signature":"00"}"#,
        );
        write_sidecar(
            &install,
            "previous",
            r#"{"version":"0.3.28","channel":"stable","sha256":"00","signature":"00"}"#,
        );
        write_sidecar(
            &install,
            "pending",
            r#"{"version":"0.3.30","channel":"canary","sha256":"00","signature":"00"}"#,
        );

        // The whole document equals: the sha256 and signature are not copied to the facts.
        assert_eq!(
            update_state(&install),
            serde_json::json!({
                "current": { "version": "0.3.29", "channel": "stable" },
                "previous": { "version": "0.3.28", "channel": "stable" },
                "pending": { "version": "0.3.30", "channel": "canary" },
            })
        );
    }

    #[test]
    fn one_malformed_sidecar_is_null_while_the_others_report() {
        let dir = TempDir::new("update-state-malformed");
        let install = dir.join("install");
        write_sidecar(
            &install,
            "current",
            r#"{"version":"0.3.29","channel":"stable"}"#,
        );
        write_sidecar(
            &install,
            "previous",
            r#"{"version":"0.3.28","channel":"stable"}"#,
        );
        // A torn write: the file is there, the document is not.
        write_sidecar(&install, "pending", r#"{"version":"0.3.3"#);

        let update = update_state(&install);
        assert_eq!(update["current"]["version"], "0.3.29");
        assert_eq!(update["previous"]["version"], "0.3.28");
        assert!(update["pending"].is_null(), "{update}");
    }

    #[test]
    fn a_sidecar_without_a_channel_is_null() {
        let dir = TempDir::new("update-state-missing-field");
        let install = dir.join("install");
        write_sidecar(&install, "current", r#"{"version":"0.3.29"}"#);

        let update = update_state(&install);
        assert!(update["current"].is_null(), "{update}");
    }

    #[test]
    fn a_missing_baseline_directory_is_three_nulls() {
        let dir = TempDir::new("update-state-no-baseline");
        let install = dir.join("install");
        std::fs::create_dir_all(&install).expect("the install directory is creatable");

        assert_eq!(
            update_state(&install),
            serde_json::json!({ "current": null, "previous": null, "pending": null })
        );
    }

    #[test]
    fn an_install_path_that_does_not_exist_does_not_panic() {
        let dir = TempDir::new("update-state-nonexistent");
        let update = update_state(&dir.join("not-an-install"));
        assert!(update["current"].is_null(), "{update}");
        assert!(update["previous"].is_null(), "{update}");
        assert!(update["pending"].is_null(), "{update}");
    }

    #[test]
    fn the_facts_report_the_update_state() {
        let facts = facts();
        let update = &facts["update"];
        assert!(update.is_object(), "{facts}");
        for generation in ["current", "previous", "pending"] {
            let value = &update[generation];
            assert!(
                value.is_null() || (value["version"].is_string() && value["channel"].is_string()),
                "{facts}"
            );
        }
    }

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
    fn the_frame_carries_the_facts_under_host() {
        let frame: serde_json::Value = serde_json::from_str(&frame()).expect("the frame is JSON");
        assert!(frame["host"]["os"]["name"].is_string(), "{frame}");
    }

    #[test]
    fn the_uptime_is_a_positive_number_on_any_real_machine() {
        let facts = facts();
        let uptime = facts["uptime_s"].as_u64().unwrap_or(0);
        assert!(uptime > 0, "{facts}");
    }
}
