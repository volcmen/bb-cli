//! The terminal IO layer: [`IoStreams`] (real + in-memory test constructor) and
//! [`ColorScheme`]. Mirrors `gh`'s `iostreams` package.

use std::io::{BufRead, Read, Write};
use std::sync::{Arc, Mutex};

/// A writer that appends into a shared in-memory buffer (used by tests).
#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("buffer poisoned")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Captured stdout/stderr buffers returned by [`IoStreams::test`].
#[derive(Clone)]
pub struct TestBuffers {
    out: Arc<Mutex<Vec<u8>>>,
    err: Arc<Mutex<Vec<u8>>>,
}

impl TestBuffers {
    /// Everything written to stdout so far, as a string.
    #[must_use]
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().expect("buffer poisoned")).into_owned()
    }

    /// Everything written to stderr so far, as a string.
    #[must_use]
    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.err.lock().expect("buffer poisoned")).into_owned()
    }
}

/// Standard streams plus TTY/color state. Construct with [`IoStreams::system`]
/// for the real process streams, or [`IoStreams::test`] for in-memory buffers.
pub struct IoStreams {
    out: Mutex<Box<dyn Write + Send>>,
    err: Mutex<Box<dyn Write + Send>>,
    input: Mutex<Box<dyn BufRead + Send>>,
    color_enabled: bool,
    stdout_tty: bool,
    stderr_tty: bool,
    stdin_tty: bool,
    never_prompt: bool,
}

fn detect_color(stdout_tty: bool) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if let Ok(v) = std::env::var("CLICOLOR_FORCE") {
        if v != "0" {
            return true;
        }
    }
    stdout_tty
}

impl IoStreams {
    /// The real process streams, with TTY + color auto-detected.
    #[must_use]
    pub fn system() -> Self {
        use std::io::IsTerminal;
        let stdout_tty = std::io::stdout().is_terminal();
        let stderr_tty = std::io::stderr().is_terminal();
        let stdin_tty = std::io::stdin().is_terminal();
        Self {
            out: Mutex::new(Box::new(std::io::stdout())),
            err: Mutex::new(Box::new(std::io::stderr())),
            input: Mutex::new(Box::new(std::io::BufReader::new(std::io::stdin()))),
            color_enabled: detect_color(stdout_tty),
            stdout_tty,
            stderr_tty,
            stdin_tty,
            never_prompt: false,
        }
    }

    /// In-memory streams for tests. Returns the streams plus the captured
    /// stdout/stderr buffers. TTYs default to `false` and prompting is disabled;
    /// flip with [`IoStreams::set_stdout_tty`] etc. before wrapping in an `Arc`.
    #[must_use]
    pub fn test() -> (Self, TestBuffers) {
        let out = Arc::new(Mutex::new(Vec::new()));
        let err = Arc::new(Mutex::new(Vec::new()));
        let streams = Self {
            out: Mutex::new(Box::new(SharedBuf(out.clone()))),
            err: Mutex::new(Box::new(SharedBuf(err.clone()))),
            input: Mutex::new(Box::new(std::io::Cursor::new(Vec::new()))),
            color_enabled: false,
            stdout_tty: false,
            stderr_tty: false,
            stdin_tty: false,
            never_prompt: true,
        };
        (streams, TestBuffers { out, err })
    }

    /// Write to stdout (no trailing newline).
    pub fn print(&self, s: &str) {
        let mut o = self.out.lock().expect("stdout poisoned");
        let _ = o.write_all(s.as_bytes());
    }

    /// Write a line to stdout.
    pub fn println(&self, s: &str) {
        let mut o = self.out.lock().expect("stdout poisoned");
        let _ = writeln!(o, "{s}");
    }

    /// Write a line to stderr.
    pub fn eprintln(&self, s: &str) {
        let mut e = self.err.lock().expect("stderr poisoned");
        let _ = writeln!(e, "{s}");
    }

    /// Read all of stdin to a string (for `--body-file -` and similar).
    ///
    /// # Errors
    /// Returns any IO error encountered while reading.
    pub fn read_stdin_to_string(&self) -> std::io::Result<String> {
        let mut buf = String::new();
        self.input
            .lock()
            .expect("stdin poisoned")
            .read_to_string(&mut buf)?;
        Ok(buf)
    }

    #[must_use]
    pub fn is_stdout_tty(&self) -> bool {
        self.stdout_tty
    }

    #[must_use]
    pub fn is_stderr_tty(&self) -> bool {
        self.stderr_tty
    }

    #[must_use]
    pub fn is_stdin_tty(&self) -> bool {
        self.stdin_tty
    }

    pub fn set_stdout_tty(&mut self, v: bool) {
        self.stdout_tty = v;
        self.color_enabled = detect_color(v);
    }

    pub fn set_stderr_tty(&mut self, v: bool) {
        self.stderr_tty = v;
    }

    pub fn set_stdin_tty(&mut self, v: bool) {
        self.stdin_tty = v;
    }

    pub fn set_never_prompt(&mut self, v: bool) {
        self.never_prompt = v;
    }

    #[must_use]
    pub fn color_enabled(&self) -> bool {
        self.color_enabled
    }

    /// Whether interactive prompting is possible (TTY in+out and not disabled).
    #[must_use]
    pub fn can_prompt(&self) -> bool {
        !self.never_prompt && self.stdin_tty && self.stdout_tty
    }

    /// A color scheme bound to this stream's color setting.
    #[must_use]
    pub fn color_scheme(&self) -> ColorScheme {
        ColorScheme {
            enabled: self.color_enabled,
        }
    }
}

/// ANSI color helpers, gated on whether color is enabled for the stream.
#[derive(Debug, Clone, Copy)]
pub struct ColorScheme {
    enabled: bool,
}

impl ColorScheme {
    fn wrap(self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }

    #[must_use]
    pub fn bold(self, s: &str) -> String {
        self.wrap("1", s)
    }

    #[must_use]
    pub fn red(self, s: &str) -> String {
        self.wrap("31", s)
    }

    #[must_use]
    pub fn green(self, s: &str) -> String {
        self.wrap("32", s)
    }

    #[must_use]
    pub fn yellow(self, s: &str) -> String {
        self.wrap("33", s)
    }

    #[must_use]
    pub fn cyan(self, s: &str) -> String {
        self.wrap("36", s)
    }

    #[must_use]
    pub fn gray(self, s: &str) -> String {
        self.wrap("90", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streams_capture_output() {
        let (io, bufs) = IoStreams::test();
        io.println("hello");
        io.eprintln("oops");
        assert_eq!(bufs.stdout_string(), "hello\n");
        assert_eq!(bufs.stderr_string(), "oops\n");
    }

    #[test]
    fn color_scheme_disabled_is_plain() {
        let cs = ColorScheme { enabled: false };
        assert_eq!(cs.bold("x"), "x");
    }

    #[test]
    fn color_scheme_enabled_wraps() {
        let cs = ColorScheme { enabled: true };
        assert_eq!(cs.bold("x"), "\x1b[1mx\x1b[0m");
    }

    #[test]
    fn no_prompt_in_test_mode() {
        let (io, _) = IoStreams::test();
        assert!(!io.can_prompt());
    }
}
