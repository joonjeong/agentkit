use std::path::PathBuf;
use std::process::Command;

use crate::config::Backend;
use crate::error::{Error, Result};

fn command_path(env_name: &str, default_path: &str) -> PathBuf {
    if cfg!(debug_assertions) && std::env::var_os("AGENTCTL_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os(env_name) {
            return PathBuf::from(path);
        }
    }

    PathBuf::from(default_path)
}

pub fn service(backend: Backend, action: &str, service: &str) -> Result<i32> {
    match backend {
        Backend::Systemd => systemctl(action, service),
        Backend::Openrc => rc_service(action, service),
    }
}

pub fn logs(backend: Backend, service: &str, lines: u32) -> Result<i32> {
    match backend {
        Backend::Systemd => journalctl(service, lines),
        Backend::Openrc => Err(Error::UnsupportedBackendAction {
            backend: backend.as_str(),
            action: "logs",
        }),
    }
}

fn systemctl(action: &str, service: &str) -> Result<i32> {
    let unit = format!("{service}.service");
    let status = Command::new(command_path("AGENTCTL_SYSTEMCTL_PATH", "/bin/systemctl"))
        .arg("--no-pager")
        .arg(action)
        .arg(unit)
        .status()
        .map_err(Error::CommandStart)?;

    Ok(status.code().unwrap_or(1))
}

fn journalctl(service: &str, lines: u32) -> Result<i32> {
    let unit = format!("{service}.service");
    let status = Command::new(command_path("AGENTCTL_JOURNALCTL_PATH", "/bin/journalctl"))
        .arg("--no-pager")
        .arg("-u")
        .arg(unit)
        .arg("-n")
        .arg(lines.to_string())
        .status()
        .map_err(Error::CommandStart)?;

    Ok(status.code().unwrap_or(1))
}

fn rc_service(action: &str, service: &str) -> Result<i32> {
    let status = Command::new(command_path("AGENTCTL_RC_SERVICE_PATH", "/sbin/rc-service"))
        .arg(service)
        .arg(action)
        .status()
        .map_err(Error::CommandStart)?;

    Ok(status.code().unwrap_or(1))
}
