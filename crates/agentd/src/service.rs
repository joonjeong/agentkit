use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::ValueEnum;
use serde::Deserialize;

use crate::peer::PeerIdentity;

const DEFAULT_MAX_LOG_LINES: u32 = 1000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Backend {
    Systemd,
    Openrc,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceConfigFile {
    pub(crate) backend: Option<Backend>,
    pub(crate) max_log_lines: Option<u32>,
    #[serde(default)]
    pub(crate) callers: BTreeMap<String, ServiceCallerPolicy>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceCallerPolicy {
    #[serde(default)]
    pub(crate) uids: Vec<u32>,
    #[serde(default)]
    pub(crate) gids: Vec<u32>,
    #[serde(default)]
    pub(crate) service_control: Vec<String>,
    #[serde(default)]
    pub(crate) service_read: Vec<String>,
}

pub(crate) struct ServiceOutput {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn validate_service_config(service: &ServiceConfigFile, path: &Path) -> Result<()> {
    if service.callers.is_empty() {
        return Err(anyhow!("missing service callers in {}", path.display()));
    }
    for (name, policy) in &service.callers {
        validate_profile_name(name, path)?;
        if policy.uids.is_empty() && policy.gids.is_empty() {
            return Err(anyhow!(
                "service caller {name:?} must include at least one uid or gid in {}",
                path.display()
            ));
        }
        for target in policy.service_control.iter().chain(&policy.service_read) {
            validate_target(target)
                .with_context(|| format!("invalid service target {target:?} in {name:?}"))?;
        }
    }
    Ok(())
}

pub(crate) fn run(
    config: &ServiceConfigFile,
    peer: &PeerIdentity,
    action: &str,
    target: &str,
    lines: Option<u32>,
) -> Result<ServiceOutput> {
    validate_target(target)?;
    authorize(config, peer, action, target)?;
    let backend = config.backend.unwrap_or(Backend::Systemd);
    match action {
        "start" | "stop" | "restart" | "reload" | "status" => service(backend, action, target),
        "logs" => {
            if backend == Backend::Openrc {
                return Err(anyhow!("backend openrc does not support action logs"));
            }
            let lines = lines.unwrap_or(200);
            let max_lines = config.max_log_lines.unwrap_or(DEFAULT_MAX_LOG_LINES);
            if lines == 0 || lines > max_lines {
                return Err(anyhow!("invalid log line count: {lines}"));
            }
            logs(backend, target, lines)
        }
        _ => Err(anyhow!("unsupported service action: {action}")),
    }
}

fn authorize(
    config: &ServiceConfigFile,
    peer: &PeerIdentity,
    action: &str,
    target: &str,
) -> Result<()> {
    let mut matched_identity = false;
    for policy in config.callers.values() {
        if !matches_peer(policy, peer) {
            continue;
        }
        matched_identity = true;
        let allowed = match action {
            "start" | "stop" | "restart" | "reload" => &policy.service_control,
            "status" | "logs" => &policy.service_read,
            _ => return Err(anyhow!("unsupported service action: {action}")),
        };
        if allowed.iter().any(|allowed| allowed == target) {
            return Ok(());
        }
    }
    if matched_identity {
        Err(anyhow!(
            "service {target:?} is not allowed for uid {} gid {}",
            peer.uid,
            peer.gid
        ))
    } else {
        Err(anyhow!(
            "no service caller policy matched uid {} gid {}",
            peer.uid,
            peer.gid
        ))
    }
}

fn matches_peer(policy: &ServiceCallerPolicy, peer: &PeerIdentity) -> bool {
    policy.uids.contains(&peer.uid) || policy.gids.contains(&peer.gid)
}

fn service(backend: Backend, action: &str, target: &str) -> Result<ServiceOutput> {
    match backend {
        Backend::Systemd => run_command(
            command_path("AGENTD_SYSTEMCTL_PATH", "/bin/systemctl"),
            &["--no-pager", action, &format!("{target}.service")],
        ),
        Backend::Openrc => run_command(
            command_path("AGENTD_RC_SERVICE_PATH", "/sbin/rc-service"),
            &[target, action],
        ),
    }
}

fn logs(backend: Backend, target: &str, lines: u32) -> Result<ServiceOutput> {
    match backend {
        Backend::Systemd => run_command(
            command_path("AGENTD_JOURNALCTL_PATH", "/bin/journalctl"),
            &[
                "--no-pager",
                "-u",
                &format!("{target}.service"),
                "-n",
                &lines.to_string(),
            ],
        ),
        Backend::Openrc => Err(anyhow!("backend openrc does not support action logs")),
    }
}

fn command_path(env_name: &str, default_path: &str) -> PathBuf {
    if cfg!(debug_assertions) && std::env::var_os("AGENTD_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os(env_name) {
            return PathBuf::from(path);
        }
    }
    PathBuf::from(default_path)
}

fn run_command(program: PathBuf, args: &[&str]) -> Result<ServiceOutput> {
    let output = Command::new(&program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {}", program.display()))?;
    Ok(ServiceOutput {
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn validate_profile_name(name: &str, path: &Path) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!(
            "service caller name must not be empty in {}",
            path.display()
        ));
    }
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "service caller name may contain only ASCII letters, numbers, '_', '-', or '.': {name:?}"
        ))
    }
}

fn validate_target(target: &str) -> Result<()> {
    if target.is_empty()
        || !target
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'@' | b'-'))
    {
        return Err(anyhow!("invalid service target: {target}"));
    }
    Ok(())
}
