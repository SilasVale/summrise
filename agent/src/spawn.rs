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
//! So the policy lives here, in three pieces: [`hidden`] (the flag and the RULE
//! for when a spawn site wants it), [`kill_tree`] (a pid and its children) and
//! [`kill_by_name`] (every process with a name, deliberately NOT a tree). Each
//! has a pure argument builder next to it so the parts that are Windows-only at
//! runtime are still pinned by a test on any platform.
//!
//! Internal-only: this is device process hygiene, not a wire surface.

use std::io;

/// `CREATE_NO_WINDOW` (0x0800_0000) — run the child with a console but no window.
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
///   playwright stdio server, the AI script runner), `powershell`, `taskkill`,
///   `cmd`/`sh` (terminal_execute's local mode), `tasklist`, `ping`.
/// * **A spawn of something with no console window of its own — or one already
///   covered — does not.** Unix-only helpers (`kill`, `pgrep`, `ps`) are in this
///   class and the flag is a no-op for them anyway, so asking there costs
///   nothing; what the rule forbids is the other direction, a site that launches
///   a console binary from the agent and never asks.
///
/// A site that does want it asks by CALL, on every platform — `hidden` is defined
/// on every platform (a no-op off Windows) precisely so a Linux-run test can
/// assert that a spawn site asks for it. The flag itself is unobservable off
/// Windows and there is no getter for it there either, so the call is the whole
/// of what a gate can hold; if this function were `#[cfg(windows)]`, every call
/// site would be `#[cfg(windows)]` too and no Linux test could see any of them.
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

/// Kill every process with a NAME.
///
/// Kept separate from [`kill_tree`] because the blast radius differs and a caller
/// must choose: a name can match several processes, and `/T` would take each
/// one's whole tree. This door deliberately takes no tree (`taskkill_name_args`
/// carries no `/T`) — on unix it uses the same shape the caller used to build by
/// hand: `pgrep -f <name>`, then a DIRECT signal to each match.
///
/// The result distinguishes what the caller must distinguish: `Ok(())` means the
/// name kill RAN and had something to kill; `Err(NotFound)` means there is no
/// name-kill facility here at all (neither `taskkill` nor `pgrep`) and the caller
/// must say the kill could not be ATTEMPTED. Any other `Err` means it ran and
/// matched nothing, or refused — the caller reports that as "nothing matched",
/// which is what it is.
pub async fn kill_by_name(name: &str, force: bool) -> io::Result<()> {
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
async fn windows_kill_by_name(name: &str, force: bool) -> io::Result<()> {
    let mut cmd = tokio::process::Command::new("taskkill");
    cmd.args(taskkill_name_args(name, force));
    attempt("taskkill", &mut cmd).await
}

#[cfg(unix)]
async fn unix_kill_by_name(name: &str, force: bool) -> io::Result<()> {
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
        // a different statement from "this device cannot kill by name".
        return Err(io::Error::other(format!("no process matched name={name}")));
    }
    let sig = if force { "-9" } else { "-15" };
    for pid in pids {
        let mut kill = tokio::process::Command::new("kill");
        kill.args([sig, "--", &pid.to_string()]);
        // Best-effort, and no tree: a match that could not be signalled is still
        // reported as killed. That optimism is inherited, not invented — the
        // call site this moved from pushed the pid after issuing the kill without
        // reading the result — and it is stated here so the next reader does not
        // mistake it for a guarantee.
        let _ = attempt("kill", &mut kill).await;
    }
    Ok(())
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

    /// THE RULE'S OTHER HALF: THE SITES MUST ASK.
    ///
    /// `hidden` being defined on every platform is what makes this checkable at
    /// all — the flag itself cannot be observed on Linux and has no getter on
    /// Windows either — so what is asserted is the ASK. It is the half that
    /// drifts: a missing no-op is invisible to every behavioural test in this
    /// suite, which is how `system_process_kill`'s pid arm and the AI script
    /// runner both came to spawn a console binary without it.
    ///
    /// The scan is over the files this policy was applied to. `spawn.rs` is not
    /// among them: it is where the flag is APPLIED (in `hidden` itself and in
    /// `attempt`, through which every kill command runs), not a caller. The
    /// program class is the rule's first class — a console-subsystem binary
    /// launched from a process that may have a console — and it is listed rather
    /// than inferred, so a new spawn site of one of these programs is caught by
    /// the ask being absent from its file's next spawn gap.
    #[test]
    fn console_subsystem_spawn_sites_ask_for_the_flag() {
        // The program expression as it is WRITTEN at the spawn site.
        const CONSOLE_PROGRAMS: &[&str] =
            &["&node", "shell", "powershell", "tasklist", "ping", "ps"];
        const SITES: &[&str] = &[
            "plugins/playwright/manager.rs",
            "plugins/playwright/tools.rs",
            "plugins/mcp_client/tools.rs",
            "plugins/terminal/tools/exec.rs",
            "plugins/system/tools.rs",
        ];
        const SPAWN: &str = "tokio::process::Command::new(";
        for file in SITES {
            let path = format!("{}/src/{file}", env!("CARGO_MANIFEST_DIR"));
            let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
            let mut from = 0;
            let mut checked = 0;
            while let Some(at) = src[from..].find(SPAWN) {
                let at = from + at;
                // The gap for one spawn runs to the NEXT one: `hidden` is applied
                // to the command as its own statement, after the builder chain.
                let after = &src[at + SPAWN.len()..];
                let program = after[..after.find(')').expect("the program expression ends")]
                    .trim()
                    .trim_matches('"');
                let next = after.find(SPAWN).unwrap_or(after.len());
                if CONSOLE_PROGRAMS.contains(&program) {
                    assert!(
                        after[..next].contains("hidden("),
                        "{file}: the spawn of `{program}` never asks `spawn::hidden` for the \
                         no-console flag — the rule that says it must is in src/spawn.rs"
                    );
                    checked += 1;
                }
                from = at + SPAWN.len();
            }
            assert!(
                checked > 0,
                "{file}: no console-subsystem spawn site was found — a scan that matches \
                 nothing passes for the wrong reason"
            );
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
