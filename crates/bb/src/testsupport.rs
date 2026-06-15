//! Test-only support: scripted fakes and a `Context` builder, so command tests
//! are hermetic (the analog of `gh`'s `runCommand` helper + prompt stubber).
//!
//! Available only under `cfg(test)`.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use bb_core::{
    Browser, ConfigProvider, Context, GitClient, IoStreams, PromptError, Prompter, TestBuffers,
    Transport,
};

/// A [`Prompter`] that returns pre-scripted answers, panicking on an unexpected
/// prompt (the analog of `gh`'s `AskStubber`).
#[derive(Default)]
pub struct ScriptedPrompter {
    confirms: Mutex<VecDeque<bool>>,
    inputs: Mutex<VecDeque<String>>,
    passwords: Mutex<VecDeque<String>>,
    selects: Mutex<VecDeque<usize>>,
    editors: Mutex<VecDeque<String>>,
}

impl ScriptedPrompter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn confirm(self, v: bool) -> Self {
        self.confirms.lock().unwrap().push_back(v);
        self
    }

    #[must_use]
    pub fn input(self, v: &str) -> Self {
        self.inputs.lock().unwrap().push_back(v.to_owned());
        self
    }

    #[must_use]
    pub fn password(self, v: &str) -> Self {
        self.passwords.lock().unwrap().push_back(v.to_owned());
        self
    }

    #[must_use]
    pub fn select(self, v: usize) -> Self {
        self.selects.lock().unwrap().push_back(v);
        self
    }

    #[must_use]
    pub fn editor(self, v: &str) -> Self {
        self.editors.lock().unwrap().push_back(v.to_owned());
        self
    }
}

fn pop<T>(q: &Mutex<VecDeque<T>>, what: &str) -> Result<T, PromptError> {
    q.lock()
        .unwrap()
        .pop_front()
        .ok_or_else(|| PromptError::Other(format!("unexpected {what} prompt")))
}

impl Prompter for ScriptedPrompter {
    fn confirm(&self, _message: &str, _default: bool) -> Result<bool, PromptError> {
        pop(&self.confirms, "confirm")
    }

    fn input(&self, _message: &str, _default: Option<&str>) -> Result<String, PromptError> {
        pop(&self.inputs, "input")
    }

    fn password(&self, _message: &str) -> Result<String, PromptError> {
        pop(&self.passwords, "password")
    }

    fn select(&self, _message: &str, _options: &[String]) -> Result<usize, PromptError> {
        pop(&self.selects, "select")
    }

    fn editor(&self, _message: &str, _initial: &str) -> Result<String, PromptError> {
        pop(&self.editors, "editor")
    }
}

/// A [`Browser`] that records the last URL "opened" instead of launching one.
#[derive(Default)]
pub struct RecordingBrowser {
    pub urls: Mutex<Vec<String>>,
}

impl Browser for RecordingBrowser {
    fn browse(&self, url: &str) -> Result<(), std::io::Error> {
        self.urls.lock().unwrap().push(url.to_owned());
        Ok(())
    }
}

/// Build a test [`Context`] from injected fakes, returning the captured
/// stdout/stderr buffers. TTY state defaults to `tty`.
pub fn test_context(
    transport: Arc<dyn Transport>,
    git: Arc<dyn GitClient>,
    config: Arc<dyn ConfigProvider>,
    prompter: Arc<dyn Prompter>,
    tty: bool,
) -> (Context, TestBuffers) {
    let (mut io, bufs) = IoStreams::test();
    io.set_stdout_tty(tty);
    io.set_stderr_tty(tty);
    io.set_stdin_tty(tty);
    let ctx = Context {
        io: Arc::new(io),
        prompter,
        browser: Arc::new(RecordingBrowser::default()),
        git,
        config,
        transport,
        app_version: "test".to_owned(),
        repo_override: None,
    };
    (ctx, bufs)
}
