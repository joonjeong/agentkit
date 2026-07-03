use crate::config::CallerPolicy;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Action {
    ServiceRestart,
    ServiceReload,
    ServiceStatus,
    Logs,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServiceRestart => "service_restart",
            Self::ServiceReload => "service_reload",
            Self::ServiceStatus => "service_status",
            Self::Logs => "logs",
        }
    }

    pub fn source_field(self) -> &'static str {
        self.as_str()
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "service_restart" => Ok(Self::ServiceRestart),
            "service_reload" => Ok(Self::ServiceReload),
            "service_status" => Ok(Self::ServiceStatus),
            "logs" => Ok(Self::Logs),
            other => Err(Error::InvalidAction(other.to_owned())),
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
        Action::ServiceRestart => &policy.service_restart,
        Action::ServiceReload => &policy.service_reload,
        Action::ServiceStatus => &policy.service_status,
        Action::Logs => &policy.logs,
    }
}

pub fn is_allowed(policy: &CallerPolicy, action: Action, target: &str) -> bool {
    allowed_targets(policy, action)
        .iter()
        .any(|allowed| allowed == target)
}
