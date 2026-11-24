//! A child process the app owns, and the operating system's handle that ends its tree with the app.
//!
//! `std::process::Child` only reaches the process it started, and only when something drops it. A
//! headless browser is a tree (the browser, its renderers, GPU and utility children), and the device
//! can end without unwinding: the launcher's restart and a crash both skip every destructor. So the
//! ownership is the operating system's, not a `Drop` alone:
//!
//! - **Unix:** the child leads its own process group (`process_group(0)`), and dropping the owner
//!   sends `SIGKILL` to the group, which includes the renderers that inherited it.
//! - **Windows:** the child is assigned to a Job Object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The kernel closes the app's handle when the process ends
//!   however it ends, and closing the last handle kills every process still in the job — a device
//!   killed by `TerminateProcess` therefore leaves no `chrome-headless-shell` behind.
//!
//! The type is the only place a browser child is started, so the ownership cannot be forgotten by a
//! new call site.

#[cfg(unix)]
use nix::sys::signal::{killpg, Signal};
#[cfg(unix)]
use nix::unistd::Pid;
use std::io;
use std::process::{Child, Command, ExitStatus};

/// A child process, owned with the operating system's tree-level lifetime.
pub(crate) struct OwnedChild {
    child: Child,
    /// The job the child and its descendants belong to. Dropping this field closes the last handle
    /// in the common case; on an abrupt process death the kernel closes it and the job kills the
    /// browser.
    #[cfg(windows)]
    job: summrise_winsys::KillOnCloseJob,
}

impl OwnedChild {
    /// Start `command` under this ownership.
    ///
    /// On Windows the child is assigned to its kill-on-close job immediately after it is spawned, so
    /// a browser that starts children later (renderers are created on demand) has all of them in the
    /// job. A child this crate cannot put in the job is killed and the error returned: an unowned
    /// browser is the leak this type exists to prevent.
    pub(crate) fn spawn(command: Command) -> io::Result<OwnedChild> {
        #[cfg(unix)]
        let mut command = {
            use std::os::unix::process::CommandExt;
            let mut command = command;
            command.process_group(0);
            command
        };
        #[cfg(not(unix))]
        let mut command = command;

        // Windows mutates the child while assigning the job and on that failure path; Unix does
        // not touch it again before the ownership is built.
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut child = command.spawn()?;
        #[cfg(windows)]
        let job = match summrise_winsys::KillOnCloseJob::assign(child.id()) {
            Ok(job) => job,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        Ok(OwnedChild {
            child,
            #[cfg(windows)]
            job,
        })
    }

    /// The direct child's process id.
    ///
    /// Unix needs it for the group kill and the tests observe it; a Windows release reaches the
    /// child through `Child::id` while assigning the job and never needs this method.
    #[cfg(any(unix, all(test, target_os = "linux")))]
    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    /// Whether the direct child has exited, reaping it when it has; unchanged from `Child`.
    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// Kill the tree and reap the direct child.
    fn terminate(&mut self) {
        #[cfg(unix)]
        {
            // `try_wait` is the ownership check: a child that already exited was reaped by a
            // previous call, and its pid may have been recycled, so its group is not safe to
            // signal. The same reasoning as the terminal capability's session close.
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            if let Ok(pid) = i32::try_from(self.id()) {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
            }
        }
        #[cfg(windows)]
        {
            // A job handle is not a pid and cannot be recycled, so this is safe even when the
            // direct child has already exited and left a renderer behind.
            let _ = self.job.terminate();
        }
        // The group/job signal is the tree kill; this is the direct child on platforms where the
        // tree call cannot reach it, and the reap on every platform.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Observing a pid through `kill(pid, 0)` is the Unix test; a Windows test would need a second
/// Win32 call this crate does not carry, and the Windows lane is a compile check, not a run.
#[cfg(all(test, unix))]
mod tests {
    use std::time::{Duration, Instant};

    use super::OwnedChild;

    /// The pid is running as far as the OS is concerned.
    fn is_running(pid: u32) -> bool {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
    }

    /// Wait for `pid` to be gone; returns whether it is.
    fn gone(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !is_running(pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        !is_running(pid)
    }

    /// A child spawned through the ownership type does not outlive it: dropping the owner kills
    /// and reaps the child. This is the guarantee a restart of the device relies on.
    #[test]
    fn dropping_the_owned_child_kills_it() {
        let mut command = std::process::Command::new("sleep");
        command.arg("60");
        let owned = OwnedChild::spawn(command).expect("the sleeper starts");
        let pid = owned.id();
        assert!(
            is_running(pid),
            "the child runs before its owner is dropped"
        );

        drop(owned);

        assert!(
            gone(pid),
            "dropping the ownership type kills and reaps the child"
        );
    }

    /// The ownership ends the whole process group, not only the process it started: a Chrome
    /// renderer is a grandchild, and a device restart must leave none of them behind.
    #[test]
    fn dropping_the_owned_child_kills_the_group_it_leads() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "summrise-owned-child-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("the test directory is creatable");
        let pid_file = dir.join("grandchild.pid");
        let mut command = std::process::Command::new("sh");
        command
            .arg("-c")
            .arg("sleep 60 & echo $! > \"$1\"; wait")
            .arg("sh")
            .arg(&pid_file);
        let owned = OwnedChild::spawn(command).expect("the shell starts");
        let leader = owned.id();

        let deadline = Instant::now() + Duration::from_secs(5);
        let grandchild = loop {
            if let Ok(text) = std::fs::read_to_string(&pid_file) {
                if let Ok(pid) = text.trim().parse::<u32>() {
                    break pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the shell wrote its background child's pid"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(
            is_running(grandchild),
            "the grandchild runs before the drop"
        );

        drop(owned);

        assert!(gone(leader), "the group leader is killed");
        assert!(
            gone(grandchild),
            "the group the owner led is killed too, not just its direct child"
        );
        let _ = std::fs::remove_file(&pid_file);
        let _ = std::fs::remove_dir(&dir);
    }
}
