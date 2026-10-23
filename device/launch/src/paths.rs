//! Every filesystem name the launcher touches, spelled once.
//!
//! The launcher is the only writer of the app binary, so its paths are part of its contract: the
//! installer, the app's update stager, and the Windows lifecycle test all have to agree with what is
//! below. A hand-joined `install.join("summrise-device.exe")` elsewhere is a second spelling of the
//! same fact, and the two eventually disagree.
//!
//! The baseline is three generations, never one pair of loose files (fix design F2):
//!
//! ```text
//! <install>/baseline/current/summrise-device.exe + baseline.json   the pair that boots
//! <install>/baseline/previous/summrise-device.exe + baseline.json   the pair to fall back to
//! <install>/baseline/pending/summrise-device.exe + baseline.json    staged, not yet proven
//! ```

use std::path::{Path, PathBuf};

/// The app binary, relative to the install root (design §4.1).
pub const APP_EXE: &str = "summrise-device.exe";

/// The launcher's own binary name, used to refuse to kill a recycled PID that is not us.
pub const LAUNCHER_EXE: &str = "summrise-launch.exe";

/// The baseline directory under the install root. Its generations are written by the launcher only,
/// after a verified install or update (design §4.4).
pub const BASELINE_DIR: &str = "baseline";

/// The sidecar file name inside every generation directory.
pub const BASELINE_SIDECAR: &str = "baseline.json";

/// The sidecar name of the pre-generation layout: `<install>/baseline/summrise-device.json`.
///
/// A tree installed before the generation transaction has one flat pair beside the `current`
/// directory instead of a generation of its own. `--install-service` migrates it (NEW-1).
pub const FLAT_SIDECAR: &str = "summrise-device.json";

/// The generation directory names.
pub const CURRENT_DIR: &str = "current";
/// The generation directory a successful update came from, kept for one rollback.
pub const PREVIOUS_DIR: &str = "previous";
/// The generation written by an adoption and not yet proven by a health probe.
pub const PENDING_DIR: &str = "pending";

/// `<install>/summrise-device.exe` — the one binary the app may never write.
pub fn app_exe(install: &Path) -> PathBuf {
    install.join(APP_EXE)
}

/// `<install>/baseline` — the parent of the three generations.
pub fn baseline_dir(install: &Path) -> PathBuf {
    install.join(BASELINE_DIR)
}

/// `<install>/baseline/<generation>` — one generation directory.
pub fn generation_dir(install: &Path, generation: &str) -> PathBuf {
    baseline_dir(install).join(generation)
}

/// `<install>/baseline/<generation>/summrise-device.exe`.
pub fn generation_exe(install: &Path, generation: &str) -> PathBuf {
    generation_dir(install, generation).join(APP_EXE)
}

/// `<install>/baseline/<generation>/baseline.json`.
pub fn generation_sidecar(install: &Path, generation: &str) -> PathBuf {
    generation_dir(install, generation).join(BASELINE_SIDECAR)
}

/// `<install>/baseline/current/summrise-device.exe` — the trusted copy the launcher repairs from.
pub fn baseline_exe(install: &Path) -> PathBuf {
    generation_exe(install, CURRENT_DIR)
}

/// `<install>/baseline/current/baseline.json` — the signed facts about the current baseline.
pub fn baseline_sidecar(install: &Path) -> PathBuf {
    generation_sidecar(install, CURRENT_DIR)
}

/// `<install>/baseline/summrise-device.exe` — the pre-generation flat binary, migrated on install.
pub fn flat_exe(install: &Path) -> PathBuf {
    baseline_dir(install).join(APP_EXE)
}

/// `<install>/baseline/summrise-device.json` — the pre-generation flat sidecar.
pub fn flat_sidecar(install: &Path) -> PathBuf {
    baseline_dir(install).join(FLAT_SIDECAR)
}

/// `<install>/summrise-device.new.exe` — the staged artifact the app writes and the supervisor
/// validates (fix design F1/F3).
pub fn staged_exe(install: &Path) -> PathBuf {
    install.join("summrise-device.new.exe")
}

/// `<install>/summrise-device.new.json` — the signed facts about [`staged_exe`].
pub fn staged_sidecar(install: &Path) -> PathBuf {
    install.join("summrise-device.new.json")
}

/// `<install>/swap.request.json` — the app's request that the supervisor perform one swap. It is
/// written atomically by the app and removed by the supervisor after the result is written.
pub fn swap_request(install: &Path) -> PathBuf {
    install.join("swap.request.json")
}

/// `<install>/swap.result.json` — the supervisor's answer to [`swap_request`].
pub fn swap_result(install: &Path) -> PathBuf {
    install.join("swap.result.json")
}

/// `<install>/repair.log` — one JSON line per restore from the baseline (I3).
///
/// The launcher is its only writer. The app reads it at start and records the offset in the state
/// document; it is the machine evidence that a repair actually happened.
pub fn repair_log(install: &Path) -> PathBuf {
    install.join(REPAIR_LOG)
}

/// The repair log's file name inside the install root.
pub const REPAIR_LOG: &str = "repair.log";

/// `<data>/token` — the install token, minted by `--install-service` if absent and read by the app.
pub fn install_token(data: &Path) -> PathBuf {
    data.join("token")
}

/// `<data>/app.pid` — the running app child, written by the supervisor before a health probe.
pub fn app_pid_file(data: &Path) -> PathBuf {
    data.join("app.pid")
}

/// `<data>/launcher.pid` — the console supervisor. The service has no pid file: the SCM owns it.
pub fn launcher_pid_file(data: &Path) -> PathBuf {
    data.join("launcher.pid")
}

/// `<data>/console.json` — the console link `--install-service` / `run` persist for the app.
///
/// It travels with the install token rather than with the launcher's own directory: the app is
/// started from `--data`, so that is the one directory both processes already agree on.
pub fn console_link(data: &Path) -> PathBuf {
    data.join(CONSOLE_FILE)
}

/// The console link's file name inside the data directory.
pub const CONSOLE_FILE: &str = "console.json";
