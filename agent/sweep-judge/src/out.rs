//! Where the judge's two output halves go.
//!
//! The split is the interface's, not a preference: the findings go to STDERR (the JavaScript uses
//! `console.error`) and the `note:` lines to STDOUT (the JavaScript uses `console.log`). A caller that
//! wants the verdict reads the exit code and stderr; a caller that wants to see what was measured reads
//! stdout.
//!
//! Every line is flushed as it is written, so the interleaving of the two streams is the order the judge
//! called them in — which is what the JavaScript produces when both are redirected to one file.

use std::io::Write;

pub struct Out {
    stdout: std::io::Stdout,
    stderr: std::io::Stderr,
    /// `Some((stdout_lines, stderr_lines))` when the caller wants the lines rather than the streams.
    /// The unit tests use it; so can anything that would otherwise have to spawn a process to read them.
    captured: Option<(Vec<String>, Vec<String>)>,
}

impl Default for Out {
    fn default() -> Self {
        Self::new()
    }
}

impl Out {
    pub fn new() -> Self {
        Self {
            stdout: std::io::stdout(),
            stderr: std::io::stderr(),
            captured: None,
        }
    }

    /// An `Out` that keeps everything in memory.
    pub fn capture() -> Self {
        Self {
            stdout: std::io::stdout(),
            stderr: std::io::stderr(),
            captured: Some((Vec::new(), Vec::new())),
        }
    }

    /// The `note:` lines written so far, in order. Empty for a real `Out`.
    pub fn notes(&self) -> &[String] {
        self.captured
            .as_ref()
            .map(|(o, _)| o.as_slice())
            .unwrap_or(&[])
    }

    /// The stderr lines written so far, in order. Empty for a real `Out`.
    pub fn errors(&self) -> &[String] {
        self.captured
            .as_ref()
            .map(|(_, e)| e.as_slice())
            .unwrap_or(&[])
    }

    /// `console.log(line)`. A line may carry embedded newlines; one is appended.
    pub fn note(&mut self, line: &str) {
        if let Some((o, _)) = &mut self.captured {
            o.push(line.to_string());
            return;
        }
        let mut h = self.stdout.lock();
        let _ = writeln!(h, "{line}");
        let _ = h.flush();
    }

    /// `console.error(line)`.
    pub fn err(&mut self, line: &str) {
        if let Some((_, e)) = &mut self.captured {
            e.push(line.to_string());
            return;
        }
        let mut h = self.stderr.lock();
        let _ = writeln!(h, "{line}");
        let _ = h.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_capturing_out_keeps_the_two_streams_apart_and_in_order() {
        let mut out = Out::capture();
        out.note("note: one");
        out.err("a finding");
        out.note("note: two");
        assert_eq!(out.notes(), ["note: one", "note: two"]);
        assert_eq!(out.errors(), ["a finding"]);
    }
}
