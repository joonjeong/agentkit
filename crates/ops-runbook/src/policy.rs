use crate::config::CallerPolicy;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Action {
    ServiceStart,
    ServiceStop,
    ServiceRestart,
    ServiceReload,
    ServiceStatus,
    Logs,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServiceStart => "service_start",
            Self::ServiceStop => "service_stop",
            Self::ServiceRestart => "service_restart",
            Self::ServiceReload => "service_reload",
            Self::ServiceStatus => "service_status",
            Self::Logs => "logs",
        }
    }
}

pub fn validate_target(target: &str) -> Result<()> {
    if target.is_empty()
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'@' | b'-'))
    {
        return Err(Error::InvalidTarget(target.to_owned()));
    }

    Ok(())
}

pub fn validate_caller(caller: &str) -> Result<()> {
    if caller.is_empty()
        || !caller
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'@' | b'-'))
    {
        return Err(Error::InvalidCaller(caller.to_owned()));
    }

    Ok(())
}

pub fn allowed_targets(policy: &CallerPolicy, action: Action) -> &[String] {
    match action {
        Action::ServiceStart
        | Action::ServiceStop
        | Action::ServiceRestart
        | Action::ServiceReload => &policy.service_control,
        Action::ServiceStatus | Action::Logs => &policy.service_read,
    }
}

pub fn is_allowed(policy: &CallerPolicy, action: Action, target: &str) -> bool {
    allowed_targets(policy, action)
        .iter()
        .any(|allowed| allowed == target)
}
