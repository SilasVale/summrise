//! The two halves of the frame contract, checked against the real program.
//!
//! The unit tests in `lib.rs` prove the writer and the reader agree with each other. This file proves
//! they agree with the **binary that is actually staged onto a target machine** — which is the thing
//! that matters, because the caller and the helper are compiled into different artifacts and shipped
//! to different operating systems. A round trip inside one process cannot see that drift; spawning
//! the real helper can.
//!
//! `CARGO_BIN_EXE_<name>` is set by cargo for integration tests, so this runs the same executable the
//! staging step would copy.

#![cfg(unix)]

use std::io::Write;
use std::process::{Command, Stdio};

const HELPER: &str = env!("CARGO_BIN_EXE_summrise-exec-argv");

/// Run the helper with `argv` in a frame, plus `child_stdin` written after the frame, and return
/// (stdout, exit code).
fn run(argv: &[&[u8]], child_stdin: &[u8]) -> (Vec<u8>, i32) {
    let mut child = Command::new(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the helper binary spawns");

    {
        let stdin = child.stdin.as_mut().expect("stdin is piped");
        let mut frame = Vec::new();
        summrise_exec_argv::write_frame(&mut frame, argv).expect("the frame is writable");
        stdin.write_all(&frame).expect("the frame is written");
        // Everything after the frame is the target's own stdin, which is exactly what the helper
        // must leave untouched.
        stdin
            .write_all(child_stdin)
            .expect("the payload is written");
    }
    drop(child.stdin.take());

    let out = child.wait_with_output().expect("the helper is waitable");
    (
        out.stdout,
        out.status.code().expect("the helper exited with a code"),
    )
}

#[test]
fn the_writer_and_the_staged_binary_agree_on_a_hostile_argv() {
    // Under a shell these four elements are not four elements: `$(id)` becomes a command
    // substitution and `two words` becomes two arguments. Through the frame they are bytes.
    let (stdout, code) = run(
        &[b"/bin/echo", b"two words", b"$(id)", b"it's \"quoted\""],
        b"",
    );
    assert_eq!(code, 0);
    assert_eq!(
        String::from_utf8_lossy(&stdout).trim_end(),
        "two words $(id) it's \"quoted\""
    );
}

#[test]
fn an_empty_element_reaches_the_target_as_an_empty_element() {
    // The reason the frame is length-prefixed: `/bin/echo` with three arguments, the middle one
    // empty, prints two spaces. A NUL-terminated frame would have delivered two arguments.
    let (stdout, code) = run(&[b"/bin/echo", b"a", b"", b"b"], b"");
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&stdout).trim_end(), "a  b");
}

#[test]
fn the_target_inherits_stdin_at_the_byte_after_the_frame() {
    // The property that makes the frame usable at all: the helper must not read — or buffer — one
    // byte beyond its own frame, or the child's stdin is consumed by a process that then execs.
    let payload = b"the target's own stdin\n";
    let (stdout, code) = run(&[b"/bin/cat"], payload);
    assert_eq!(code, 0);
    assert_eq!(stdout, payload);
}

#[test]
fn the_targets_exit_status_is_the_helpers_exit_status() {
    // On success there is no helper left to report anything: it exec'd. So the status a caller reads
    // is the target's own, which is the only thing the subprocess seam can accept.
    let (_, code) = run(&[b"/bin/sh", b"-c", b"exit 42"], b"");
    assert_eq!(code, 42);
}

#[test]
fn a_program_that_is_not_there_exits_127() {
    let (_, code) = run(&[b"/nonexistent/summrise-test-program"], b"");
    assert_eq!(code, 127);
}
