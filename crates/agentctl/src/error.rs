use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("agentctl must be executed via sudo")]
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

    #[error("invalid config option: {0}")]
    InvalidConfigOption(String),

    #[error("invalid notification option: {0}")]
    InvalidNotificationOption(String),

    #[error("github app session error: {0}")]
    GithubAppSession(String),

    #[error("agentd rejected request: {0}")]
    AgentdRejected(String),

    #[error("agentd protocol error: {reason}")]
    AgentdProtocol { reason: String },

    #[error("failed to connect to agentd socket {path}: {source}")]
    AgentdConnect {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("bootstrap must be run as root")]
    BootstrapRequiresRoot,

    #[error("invalid log line count: {0}")]
    InvalidLineCount(u32),

    #[error("backend {backend} does not support action {action}")]
    UnsupportedBackendAction {
        backend: &'static str,
        action: &'static str,
    },

    #[error("unsupported config version: {0}")]
    UnsupportedConfigVersion(u32),

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
}

pub type Result<T> = std::result::Result<T, Error>;
