//! ONE OWNER FOR THE SPAWN POLICY — the no-console flag, and the two kill doors.
//!
//! Both rules used to be restated at every site that spawns: whether the child
//! needs `CREATE_NO_WINDOW`, and how to kill a TREE rather than a process. The
//! copies drifted — the flag was declared twice, applied at some sites and not
//! others, and the user-facing `system_process_kill` ran `taskkill /PID <pid> /F`
//! with no `/T`, so a child that outlived its parent was not in the answer that
//! tool promises ("Returns what was killed"). None of it was assertable off
//! Windows, which is exactly why it drifted: `cargo test` on Linux can compile
//! neither arm of a `#[cfg(windows)]` block, and an unobservable policy is a
//! policy nobody notices breaking.
//!
//! So the policy lives here, in three pieces: [`hidden`] and [`hidden_std`] (the
//! flag and the RULE for when a spawn site wants it — ONE rule with ONE entry
//! point per command type, because the sites do not share a type), [`kill_tree`]
//! (a pid and its children) and [`kill_by_name`] (every process with a name,
//! deliberately NOT a tree). Each has a pure argument builder next to it so the
//! parts that are Windows-only at runtime are still pinned by a test on any
//! platform.
//!
//! Internal-only: this is device process hygiene, not a wire surface. It is `pub`
//! only because the BINARY crate (`main.rs` and its `winmain` module) is a
//! separate crate, and its `std::process::Command` sites have to be able to ask
//! the same question the async ones do.

use std::io;

/// `CREATE_NO_WINDOW` (0x0800_0000) — run the child with a console but no window.
///
/// ONE CONST FOR BOTH ENTRY POINTS ([`hidden`], [`hidden_std`]): they are the same
/// policy applied to two command types, and a second copy of the number is exactly
/// the drift this module exists to stop.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// THE NO-CONSOLE FLAG, and the rule for when a spawn site wants it.
///
/// round-143: `node.exe` is a console-subsystem binary; when the agent (or the
/// swap's powershell / taskkill helpers) spawns it without this flag and the
/// parent has an interactive console, Windows allocates a visible cmd window.
/// Harmless when the parent has no console (session-0 service) and prevents the
/// flash under `summrise run` / dev consoles — which is why this only ever showed
/// up under `summrise run` and in dev consoles rather than on a device. We apply
/// it through `std::os::windows::process::CommandExt::creation_flags` on the
/// inner std Command (tokio's Command does not expose it directly, but its
/// `as_std_mut` gives us the same underlying handle).
///
/// THE RULE, so the next spawn site does not have to guess:
///
/// * **A spawn of a console-subsystem binary from a process that may have a
///   console WANTS the flag.** That is `node.exe` (bundled playwright, the
///   playwright stdio server, the AI script runner), `powershell`, `tar`
///   (bsdtar ships with Windows 10 1803+ as `C:\Windows\System32\tar.exe`, and
///   the update path extracts the npm tgz with it), `taskkill`, `tasklist`,
///   `ping`, `where`, `sc.exe`, `schtasks`, `reg`, `icacls`, `cloudflared.exe`
///   (the tunnel binary), and `cmd`/`sh` (terminal_execute's local mode, spawned
///   through the `shell` variable).
/// * **A spawn of something with no console window of its own — or one already
///   covered — does not.** Unix-only helpers (`kill`, `pgrep`) are in this class
///   and the flag is a no-op for them anyway, so asking there costs nothing;
///   what the rule forbids is the other direction, a site that launches a
///   console binary from the agent and never asks.
///
/// THE CLASS IS DATA, NOT PROSE: `console_subsystem_spawn_sites_ask_for_the_flag`
/// holds the two bullets as `CONSOLE_PROGRAMS` and scans every `.rs` file under
/// `src/`, so a new spawn site of one of these programs is caught whether or not
/// its file was already on a list. The two have to agree: adding a program to
/// the rule means adding it to that class.
///
/// A site that does want it asks by CALL, on every platform — `hidden` is defined
/// on every platform (a no-op off Windows) precisely so a Linux-run test can
/// assert that a spawn site asks for it. The flag itself is unobservable off
/// Windows and there is no getter for it there either, so the call is the whole
/// of what a gate can hold; if this function were `#[cfg(windows)]`, every call
/// site would be `#[cfg(windows)]` too and no Linux test could see any of them.
///
/// A site whose command is the BLOCKING `std::process::Command` asks through
/// [`hidden_std`] instead — the same flag, the same rule, one entry point per
/// command type.
///
/// Returns the command so a call site can chain.
pub fn hidden(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.as_std_mut().creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// THE SAME FLAG FOR A BLOCKING `Command`. `hidden` takes the tokio type because
/// most of this agent's spawns are async; these are not — the service installer,
/// the ACL hardening and the tunnel repair run before or outside the async
/// runtime, and a rule that only reaches one command type is a rule half the spawn
/// sites cannot follow.
///
/// THE RULE IS [`hidden`]'S, stated once there and deliberately not restated here:
/// which programs are console-subsystem binaries, when a site wants the flag, and
/// why the ask is a call rather than a comment are all that function's doc. The
/// only difference between the two is the command type — same `#[cfg(windows)]`
/// body, same [`CREATE_NO_WINDOW`], same no-op on every other platform, which is
/// what lets a Linux test see this ask too.
///
/// Returns the command so a call site can chain.
pub fn hidden_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// THE WINDOWS TREE KILL, as arguments — and the seam that keeps `/T` testable.
///
/// `/T` walks the child tree and is the whole point of this door: node forks
/// Edge, and killing only the parent orphans it. `/F` is the force switch;
/// `force = false` is the graceful ask (`taskkill` with no `/F`, which is the
/// Windows answer to the unix SIGTERM arm below — SIGKILL straight away left
/// databases and build caches half-written).
///
/// PURE AND PLATFORM-INDEPENDENT ON PURPOSE. The spawn this feeds never runs on
/// Linux, so the only way a Linux gate can hold the flag is to call the argument
/// builder directly — and that is not a nicety: `system_process_kill`'s pid arm
/// shipped `["/PID", pid, "/F"]` (no `/T`) for exactly as long as this shape was
/// inline and unobservable off Windows.
///
/// Compiled where it is used — Windows at runtime, TESTS on every platform that
/// has `cargo test` — rather than unconditionally, so a Linux release build does
/// not carry a dead Windows argument builder.
#[cfg(any(windows, test))]
pub(crate) fn taskkill_args(pid: u32, force: bool) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if force {
        args.push("/F".to_string());
    }
    args.push("/T".to_string());
    args.push("/PID".to_string());
    args.push(pid.to_string());
    args
}

/// THE WINDOWS NAME KILL, as arguments — `taskkill /IM <name>` (+ `/F`), and
/// NEVER `/T`.
///
/// The absence of `/T` is a decision, not an omission: a name can match several
/// processes, so `/T` would take each one's whole tree, and the blast radius is
/// the caller's to choose. [`kill_by_name`] is the door that makes that choice;
/// [`kill_tree`] is the other one. Pinned by its own test, because "we did not
/// pass the tree flag" is a property that only a test can keep true.
#[cfg(any(windows, test))]
pub(crate) fn taskkill_name_args(name: &str, force: bool) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    if force {
        args.push("/F".to_string());
    }
    args.push("/IM".to_string());
    args.push(name.to_string());
    args
}

/// Run one kill command and report whether the kill was MADE.
///
/// * `Ok(())` — the tool ran and reported success.
/// * `Err(kind == NotFound)` — the tool could not be run at all (no `taskkill`,
///   no `kill`, no `pgrep`). The caller must then say the kill could not be
///   ATTEMPTED; it must not report that nothing matched, which would be a false
///   statement about the process table made from a command that never ran.
/// * any other `Err` — the tool ran and refused or found nothing, and its OWN
///   stderr is carried in the message ("No such process" and "Access is denied"
///   are different answers and must not be flattened into one).
///
/// The no-console flag is applied here rather than at each caller: every command
/// this module runs (`taskkill`, `kill`, `pgrep`) is a console-subsystem binary,
/// so all of them are in the rule's first class.
async fn attempt(what: &str, cmd: &mut tokio::process::Command) -> io::Result<()> {
    hidden(cmd);
    let out = cmd
        .output()
        .await
        .map_err(|e| io::Error::new(e.kind(), format!("{what}: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stderr = stderr.trim();
    Err(io::Error::other(if stderr.is_empty() {
        format!("{what} exited with {}", out.status)
    } else {
        format!("{what}: {stderr}")
    }))
}

/// KILL A PROCESS **AND ITS CHILDREN**.
///
/// On Windows `/T` is the tree flag, and it is the whole point: node forks Edge,
/// and killing only the parent orphans it (the reason `playwright`'s stop site
/// records). On unix, `kill`.
///
/// "The tree" means something different on each platform, so both are stated
/// rather than implied:
///
/// * **Windows** — `taskkill /T /PID <pid>` ([`taskkill_args`]) walks the child
///   tree; there are no process groups here (round-55).
/// * **Unix** — the tree IS the process group. A spawn site that wants its
///   descendants covered spawns the leader with `process_group(0)`, which makes
///   the child's pid its own group id; a NEGATIVE pid then addresses every member.
///   That is not decoration: killing only the shell left a timed-out `make` /
///   installer running orphaned on the device (round-54), and terminal_execute's
///   local shell still relies on it. The group is tried FIRST and a pid that
///   leads no group (playwright's node in a dev run) falls back to a direct
///   signal, so no caller silently loses its kill. A pid that leads no group
///   cannot have its own children reached without walking `/proc`, and this does
///   not pretend otherwise.
///
/// `force = false` is the graceful signal (SIGTERM / `taskkill` without `/F`:
/// SIGKILL straight away left databases and build caches half-written);
/// `force = true` is the last resort (SIGKILL / `/F`).
pub async fn kill_tree(pid: u32, force: bool) -> io::Result<()> {
    // pid 0 is refused, and that refusal is load-bearing: on unix `kill 0` (or
    // `kill -- -0`) means "every process in the CALLER's group", so a caller that
    // lost its child handle (`id()` is None, `unwrap_or(0)`) would otherwise
    // order the agent to kill itself. On Windows it is a taskkill that can only
    // fail, so nothing legitimate is lost.
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing pid 0: on unix that is the caller's own process group",
        ));
    }
    #[cfg(test)]
    test_records::tree_requested_note(pid, force);
    #[cfg(windows)]
    return windows_kill_tree(pid, force).await;
    #[cfg(unix)]
    return unix_kill_tree(pid, force).await;
    #[cfg(not(any(unix, windows)))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("no tree kill for pid {pid} on this platform"),
    ));
}

#[cfg(windows)]
async fn windows_kill_tree(pid: u32, force: bool) -> io::Result<()> {
    let mut cmd = tokio::process::Command::new("taskkill");
    cmd.args(taskkill_args(pid, force));
    attempt("taskkill", &mut cmd).await
}

#[cfg(unix)]
async fn unix_kill_tree(pid: u32, force: bool) -> io::Result<()> {
    let sig = if force { "-9" } else { "-15" };
    let mut group = tokio::process::Command::new("kill");
    group.args([sig, "--", &format!("-{pid}")]);
    if attempt("kill (process group)", &mut group).await.is_ok() {
        return Ok(());
    }
    // No such group: the pid is not a leader (or the group is already gone), so
    // signal the process itself. The direct attempt's error is the one returned
    // — it is the attempt that was actually about the pid the caller named.
    let mut direct = tokio::process::Command::new("kill");
    direct.args([sig, "--", &pid.to_string()]);
    attempt("kill", &mut direct).await
}

/// WHAT A NAME KILL DID — the three answers a caller must not flatten.
///
/// This door used to answer `io::Result<()>`: `Ok` meant "something was killed"
/// while every per-pid result was discarded (`let _ = attempt(…)`), so a signal
/// that FAILED was reported as a kill — against the tool's own promise ("Returns
/// what was killed"). The optimism was inherited from the call site this door
/// replaced, which pushed the pid after issuing the kill without reading the
/// result. It stops here: what the facility SAID is what the caller reports.
///
/// `Err(kind == NotFound)` still means the kill could not be ATTEMPTED at all
/// (neither `taskkill` nor `pgrep` exists here, so nothing ran). These are the
/// answers of a facility that DID run.
#[derive(Debug)]
pub enum NameKill {
    /// Every match was killed. `pids` are the pids a signal was DELIVERED to,
    /// and it is EMPTY where the platform answers in aggregate: `taskkill /IM`
    /// reports that the name was killed and never which pids, so a Windows
    /// caller reports the NAME rather than ids nobody saw.
    Killed { pids: Vec<u32> },
    /// The kill RAN and the name matched NOTHING. A different statement from
    /// `Err(NotFound)` ("nothing could run") and from [`NameKill::Refused`]
    /// ("something matched and would not die").
    NoMatch,
    /// A match EXISTED and the kill was REFUSED — "Access is denied", or the pid
    /// went away between the match and the signal. `killed` carries the pids
    /// that WERE signalled, so a partial outcome is reported as partial instead
    /// of being rounded to either extreme; `refused` the ones that were not.
    /// Where the platform reports no pids at all (`taskkill /IM`), the caller
    /// names the name it asked for.
    Refused {
        killed: Vec<u32>,
        refused: Vec<u32>,
        why: String,
    },
}

/// Kill every process with a NAME.
///
/// Kept separate from [`kill_tree`] because the blast radius differs and a caller
/// must choose: a name can match several processes, and `/T` would take each
/// one's whole tree. This door deliberately takes no tree (`taskkill_name_args`
/// carries no `/T`) — on unix it uses the same shape the caller used to build by
/// hand: `pgrep -f <name>`, then a DIRECT signal to each match.
///
/// The result distinguishes what the caller must distinguish — see [`NameKill`]
/// for the three answers a run can give, and `Err(NotFound)` for the one case
/// where nothing ran at all. Any OTHER `Err` means the question could not be
/// answered (a helper that would not spawn, or Windows' `taskkill` failing where
/// `tasklist` could not say whether the name is still there): the caller reports
/// that as the failure it is, never as "nothing matched".
pub async fn kill_by_name(name: &str, force: bool) -> io::Result<NameKill> {
    #[cfg(windows)]
    return windows_kill_by_name(name, force).await;
    #[cfg(unix)]
    return unix_kill_by_name(name, force).await;
    #[cfg(not(any(unix, windows)))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        format!("no name kill for {name} on this platform"),
    ));
}

#[cfg(windows)]
async fn windows_kill_by_name(name: &str, force: bool) -> io::Result<NameKill> {
    let mut cmd = tokio::process::Command::new("taskkill");
    cmd.args(taskkill_name_args(name, force));
    match attempt("taskkill", &mut cmd).await {
        Ok(()) => Ok(NameKill::Killed { pids: Vec::new() }),
        // WHICH FAILURE THIS IS CANNOT BE READ OFF taskkill's TEXT: "not found"
        // and "Access is denied" arrive the same way, and the text is LOCALIZED
        // — matching English out of stderr is a guess that breaks on the first
        // non-English device. So ask the PROCESS TABLE instead: a name
        // `tasklist` can still see was REFUSED, not absent. If `tasklist` itself
        // cannot run, the question stays open and that is what `Err` says.
        Err(e) => match tasklist_matches_name(name).await {
            Ok(true) => Ok(NameKill::Refused {
                killed: Vec::new(),
                refused: Vec::new(),
                why: e.to_string(),
            }),
            Ok(false) => Ok(NameKill::NoMatch),
            Err(_) => Err(e),
        },
    }
}

/// The `tasklist` query behind [`windows_kill_by_name`]'s disambiguation, as
/// arguments.
///
/// `IMAGENAME eq` is the filter's own syntax (not localized), and `CSV` + `/NH`
/// is its machine-readable form: no header, one quoted row per match, and an
/// INFO line instead of rows when nothing matches. Pure, so the shape is pinned
/// on every platform — the same seam [`taskkill_args`] gives the kill flags.
#[cfg(any(windows, test))]
pub(crate) fn tasklist_query_args(name: &str) -> Vec<String> {
    vec![
        "/FI".to_string(),
        format!("IMAGENAME eq {name}"),
        "/NH".to_string(),
        "/FO".to_string(),
        "CSV".to_string(),
    ]
}

/// Did that `tasklist` output name this program? The IMAGE NAME is the first CSV
/// column, and the "no tasks are running" INFO line has no columns at all, so it
/// names nothing here.
#[cfg(any(windows, test))]
pub(crate) fn tasklist_saw_name(stdout: &str, name: &str) -> bool {
    stdout.lines().any(|line| {
        line.split(',')
            .next()
            .map(|first| first.trim().trim_matches('"').eq_ignore_ascii_case(name))
            .unwrap_or(false)
    })
}

/// Ask the process table whether `name` is still there — see
/// [`tasklist_query_args`]. The exit status is not the answer (tasklist exits 0
/// with its INFO line when nothing matches); the ROWS are.
#[cfg(windows)]
async fn tasklist_matches_name(name: &str) -> io::Result<bool> {
    let mut cmd = tokio::process::Command::new("tasklist");
    cmd.args(tasklist_query_args(name));
    hidden(&mut cmd);
    let out = cmd.output().await?;
    Ok(tasklist_saw_name(
        &String::from_utf8_lossy(&out.stdout),
        name,
    ))
}

#[cfg(unix)]
async fn unix_kill_by_name(name: &str, force: bool) -> io::Result<NameKill> {
    let mut pgrep = tokio::process::Command::new("pgrep");
    pgrep.args(["-f", name]);
    let out = pgrep
        .output()
        .await
        .map_err(|e| io::Error::new(e.kind(), format!("pgrep: {e}")))?;
    let pids: Vec<u32> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .collect();
    if pids.is_empty() {
        // NOT NotFound: `pgrep` ran and the answer is "nothing matched", which is
        // a different statement from "this device cannot kill by name" — and a
        // different one again from "a match existed and the signal was refused".
        return Ok(NameKill::NoMatch);
    }
    let sig = if force { "-9" } else { "-15" };
    let mut results: Vec<(u32, io::Result<()>)> = Vec::with_capacity(pids.len());
    for pid in pids {
        // TEST-ONLY: a refusal forced by the test seam. A real EPERM cannot be
        // produced by a suite that may run as root — the only reliable target is
        // a process the test does not own, and killing other people's processes
        // is not a test's call — so the refusal is injected exactly where a real
        // one would land and the REPORT is what gets asserted.
        #[cfg(test)]
        if test_records::signal_is_refused(pid) {
            results.push((
                pid,
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "kill: Operation not permitted (refusal forced by the test seam)",
                )),
            ));
            continue;
        }
        let mut kill = tokio::process::Command::new("kill");
        kill.args([sig, "--", &pid.to_string()]);
        // NO TREE, and the per-pid result is KEPT: a match that could not be
        // signalled is exactly the fact this door used to throw away, and the
        // caller cannot report honest work from a result nobody read.
        results.push((pid, attempt("kill", &mut kill).await));
    }
    Ok(classify_name_kill(results))
}

/// TURN PER-PID RESULTS INTO THE ONE ANSWER THE CALLER REPORTS.
///
/// Pure and platform-independent so the three answers are pinned wherever
/// `cargo test` runs, not only where `pgrep`/`kill` exist — the same reason
/// [`taskkill_args`] is a function instead of an inline chain.
#[cfg(any(unix, test))]
fn classify_name_kill(results: Vec<(u32, io::Result<()>)>) -> NameKill {
    let mut killed: Vec<u32> = Vec::new();
    let mut refused: Vec<u32> = Vec::new();
    let mut why = String::new();
    for (pid, result) in results {
        match result {
            Ok(()) => killed.push(pid),
            Err(e) => {
                refused.push(pid);
                if why.is_empty() {
                    why = e.to_string();
                }
            }
        }
    }
    if refused.is_empty() {
        NameKill::Killed { pids: killed }
    } else {
        NameKill::Refused {
            killed,
            refused,
            why,
        }
    }
}

/// TEST-ONLY: what [`kill_tree`] was asked to do.
///
/// The tree flag is pinned by [`taskkill_args`], but the flag's SHAPE is not the
/// round's defect — the defect was a CALL SITE that never reached the tree door
/// at all, and no test of a pure function can see that. So the door records its
/// requests and the call site's own test asserts that its request arrived; a
/// call site rewritten to run `taskkill`/`kill` by hand then fails that test.
/// Compiled only under `cfg(test)`.
#[cfg(test)]
pub(crate) mod test_records {
    use std::sync::Mutex;

    static TREE_REQUESTS: Mutex<Vec<(u32, bool)>> = Mutex::new(Vec::new());

    /// Note one request. A poisoned lock loses a test record; it never changes a
    /// kill, so there is nothing to propagate.
    pub(crate) fn tree_requested_note(pid: u32, force: bool) {
        if let Ok(mut requests) = TREE_REQUESTS.lock() {
            requests.push((pid, force));
        }
    }

    /// Was `kill_tree(pid, force)` requested in this process?
    pub(crate) fn saw_tree_request(pid: u32, force: bool) -> bool {
        TREE_REQUESTS
            .lock()
            .map(|requests| requests.iter().any(|&(p, f)| p == pid && f == force))
            .unwrap_or(false)
    }

    static SIGNAL_REFUSALS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

    /// Force [`super::kill_by_name`]'s unix arm to report pid `pid` as REFUSED.
    ///
    /// The same seam shape as [`tree_requested_note`], for the same reason: the
    /// fact under test is one no other arrangement can produce here. A refused
    /// signal is a real thing ("Access is denied" on Windows, EPERM on unix) but
    /// a suite that may run as ROOT cannot produce one safely — the only
    /// dependable EPERM target is a process the test does not own, and killing
    /// other people's processes is not a test's call. So the refusal is injected
    /// where a real one lands, and what the tests assert is the REPORT.
    #[cfg(unix)]
    pub(crate) fn refuse_signal_for(pid: u32) {
        if let Ok(mut refusals) = SIGNAL_REFUSALS.lock() {
            refusals.push(pid);
        }
    }

    /// Was a refusal FORCED for `pid`? Compiled only where the unix arm is, since
    /// that arm is the only reader.
    #[cfg(unix)]
    pub(crate) fn signal_is_refused(pid: u32) -> bool {
        SIGNAL_REFUSALS
            .lock()
            .map(|refusals| refusals.contains(&pid))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// THE FLAG'S CALL IS TESTABLE HERE; THE FLAG IS NOT.
    ///
    /// `hidden` is defined on every platform so that this call — the thing a
    /// spawn site must make — compiles and runs on Linux. It must be a NO-OP
    /// there (a Linux build that somehow tried to set a Windows creation flag
    /// would be a compile error, which is the other half of this test) and it
    /// must hand the command back, because call sites chain off it.
    #[test]
    fn hidden_is_callable_off_windows_and_returns_the_command_for_chaining() {
        let mut cmd = tokio::process::Command::new("summrise-no-such-program");
        cmd.arg("--first");
        hidden(&mut cmd).arg("--second");
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec!["--first".to_string(), "--second".to_string()],
            "hidden must return the same command so a call site can chain"
        );
    }

    /// THE DEFECT, PINNED ON EVERY PLATFORM.
    ///
    /// `system_process_kill`'s pid arm killed the parent alone because its
    /// `taskkill` arguments were built inline, where no Linux test could see
    /// them. The argument builder is now the seam, and this is the assertion the
    /// next rewrite has to get past: whatever else changes, the TREE flag is
    /// there, `/F` appears only when force is asked for, and the pid is the
    /// argument OF `/PID` rather than a stray token taskkill would ignore.
    #[test]
    fn taskkill_arguments_always_carry_the_tree_flag() {
        let graceful = taskkill_args(4242, false);
        assert_eq!(graceful, vec!["/T", "/PID", "4242"]);
        assert!(
            !graceful.iter().any(|a| a == "/F"),
            "a graceful kill must not force: {graceful:?}"
        );

        let forced = taskkill_args(4242, true);
        assert!(
            forced.contains(&"/T".to_string()),
            "the pid kill must take the tree: {forced:?}"
        );
        assert!(
            forced.contains(&"/F".to_string()),
            "force must reach taskkill: {forced:?}"
        );
        let pid_at = forced
            .iter()
            .position(|a| a == "/PID")
            .expect("/PID must be present");
        assert_eq!(forced[pid_at + 1], "4242", "the pid follows /PID");
    }

    /// THE NAME DOOR TAKES NO TREE, and that is a decision the arguments must
    /// keep: a name matches several processes, so `/T` here would take a whole
    /// tree per match.
    #[test]
    fn taskkill_name_arguments_never_carry_the_tree_flag() {
        for force in [false, true] {
            let args = taskkill_name_args("node.exe", force);
            assert!(
                !args.iter().any(|a| a == "/T"),
                "the name kill must not take each match's tree: {args:?}"
            );
            let name_at = args
                .iter()
                .position(|a| a == "/IM")
                .expect("/IM must be present");
            assert_eq!(args[name_at + 1], "node.exe", "the name follows /IM");
        }
    }

    /// pid 0 IS NOT "NO PROCESS" — on unix it is the CALLER'S OWN GROUP.
    ///
    /// The refusal is what makes `id().unwrap_or(0)` survivable at a call site
    /// that lost its child handle, so it is asserted rather than trusted: a door
    /// that passed 0 through would order the agent to kill itself.
    #[tokio::test]
    async fn kill_tree_refuses_pid_zero_rather_than_signalling_a_group() {
        let err = kill_tree(0, true)
            .await
            .expect_err("pid 0 must be refused, never signalled");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
    }

    /// A KILL THAT COULD NOT BE ATTEMPTED SAYS SO, and says WHICH tool was
    /// missing — the caller turns this into "cannot kill by name on this device"
    /// rather than "nothing matched".
    #[tokio::test]
    async fn a_missing_kill_tool_is_reported_as_not_found() {
        let mut cmd = tokio::process::Command::new("summrise-no-such-kill-tool");
        let err = attempt("summrise-no-such-kill-tool", &mut cmd)
            .await
            .expect_err("a tool that cannot be spawned is not a kill");
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
    }

    /// THE THREE ANSWERS A NAME KILL CAN GIVE, as data.
    ///
    /// The door used to discard every per-pid result, so "signalled" and
    /// "refused" were the same answer and both were reported as kills. Pinned
    /// here on every platform: a partial outcome must stay partial (the pid that
    /// WAS signalled is still reported), and a refusal must carry the tool's own
    /// words rather than being rounded to "nothing matched".
    #[test]
    fn a_name_kill_separates_what_died_from_what_would_not() {
        let killed = classify_name_kill(vec![(11, Ok(())), (12, Ok(()))]);
        assert!(
            matches!(&killed, NameKill::Killed { pids } if pids == &[11, 12]),
            "both signalled pids must be reported: {killed:?}"
        );

        let refused = classify_name_kill(vec![
            (11, Ok(())),
            (
                12,
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "kill: Operation not permitted",
                )),
            ),
        ]);
        match refused {
            NameKill::Refused {
                killed,
                refused,
                why,
            } => {
                assert_eq!(killed, vec![11], "a partial kill must report its success");
                assert_eq!(refused, vec![12], "and the pid that would not die");
                assert!(why.contains("Operation not permitted"), "{why}");
            }
            other => panic!("a refused signal must be reported as a refusal: {other:?}"),
        }
    }

    /// THE WINDOWS DISAMBIGUATOR'S TWO HALVES, pinned where they can be pinned:
    /// the filter as written, and the reading of `tasklist`'s rows. The CSV rows
    /// matter — the "no tasks are running" INFO line has no columns and must name
    /// nothing, or a Windows name kill would call every absent name a refusal.
    #[test]
    fn tasklist_rows_name_the_program_and_its_info_line_does_not() {
        assert_eq!(
            tasklist_query_args("node.exe"),
            vec!["/FI", "IMAGENAME eq node.exe", "/NH", "/FO", "CSV"]
        );
        let rows = "\"node.exe\",\"4242\",\"Console\",\"1\",\"12,345 K\"\r\n";
        assert!(tasklist_saw_name(rows, "node.exe"), "{rows:?}");
        assert!(
            tasklist_saw_name(rows, "NODE.EXE"),
            "image names are case-insensitive: {rows:?}"
        );
        assert!(!tasklist_saw_name(rows, "other.exe"), "{rows:?}");
        assert!(
            !tasklist_saw_name(
                "INFO: No tasks are running which match the specified criteria.\r\n",
                "node.exe"
            ),
            "the INFO line is not a row"
        );
    }

    /// THE RULE'S OTHER HALF: THE SITES MUST ASK — AND A FILE NOBODY LISTED IS
    /// STILL CAUGHT.
    ///
    /// `hidden` and `hidden_std` being defined on every platform is what makes
    /// this checkable at all — the flag itself cannot be observed on Linux and has
    /// no getter on Windows either — so what is asserted is the ASK. It is the half
    /// that drifts: a missing no-op is invisible to every behavioural test in this
    /// suite, which is how `system_process_kill`'s pid arm and the AI script
    /// runner both came to spawn a console binary without it.
    ///
    /// THE SCAN WALKS THE WHOLE TREE, because the first version of this gate
    /// held `SITES` and iterated over IT: a file that was not on the list was
    /// unguarded BY CONSTRUCTION, and the suite stayed green while
    /// `plugins/update/tools.rs` spawned `powershell` (and `tar`, both
    /// console-subsystem) with no ask at all. So the floor is now per SITE and
    /// over every `.rs` file under `src/`: a console-subsystem spawn either
    /// ASKS — and then its file must be in `SITES`, and the ask is counted —
    /// or its file must be named in `EXEMPT` with a reason. A console spawn in
    /// a file that is in neither list fails, whether that file is new or was
    /// simply never visited.
    ///
    /// AND IT KNOWS BOTH ENTRY POINTS ([`ASKS`]): the round that gave the blocking
    /// `std::process::Command` its own entry point moved four files from EXEMPT to
    /// ASKING, and a scan that had learned only `hidden(&mut …)` would have read
    /// every one of those sites as an unlisted console spawn — failing on the fix
    /// rather than on the defect.
    ///
    /// `EXEMPT` follows the sweep's exemption idiom: every entry carries its
    /// reason, and the entries this run did NOT need are printed (a list like
    /// this is weight the moment it stops being read, and a renamed file is
    /// exactly what a silent waiver hides). The entries that ARE needed are
    /// asserted by `the_console_spawn_exemptions_are_only_the_sites_that_cannot_ask`,
    /// because a print is not a gate.
    #[test]
    fn console_subsystem_spawn_sites_ask_for_the_flag() {
        let sources = read_src_sources();
        assert!(
            sources.len() > 20,
            "the scan found {} source files — a walk that sees almost nothing cannot \
             guard anything",
            sources.len()
        );

        let scan = scan_console_spawn_sites(&sources, SITES, EXEMPT);
        assert!(
            scan.unlisted.is_empty(),
            "console-subsystem spawn(s) in files that are neither in SITES (asking \
             `spawn::hidden`/`spawn::hidden_std`) nor named in EXEMPT with a reason — a file \
             nobody listed used to be unguarded by construction, which is how \
             `plugins/update/tools.rs` spawned powershell with no ask: {:?}",
            scan.unlisted
        );
        for file in SITES {
            let asks = scan.counted.get(*file).copied().unwrap_or(0);
            assert!(
                asks > 0,
                "{file}: no console-subsystem spawn site ASKED for the flag — a scan that \
                 matches nothing passes for the wrong reason"
            );
        }
        // The exemption list's own weight, printed the way the design sweep prints
        // its unused waivers: an entry this run did not need is either a gate for a
        // state that is currently absent or a stale path, and only a reader can
        // tell which.
        let unused: Vec<&str> = EXEMPT
            .iter()
            .map(|(f, _)| *f)
            .filter(|f| !scan.exempt_needed.contains(*f))
            .collect();
        if !unused.is_empty() {
            eprintln!(
                "note: {} of {} spawn-exemption entries matched no un-asking console-subsystem \
                 spawn in this scan — an exemption nothing needs is weight; prune it or say why \
                 it stays: {unused:?}",
                unused.len(),
                EXEMPT.len()
            );
        }
    }

    /// THE WAIVER LIST IS DOWN TO THE APPLIER ITSELF — AND IT IS ASSERTED, NOT
    /// TRUSTED.
    ///
    /// Every entry that used to be here for the BLOCKING command type is gone,
    /// because the reason those entries gave ("`hidden` is tokio-typed, so it
    /// cannot ask") stopped being true the moment `hidden_std` existed: a waiver
    /// whose only justification is a type boundary is not a decision, it is a hole
    /// in the rule, and leaving one behind would read as a site that still cannot
    /// ask. The two `cloudflared` entries that remained after that — named as a
    /// remainder rather than as a site that could not ask — are gone too: both
    /// are TOKIO spawns, which `hidden` reaches, so the only thing excusing them
    /// was scope. What is left is one file, and its reason a reader can check
    /// against the source:
    ///
    /// * `spawn.rs` — the applier itself, not a caller: `hidden`/`hidden_std` are
    ///   called inside it, and its taskkill/kill/pgrep spawns are covered one call
    ///   deeper, by `attempt`.
    ///
    /// THE `assert_eq!` IS THE EXACT LIST rather than a `contains`, because the
    /// failure being guarded against is an entry ADDED BACK: a new waiver for a
    /// site that can ask is exactly the drift this whole module exists to stop, and
    /// it must cost a test run rather than a line in a list nobody re-reads. The
    /// second half re-walks the real tree and requires every remaining entry to be
    /// NEEDED, so an exemption that outlives its spawn fails here even though the
    /// gate only prints it.
    #[test]
    fn the_console_spawn_exemptions_are_only_the_sites_that_cannot_ask() {
        let names: Vec<&str> = EXEMPT.iter().map(|(file, _)| *file).collect();
        assert_eq!(
            names,
            vec!["spawn.rs"],
            "the no-console rule reaches BOTH command types (`hidden` and `hidden_std`), so the \
             `cloudflared` spawns that were waived for scope now ask — the applier itself is the \
             only file that may still waive the ask"
        );
        for (file, reason) in EXEMPT {
            assert!(
                !reason.trim().is_empty(),
                "{file}: an exemption must carry its reason"
            );
        }
        let scan = scan_console_spawn_sites(&read_src_sources(), SITES, EXEMPT);
        let needed: Vec<&str> = names
            .iter()
            .copied()
            .filter(|file| scan.exempt_needed.contains(*file))
            .collect();
        assert_eq!(
            needed, names,
            "an exemption this scan did not need is weight rather than a waiver — the file it \
             names has no un-asking console-subsystem spawn left, so prune it or say why it stays"
        );
    }

    /// THE SECOND ENTRY POINT IS AN ASK — proved on a tree this test owns.
    ///
    /// The gate's own mutations are run against the real tree (a `hidden_std` ask
    /// deleted, an unlisted console spawn added); this is the same pair of facts on
    /// two literal sources, so the scan's reading of `hidden_std` is pinned by an
    /// assertion and not only by a mutation an implementer remembers to run. The
    /// sources are text rather than files on purpose: the scan is a function of
    /// text, and a test that writes files can leave them behind.
    ///
    /// The `Command::new(` inside these literals is invisible to the scan for the
    /// same reason prose is: the program expression there reads as `\"powershell\"`,
    /// which is not one of [`CONSOLE_PROGRAMS`].
    #[test]
    fn a_site_asking_through_hidden_std_satisfies_the_gate() {
        let asking = concat!(
            "fn probe() {\n",
            "    let mut c = std::process::Command::new(\"powershell\");\n",
            "    c.arg(\"-NoProfile\");\n",
            "    crate::spawn::hidden_std(&mut c);\n",
            "    let _ = c.output();\n",
            "}\n",
        );
        let silent = asking.replace("    crate::spawn::hidden_std(&mut c);\n", "");
        let sources = vec![
            ("asks_via_std.rs".to_string(), asking.to_string()),
            ("silent.rs".to_string(), silent),
        ];
        let scan = scan_console_spawn_sites(&sources, &["asks_via_std.rs"], &[]);
        assert_eq!(
            scan.counted.get("asks_via_std.rs").copied(),
            Some(1),
            "a spawn that asks through `hidden_std` must COUNT as an ask: {:?}",
            scan.counted
        );
        assert_eq!(
            scan.unlisted,
            vec!["silent.rs: `powershell`".to_string()],
            "the only unlisted spawn must be the one that never asked"
        );
    }

    /// AND THE OTHER DIRECTION OF THE SAME RULE: a file whose spawn ASKS but which
    /// nobody added to `SITES` is still a failure. The ask is what puts a file on
    /// the list, so a file that asks and is not listed is a rule nobody can find
    /// from the site — the same shape as the unlisted-spawn defect, one list over.
    #[test]
    #[should_panic(expected = "so the file belongs in SITES")]
    fn a_file_that_asks_but_is_not_in_sites_fails() {
        let sources = vec![(
            "asks_but_unlisted.rs".to_string(),
            concat!(
                "fn probe() {\n",
                "    let mut c = std::process::Command::new(\"powershell\");\n",
                "    crate::spawn::hidden_std(&mut c);\n",
                "    let _ = c.output();\n",
                "}\n",
            )
            .to_string(),
        )];
        let _ = scan_console_spawn_sites(&sources, &[], &[]);
    }

    /// THE ASK IS A CALL, NOT A WORD — for BOTH entry points, and the near-misses
    /// that must not count as one. A gate that accepted the name alone would pass
    /// on a site that only MENTIONS the flag in a comment, which is the state every
    /// one of these sites was in before the rule existed.
    #[test]
    fn both_entry_points_count_as_an_ask_and_prose_does_not() {
        assert!(gap_asks_for_the_flag("crate::spawn::hidden(&mut cmd);"));
        assert!(gap_asks_for_the_flag("crate::spawn::hidden_std(&mut cmd);"));
        assert!(
            !gap_asks_for_the_flag("// `hidden` is tokio-typed, so this site cannot ask"),
            "a mention of the name in prose is not an ask"
        );
        assert!(
            !gap_asks_for_the_flag("let flag = command_hidden();"),
            "a lookalike call is not an ask"
        );
        assert!(
            !gap_asks_for_the_flag("crate::spawn::hidden_std(cmd);"),
            "the ask passes `&mut`, which is what keeps a named call from counting"
        );
    }

    // THE PROGRAM CLASS, as the program expression is WRITTEN at the spawn
    // site: the rule's first bullet in `hidden`'s doc comment, which the two
    // lists have to agree with. Two spellings that need the mapping said out
    // loud — `&node` is the node.exe PathBuf every playwright site holds, and
    // `&cf` is the cloudflared.exe PathBuf the tunnel sites hold — because a
    // class written in spellings is only as good as the reader's ability to
    // recognise them. `ps` is here because the list always carried it (a unix
    // helper, so asking costs nothing and the site already does).
    const CONSOLE_PROGRAMS: &[&str] = &[
        "&node",
        "node",
        "shell",
        "cmd",
        "sh",
        "powershell",
        "taskkill",
        "tasklist",
        "ping",
        "tar",
        "where",
        "sc.exe",
        "schtasks",
        "reg",
        "icacls",
        "&cf",
        "ps",
    ];
    /// Files where a console-subsystem spawn's ASK is counted: every spawn
    /// that DOES ask (one of [`ASKS`] in its gap — the gap for one spawn runs
    /// to the NEXT one, because the ask is its own statement after the builder
    /// chain) must live in one of these, and each listed file must have at
    /// least one such ask, because a scan that matches nothing passes for the
    /// wrong reason.
    ///
    /// The four files at the end are the round that gave the blocking command
    /// type its own entry point: `main.rs` (the fix-tunnel `powershell`),
    /// `paths.rs` (`reg query`, `icacls`), `web/mod.rs` (the cloudflared
    /// `tasklist` probe) and `winmain.rs` (self-heal's `powershell`,
    /// `sc.exe` ×2, `schtasks` ×2). They were in `EXEMPT` because they could
    /// not ask; now they do.
    ///
    /// `tunnel.rs` arrived last, and it is the round that closed the REMAINDER
    /// rather than a type boundary: its five `cloudflared` CLI spawns (login,
    /// list, create, list again, route dns) were waived for scope alone, and
    /// `winmain.rs`'s supervised `cloudflared` with them — both TOKIO commands,
    /// which `hidden` reaches. Each was restructured out of a builder chain so
    /// its ask is its own statement, the shape `main.rs` already used.
    const SITES: &[&str] = &[
        "spawn.rs",
        "plugins/playwright/manager.rs",
        "plugins/playwright/tools.rs",
        "plugins/mcp_client/tools.rs",
        "plugins/terminal/tools/exec.rs",
        "plugins/system/tools.rs",
        "plugins/update/tools.rs",
        "main.rs",
        "paths.rs",
        "web/mod.rs",
        "winmain.rs",
        "tunnel.rs",
    ];
    /// Files where a console-subsystem spawn does NOT ask, each with the reason
    /// it cannot (or deliberately does not). A file may be in BOTH lists, and that
    /// means both things at once: `spawn.rs` asks for the `tasklist` that
    /// disambiguates a name kill while its taskkill/kill/pgrep spawns are covered
    /// one call deeper, by `attempt`.
    ///
    /// IT IS ONE ENTRY, and the two `cloudflared` waivers that stood beside it
    /// were DELETED rather than reworded: `hidden` reaches the tokio commands in
    /// both files, so "outside the round that wrote the rule" was a statement
    /// about scope, never about the site's ability to ask.
    const EXEMPT: &[(&str, &str)] = &[(
        "spawn.rs",
        "the applier, not a caller: `hidden`/`hidden_std` are called inside this module (in \
         those two functions and in `attempt`, through which taskkill/kill/pgrep run)",
    )];
    const SPAWN: &str = "Command::new(";
    // THE ASKS, as CALLS rather than words — ONE PER ENTRY POINT in `hidden`'s
    // module, and BOTH have to count: `hidden(&mut …)` for a tokio command,
    // `hidden_std(&mut …)` for a blocking `std` one. Requiring the call shape is
    // what keeps a mention of the name in prose — or an unrelated `…_hidden()`
    // test function further down the file — from counting as an ask for a site
    // that never made one.
    const ASKS: &[&str] = &["hidden(&mut ", "hidden_std(&mut "];

    /// Does the gap after one spawn site contain an ASK? The one place the two
    /// spellings are read, so a test can pin them without a source tree.
    fn gap_asks_for_the_flag(gap: &str) -> bool {
        ASKS.iter().any(|ask| gap.contains(ask))
    }

    /// ONE SCAN, SO THE GATE AND THE GATE'S OWN TESTS REACH THE SAME VERDICT.
    struct SpawnScan {
        /// file → how many console-subsystem spawns ASKED.
        counted: std::collections::BTreeMap<String, usize>,
        /// exempt files whose waiver this scan actually NEEDED.
        exempt_needed: std::collections::BTreeSet<String>,
        /// `file: \`program\`` for a console spawn in neither list.
        unlisted: Vec<String>,
    }

    /// Apply the rule to `sources` — `(path as SITES/EXEMPT write it, file text)`
    /// pairs, so the scan is a function of TEXT and a test can hand it literals.
    fn scan_console_spawn_sites(
        sources: &[(String, String)],
        sites: &[&str],
        exempt: &[(&str, &str)],
    ) -> SpawnScan {
        let mut scan = SpawnScan {
            counted: std::collections::BTreeMap::new(),
            exempt_needed: std::collections::BTreeSet::new(),
            unlisted: Vec::new(),
        };
        for (file, src) in sources {
            let in_sites = sites.contains(&file.as_str());
            let exempt = exempt.iter().find(|(f, _)| f == file);
            let mut from = 0;
            while let Some(at) = src[from..].find(SPAWN) {
                let at = from + at;
                from = at + SPAWN.len();
                // This needle is a string literal in THIS file; skip its own
                // occurrence rather than reading the source that holds it.
                if src[..at].ends_with('"') {
                    continue;
                }
                let after = &src[at + SPAWN.len()..];
                let next = after.find(SPAWN).unwrap_or(after.len());
                // The program expression, as written. One line at every site —
                // the guard is what keeps this scanner from reading prose (the
                // needle's own literal, a comment) as a spawn site, and a
                // wrapped expression would be skipped rather than misread.
                let Some(end) = after.find(')') else { continue };
                let program = after[..end].trim().trim_matches('"');
                if program.is_empty()
                    || program.len() > 40
                    || program.contains('\n')
                    || program.contains(';')
                    || !CONSOLE_PROGRAMS.contains(&program)
                {
                    continue;
                }
                if gap_asks_for_the_flag(&after[..next]) {
                    assert!(
                        in_sites,
                        "{file}: the spawn of `{program}` asks `spawn::hidden`/`spawn::hidden_std` \
                         for the no-console flag, so the file belongs in SITES — the rule is in \
                         src/spawn.rs"
                    );
                    *scan.counted.entry(file.clone()).or_default() += 1;
                    continue;
                }
                match exempt {
                    Some((_, reason)) => {
                        assert!(
                            !reason.trim().is_empty(),
                            "{file}: an exemption must carry its reason"
                        );
                        scan.exempt_needed.insert(file.clone());
                    }
                    None => scan.unlisted.push(format!("{file}: `{program}`")),
                }
            }
        }
        scan
    }

    /// Every `.rs` file under `src/`, as `(path relative to src/, text)` — the
    /// shape [`scan_console_spawn_sites`] reads, and the shape `SITES` and
    /// `EXEMPT` are written in. Walked rather than listed, because the defect the
    /// gate's floor exists for was a FILE nobody had added to a list.
    fn read_src_sources() -> Vec<(String, String)> {
        let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files: Vec<String> = Vec::new();
        collect_rs_files(&src_dir, &src_dir, &mut files);
        files.sort();
        files
            .into_iter()
            .map(|file| {
                let path = src_dir.join(&file);
                let text =
                    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
                (file, text)
            })
            .collect()
    }

    /// Every `.rs` file under `base`, as a path relative to `base` with `/`
    /// separators — what "the scan can see" means, and the shape `SITES` and
    /// `EXEMPT` are written in. Walked rather than listed, because the defect the
    /// floor above exists for was a FILE nobody had added to a list.
    fn collect_rs_files(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, base, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                if let Ok(rel) = path.strip_prefix(base) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }

    /// THE COMMANDS THIS MODULE RUNS ARE ONE COMMAND SHORT OF A REAL KILL, so
    /// the no-console rule has to reach them too: `attempt` is the only place
    /// they are spawned, and it asks for the flag on every platform.
    #[tokio::test]
    async fn attempt_applies_the_no_console_flag_and_runs_the_command() {
        let mut cmd = tokio::process::Command::new(if cfg!(windows) { "cmd" } else { "true" });
        if cfg!(windows) {
            cmd.args(["/C", "exit 0"]);
        }
        attempt("exit 0", &mut cmd)
            .await
            .expect("a command that exits 0 is a kill that was made");
    }
}
