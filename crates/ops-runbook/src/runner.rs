use std::path::PathBuf;
use std::process::Command;

use crate::error::{Error, Result};

fn command_path(env_name: &str, default_path: &str) -> PathBuf {
    if cfg!(debug_assertions) && std::env::var_os("OPS_RUNBOOK_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os(env_name) {
            return PathBuf::from(path);
        }
    }

    PathBuf::from(default_path)
}

pub fn systemctl(action: &str, service: &str) -> Result<i32> {
    let unit = format!("{service}.service");
    let status = Command::new(command_path("OPS_RUNBOOK_SYSTEMCTL_PATH", "/bin/systemctl"))
        .arg("--no-pager")
        .arg(action)
        .arg(unit)
        .status()
        .map_err(Error::CommandStart)?;

    Ok(status.code().unwrap_or(1))
}

pub fn journalctl(service: &str, lines: u32) -> Result<i32> {
    let unit = format!("{service}.service");
    let status = Command::new(command_path(
        "OPS_RUNBOOK_JOURNALCTL_PATH",
        "/bin/journalctl",
    ))
    .arg("--no-pager")
    .arg("-u")
    .arg(unit)
    .arg("-n")
    .arg(lines.to_string())
    .status()
    .map_err(Error::CommandStart)?;

    Ok(status.code().unwrap_or(1))
}
