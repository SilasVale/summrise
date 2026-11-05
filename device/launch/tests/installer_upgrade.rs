//! The installer's second install is an **upgrade**, not a second migration.
//!
//! `device/installer/summrise.nsi` cannot be run on Linux, so this test holds the two halves the NSIS
//! script owns to the launcher's own seam:
//!
//! 1. the script text: an upgrade is guarded by `current.txt`, runs the launcher's
//!    `--install-version <version-dir> <sidecar> --install ... --data ... --port ...`, and keeps
//!    `--migrate-layout` for the no-pointer branch only;
//! 2. the argv: the exact command the setup would run is tokenized and handed to the launcher's
//!    `parse_config`, so a flag the launcher refuses is a failing test rather than a broken install;
//! 3. the transaction: a migrated tree with `versions/0.3.275` gains a staged `versions/0.3.276`
//!    (the installer's own launcher-signed pair, which `--install-version` now accepts) and the
//!    pointer moves only when the candidate's probe succeeds.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::{Signer, SigningKey};
use summrise_launch::{
    current_pointer, hex_encode, install_version, parse_config, previous_pointer,
    read_current_pointer, read_previous_pointer, save_sidecar, sha256_bytes,
    validate_install_version_with, version_dir, version_exe, version_sidecar, Config, Pointer,
    Sidecar, VersionProbe, VERSION_SIDECAR,
};

/// The installer script, compiled into the test: editing the upgrade call without changing this
/// test is not possible.
const NSI: &str = include_str!("../../installer/summrise.nsi");

/// The version the test setup is installing, and the one `current.txt` already names.
const NEW_VERSION: &str = "0.3.276";
const OLD_VERSION: &str = "0.3.275";

/// The legal command inside the NSIS script: its argv is separated by spaces and double quotes, and
/// it has no embedded quote.
fn upgrade_command() -> String {
    let line = NSI
        .lines()
        .find(|line| line.contains("--install-version") && line.contains("nsExec::ExecToStack"))
        .expect("the installer has an upgrade command");
    let body = line
        .trim()
        .strip_prefix("nsExec::ExecToStack '")
        .expect("the installer runs the launcher through nsExec")
        .strip_suffix('\'')
        .expect("the nsExec argument is quoted");
    body.to_string()
}

/// Split a Windows command line into argv: double quotes group, spaces outside them separate.
fn tokenize(command: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut started = false;
    for character in command.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            character if character.is_whitespace() && !quoted => {
                if started {
                    argv.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            character => {
                token.push(character);
                started = true;
            }
        }
    }
    if started {
        argv.push(token);
    }
    argv
}

/// The command with the NSIS variables replaced by what the setup would pass.
fn substitute(command: &str, install: &str, data: &str) -> String {
    command
        .replace("${DATA_DIR}", data)
        .replace("${VERSION}", NEW_VERSION)
        .replace("${PORT}", "7737")
        .replace("$INSTDIR", install)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "summrise-installer-upgrade-{}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create the test root");
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The one signing key this test uses; its public half is the "launcher" key the installer's sidecar
/// carries. The key value is arbitrary: the point is that the version pair verifies against it.
fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn sidecar_for(version: &str, bytes: &[u8], key: &SigningKey) -> Sidecar {
    let sha256 = sha256_bytes(bytes);
    let channel = "stable";
    let signature = key.sign(format!("{version}\n{channel}\n{sha256}").as_bytes());
    Sidecar {
        version: version.to_string(),
        channel: channel.to_string(),
        sha256,
        signature: hex_encode(&signature.to_bytes()),
    }
}

struct Probe(Result<(), String>);

impl VersionProbe for Probe {
    fn probe(&self, _config: &Config, _version: &summrise_launch::Version) -> Result<(), String> {
        self.0.clone()
    }
}

fn write_version(install: &Path, version: &str, bytes: &[u8]) {
    fs::create_dir_all(version_dir(install, version)).expect("create the version directory");
    fs::write(version_exe(install, version), bytes).expect("write the version app");
    save_sidecar(
        &version_sidecar(install, version),
        &sidecar_for(version, bytes, &signing_key()),
    )
    .expect("write the version sidecar");
}

#[test]
fn the_setup_calls_the_upgrade_on_a_migrated_install_and_migration_only_once() {
    let executes = |flag: &str| {
        NSI.lines()
            .filter(|line| line.contains("nsExec::ExecToStack") && line.contains(flag))
            .count()
    };
    assert_eq!(
        executes("--install-version"),
        1,
        "the installer calls the upgrade exactly once"
    );
    assert_eq!(
        executes("--migrate-layout"),
        1,
        "the first migration remains the only migration call"
    );

    let guard = NSI
        .find("${If} ${FileExists} \"$INSTDIR\\current.txt\"")
        .expect("the upgrade is guarded by current.txt");
    let upgrade = NSI
        .find("nsExec::ExecToStack '\"$INSTDIR\\summrise-launch.exe\" --install-version")
        .expect("the installer has an upgrade call");
    let migrate = NSI
        .find("nsExec::ExecToStack '\"$INSTDIR\\summrise-launch.exe\" --migrate-layout")
        .expect("the installer has a migration call");
    // The outer `${Else}` is the one that opens the no-current.txt branch. The upgrade branch has
    // its own nested `${If}`/`${Else}` pair (the restart after a successful pointer move), so an
    // unqualified search would stop at the inner one.
    let otherwise = NSI
        .find("\n  ${Else}\n    ; THE ONE-TIME MOVE")
        .expect("the migration is the else branch of the pointer check");

    assert!(guard < upgrade, "the upgrade is inside the pointer guard");
    assert!(
        upgrade < otherwise && otherwise < migrate,
        "migration is the no-current.txt branch, not a second step after the upgrade"
    );

    let command = upgrade_command();
    let expected = "--install-version \"$INSTDIR\\versions\\${VERSION}\" \
                    \"$INSTDIR\\versions\\${VERSION}\\summrise-device.json\" \
                    --install \"$INSTDIR\" --data \"${DATA_DIR}\" --port ${PORT}";
    assert!(
        command.contains(expected),
        "the upgrade argv is the launcher's contract: {command}"
    );
    for staging in [
        "SetOutPath \"$INSTDIR\\versions\\${VERSION}\"",
        "File /oname=summrise-device.exe \"${SOURCE}\\summrise-device.exe\"",
        "File /oname=summrise-device.json \"${SOURCE}\\summrise-device.sidecar.json\"",
    ] {
        assert!(
            NSI.contains(staging),
            "the setup stages its own pair into versions\\<version>: {staging}"
        );
    }

    let failure = &NSI[upgrade..otherwise];
    assert!(
        failure.contains("SetErrorLevel 1"),
        "a failed upgrade must make the installer exit non-zero"
    );
    assert!(
        failure.contains("$1"),
        "the failure message reports the launcher's stderr"
    );
    assert!(
        !NSI.contains("Delete \"$INSTDIR\\current.txt\""),
        "the installer never removes the pointer; the launcher owns it"
    );
}

#[test]
fn the_setup_restarts_the_service_and_requires_the_new_version_after_the_pointer_moves() {
    let upgrade = NSI
        .find("nsExec::ExecToStack '\"$INSTDIR\\summrise-launch.exe\" --install-version")
        .expect("the installer has an upgrade call");
    let otherwise = NSI
        .find("\n  ${Else}\n    ; THE ONE-TIME MOVE")
        .expect("the migration is the else branch of the pointer check");
    let branch = &NSI[upgrade..otherwise];

    // The launcher's call must return 0 before the installer touches the service: a failed pointer
    // move is reported and the running process is left alone.
    let failed = branch
        .find("${If} $0 != \"0\"")
        .expect("the installer checks the launcher's exit");
    let succeeded = branch
        .find("${Else}")
        .expect("the restart is the success branch of that check");
    assert!(failed < succeeded, "the success branch follows the check");

    let stop = branch
        .find("sc.exe stop ${SERVICE}")
        .expect("the setup stops the service after the pointer moves");
    let start = branch
        .find("sc.exe start ${SERVICE}")
        .expect("the setup starts the service again");
    let health = branch
        .find("http://127.0.0.1:${PORT}/api/health")
        .expect("the setup polls the health endpoint");
    let version = branch
        .find(r#""$\"version$\":$\"${VERSION}$\"""#)
        .expect("the poll requires the reported version to equal ${VERSION}");
    assert!(
        succeeded < stop,
        "the restart is requested only after --install-version returned 0"
    );
    assert!(stop < start, "the service is started after it is stopped");
    assert!(
        start < health,
        "health is polled after the restarted process exists"
    );
    assert!(
        health < version,
        "the health answer is matched against the setup's version"
    );
    assert!(
        branch.contains("${If} $2 >= 30") && branch.contains("Sleep 1000"),
        "the version poll is bounded (30 x 1 s)"
    );

    // A mismatch is reported, not repaired: the pointer and previous.txt stay where the launcher
    // left them, so the operator can still roll back with one write and the old version stays on
    // disk.
    let failure = branch
        .find("upgrade: ${VERSION} did not answer /api/health")
        .expect("a version that never answers is logged");
    assert!(
        version < failure,
        "the failure comes after the poll saw a non-matching answer"
    );
    assert!(
        branch[failure..].contains("SetErrorLevel 1"),
        "a version mismatch makes the setup exit non-zero"
    );
    assert!(
        branch[failure..].contains("$4"),
        "the last health answer is logged with the failure"
    );
    assert!(
        branch[failure..].contains("previous.txt"),
        "the failure names the rollback target the operator still has"
    );
    assert!(
        !branch.contains("Delete \"$INSTDIR\\current.txt\"")
            && !branch.contains("Delete \"$INSTDIR\\previous.txt\""),
        "the installer never removes a pointer; the launcher and the operator own them"
    );
}

#[test]
fn the_installer_argv_is_the_launchers_upgrade_argv() {
    let install = r"C:\Program Files\Summrise";
    let data = r"C:\ProgramData\Summrise";
    let argv = tokenize(&substitute(&upgrade_command(), install, data));

    assert_eq!(
        argv[0],
        format!(r"{install}\summrise-launch.exe"),
        "the setup calls the launcher it installed"
    );
    assert_eq!(argv[1], "--install-version");
    assert_eq!(
        argv[2],
        format!(r"{install}\versions\{NEW_VERSION}"),
        "the version directory is versions\\<version>"
    );
    assert_eq!(
        argv[3],
        format!(r"{install}\versions\{NEW_VERSION}\{VERSION_SIDECAR}"),
        "the sidecar sits beside the app binary"
    );

    // The tail is exactly the launcher's usual install/data/port parser: a flag the launcher does
    // not own would fail here, before any installer is built.
    let config = parse_config(&argv[4..], Path::new(r"C:\nowhere"), None)
        .expect("the launcher accepts the installer's install/data/port flags");
    assert_eq!(config.install, PathBuf::from(install));
    assert_eq!(config.data, PathBuf::from(data));
    assert_eq!(config.port, 7737);
}

#[test]
fn an_installer_upgrade_stages_a_version_and_moves_the_pointer() {
    let tmp = TempDir::new("moves-pointer");
    let install = tmp.path().join("install");
    let data = tmp.path().join("data");
    fs::create_dir_all(&data).expect("create the data directory");
    write_version(&install, OLD_VERSION, b"the running build");
    fs::write(current_pointer(&install), format!("{OLD_VERSION}\n")).expect("the pointer");

    // What the setup does on a migrated layout: stage its own pair as versions/<version>.
    write_version(&install, NEW_VERSION, b"the new setup's app");

    let pubkey = hex_encode(signing_key().verifying_key().as_bytes());
    let pair = validate_install_version_with(
        &version_exe(&install, NEW_VERSION),
        &version_sidecar(&install, NEW_VERSION),
        &[&pubkey],
    )
    .expect("the installer's launcher-signed pair verifies for --install-version");

    let config = Config {
        install: install.clone(),
        data,
        port: 7_737,
    };
    let installed = install_version(&config, NEW_VERSION, &pair, &Probe(Ok(())))
        .expect("a proven upgrade becomes current");

    assert_eq!(installed.version.name, NEW_VERSION);
    assert_eq!(installed.previous.as_deref(), Some(OLD_VERSION));
    assert_eq!(
        read_current_pointer(&install),
        Pointer::Version(NEW_VERSION.to_string())
    );
    assert_eq!(
        read_previous_pointer(&install),
        Pointer::Version(OLD_VERSION.to_string()),
        "the version the setup replaced stays as the rollback target"
    );
    assert!(
        version_exe(&install, OLD_VERSION).is_file(),
        "the old version directory is kept for a rollback"
    );
    assert_eq!(
        fs::read(version_exe(&install, NEW_VERSION)).expect("the new app"),
        b"the new setup's app"
    );
}

#[test]
fn a_failed_installer_upgrade_leaves_the_pointer_alone() {
    let tmp = TempDir::new("failed-upgrade");
    let install = tmp.path().join("install");
    let data = tmp.path().join("data");
    fs::create_dir_all(&data).expect("create the data directory");
    write_version(&install, OLD_VERSION, b"the running build");
    fs::write(current_pointer(&install), format!("{OLD_VERSION}\n")).expect("the pointer");
    write_version(&install, NEW_VERSION, b"the unproven setup");

    let pubkey = hex_encode(signing_key().verifying_key().as_bytes());
    let pair = validate_install_version_with(
        &version_exe(&install, NEW_VERSION),
        &version_sidecar(&install, NEW_VERSION),
        &[&pubkey],
    )
    .expect("the staged pair verifies");
    let config = Config {
        install: install.clone(),
        data,
        port: 7_737,
    };

    let error = install_version(
        &config,
        NEW_VERSION,
        &pair,
        &Probe(Err("it never answered /api/health".to_string())),
    )
    .expect_err("a candidate that cannot serve is refused");
    assert!(error.contains("health"), "{error}");
    assert_eq!(
        read_current_pointer(&install),
        Pointer::Version(OLD_VERSION.to_string()),
        "the launcher writes the pointer only after the probe; a failure leaves it untouched"
    );
    assert!(
        !previous_pointer(&install).exists(),
        "a failed probe never records a previous version either"
    );
    assert!(
        version_exe(&install, OLD_VERSION).is_file(),
        "the running version is still on disk"
    );
}
