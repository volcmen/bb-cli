//! Error model and process exit codes, mirroring `gh`.

/// Process exit codes, mirroring `gh` (`OK=0`, `Error=1`, `Cancel=2`, `Auth=4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ExitCode {
    Ok = 0,
    Error = 1,
    Cancel = 2,
    Auth = 4,
}

impl ExitCode {
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            ExitCode::Ok => 0,
            ExitCode::Error => 1,
            ExitCode::Cancel => 2,
            ExitCode::Auth => 4,
        }
    }

    /// The code as a `u8`, for `std::process::ExitCode::from`.
    #[must_use]
    pub fn as_u8(self) -> u8 {
        match self {
            ExitCode::Ok => 0,
            ExitCode::Error => 1,
            ExitCode::Cancel => 2,
            ExitCode::Auth => 4,
        }
    }
}

/// A flag/usage error raised during command execution: the message is printed
/// and the process exits with code 1. (clap's own parse-time usage errors,
/// e.g. an unknown flag, exit with code 2.)
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct FlagError(pub String);

impl FlagError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// An error that should exit non-zero with no printed message (the message was
/// already shown).
#[derive(Debug, thiserror::Error)]
#[error("")]
pub struct SilentError;

/// Authentication is required or invalid for `hostname`. Exits with code 4.
#[derive(Debug, thiserror::Error)]
#[error("authentication required for {hostname}")]
pub struct AuthError {
    pub hostname: String,
}

impl AuthError {
    pub fn new(hostname: impl Into<String>) -> Self {
        Self {
            hostname: hostname.into(),
        }
    }
}

/// The user cancelled (Ctrl-C / prompt interrupt). Exits with code 2.
#[derive(Debug, thiserror::Error)]
#[error("cancelled")]
pub struct CancelError;

/// A single field-level error item from a Bitbucket API error response.
#[derive(Debug, Clone)]
pub struct ApiErrorItem {
    pub field: Option<String>,
    pub message: String,
}

/// Errors from the HTTP/API layer.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// A non-2xx HTTP response.
    #[error("HTTP {status} on {url}: {message}")]
    Http {
        status: u16,
        url: String,
        message: String,
        errors: Vec<ApiErrorItem>,
    },
    /// A transport/connection failure (DNS, TLS, timeout, ...).
    #[error("network error: {0}")]
    Network(String),
    /// The response body could not be decoded as expected.
    #[error("failed to decode response: {0}")]
    Decode(String),
}

impl ApiError {
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            ApiError::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    #[must_use]
    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(401)
    }

    #[must_use]
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404)
    }

    /// `410 Gone` — Bitbucket returns this for a feature that is disabled on the
    /// repository (e.g. the issue tracker).
    #[must_use]
    pub fn is_gone(&self) -> bool {
        self.status() == Some(410)
    }

    /// The human-readable message Bitbucket returned for an HTTP error (its
    /// `error.message`), if this is an [`ApiError::Http`]. Lets callers tell
    /// otherwise-identical statuses apart by body — e.g. a 404 whose message
    /// mentions "no issue tracker" (feature disabled) vs. a missing repository.
    #[must_use]
    pub fn http_message(&self) -> Option<&str> {
        match self {
            ApiError::Http { message, .. } => Some(message),
            _ => None,
        }
    }
}

/// Errors from the git integration layer.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git command failed (exit {code}): {stderr}")]
    Command { code: i32, stderr: String },
    #[error("git executable not found: {0}")]
    NotFound(String),
    #[error("could not parse remote URL: {0}")]
    RemoteParse(String),
    #[error("{0}")]
    Other(String),
}

/// Errors from the config layer.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config io error: {0}")]
    Io(String),
    #[error("config parse error: {0}")]
    Parse(String),
}

/// Errors from the interactive prompt layer.
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Other(String),
}
