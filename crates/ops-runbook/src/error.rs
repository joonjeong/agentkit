use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("ops-runbook must be executed via sudo")]
    MissingSudoUser,

    #[error("direct root execution is not allowed")]
    DirectRootExecution,

    #[error("caller not allowed: {0}")]
    CallerNotAllowed(String),

    #[error("target not allowed: {0}")]
    TargetNotAllowed(String),

    #[error("invalid target: {0}")]
    InvalidTarget(String),

    #[error("invalid caller: {0}")]
    InvalidCaller(String),

    #[error("invalid bootstrap option: {0}")]
    InvalidBootstrapOption(String),

    #[error("invalid policy option: {0}")]
    InvalidPolicyOption(String),

    #[error("invalid notification option: {0}")]
    InvalidNotificationOption(String),

    #[error("notification channel not allowed: {0}")]
    NotificationChannelNotAllowed(String),

    #[error("notification channel not found: {0}")]
    NotificationChannelNotFound(String),

    #[error("bootstrap must be run as root")]
    BootstrapRequiresRoot,

    #[error("invalid log line count: {0}")]
    InvalidLineCount(u32),

    #[error("backend {backend} does not support action {action}")]
    UnsupportedBackendAction {
        backend: &'static str,
        action: &'static str,
    },

    #[error("unsupported policy version: {0}")]
    UnsupportedPolicyVersion(u32),

    #[error("config error in {path}: {source}")]
    Config {
        path: PathBuf,
        source: toml::de::Error,
    },

    #[error("io error for {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("command failed to start: {0}")]
    CommandStart(std::io::Error),

    #[error("command failed: {program} {args}")]
    CommandFailed { program: String, args: String },

    #[error("notification send failed for {destination}: {reason}")]
    NotificationSend { destination: String, reason: String },
}

pub type Result<T> = std::result::Result<T, Error>;
