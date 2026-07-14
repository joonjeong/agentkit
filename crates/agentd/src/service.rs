use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
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

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct ServiceConfigFile {
    pub(crate) backend: Option<Backend>,
    pub(crate) max_log_lines: Option<u32>,
    #[serde(default)]
    #[serde(flatten)]
    pub(crate) services: BTreeMap<String, ServicePolicy>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServicePolicy {
    #[serde(default)]
    pub(crate) viewer: Vec<String>,
    #[serde(default)]
    pub(crate) operator: Vec<String>,
}

pub(crate) struct ServiceOutput {
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn validate_service_config(service: &ServiceConfigFile, path: &Path) -> Result<()> {
    if service.services.is_empty() {
        return Err(anyhow!("missing service entries in {}", path.display()));
    }
    for (target, policy) in &service.services {
        validate_target(target)
            .with_context(|| format!("invalid service entry {target:?} in {}", path.display()))?;
        if policy.viewer.is_empty() && policy.operator.is_empty() {
            return Err(anyhow!(
                "service entry {target:?} must include at least one viewer or operator in {}",
                path.display()
            ));
        }
        for identity in policy.viewer.iter().chain(&policy.operator) {
            validate_identity(identity, target)?;
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
    let policy = config.services.get(target).ok_or_else(|| {
        anyhow!(
            "service {target:?} is not configured for {}",
            peer_description(peer)
        )
    })?;
    let allowed = match action {
        "start" | "stop" | "restart" | "reload" => matches_peer(&policy.operator, peer)?,
        "status" | "logs" => matches_peer(&policy.viewer, peer)?,
        _ => return Err(anyhow!("unsupported service action: {action}")),
    };
    if allowed {
        Ok(())
    } else {
        Err(anyhow!(
            "service {target:?} action {action:?} is not allowed for {}",
            peer_description(peer)
        ))
    }
}

fn matches_peer(identities: &[String], peer: &PeerIdentity) -> Result<bool> {
    if identities.is_empty() {
        return Ok(false);
    }
    let mut peer_groups = None;
    for identity in identities {
        match parse_identity(identity)? {
            Identity::User(user) => {
                if resolve_user(user).is_ok_and(|uid| uid == peer.uid) {
                    return Ok(true);
                }
            }
            Identity::Group(group) => {
                if peer_groups.is_none() {
                    peer_groups = Some(peer_groups_for_match(peer));
                }
                if resolve_group(group).is_ok_and(|gid| {
                    peer_groups
                        .as_ref()
                        .is_some_and(|groups| groups.contains(&gid))
                }) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
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

enum Identity<'a> {
    User(&'a str),
    Group(&'a str),
}

fn parse_identity(identity: &str) -> Result<Identity<'_>> {
    if let Some(name) = identity.strip_prefix("u:") {
        Ok(Identity::User(name))
    } else if let Some(name) = identity.strip_prefix("g:") {
        Ok(Identity::Group(name))
    } else {
        Err(anyhow!(
            "service identity must use u: or g: prefix: {identity:?}"
        ))
    }
}

fn validate_identity(identity: &str, target: &str) -> Result<()> {
    match parse_identity(identity)? {
        Identity::User(name) => validate_identity_name(name, "user", target),
        Identity::Group(name) => validate_identity_name(name, "group", target),
    }
}

fn validate_identity_name(name: &str, kind: &str, target: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!(
            "{kind} name must not be empty in service {target:?}"
        ));
    }
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "{kind} name may contain only ASCII letters, numbers, '_', '-', or '.': {name:?}"
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

fn resolve_user(name: &str) -> Result<u32> {
    let name = CString::new(name).context("user name contains a NUL byte")?;
    let mut pwd = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut::<libc::passwd>();
    let mut buffer = vec![0_u8; passwd_buffer_size()];
    let status = unsafe {
        libc::getpwnam_r(
            name.as_ptr(),
            std::ptr::addr_of_mut!(pwd),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            std::ptr::addr_of_mut!(result),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status)).context("getpwnam_r failed");
    }
    if result.is_null() {
        return Err(anyhow!("user does not exist"));
    }
    Ok(pwd.pw_uid)
}

fn resolve_group(name: &str) -> Result<u32> {
    let name = CString::new(name).context("group name contains a NUL byte")?;
    let mut group = unsafe { std::mem::zeroed::<libc::group>() };
    let mut result = std::ptr::null_mut::<libc::group>();
    let mut buffer = vec![0_u8; group_buffer_size()];
    let status = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            std::ptr::addr_of_mut!(group),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            std::ptr::addr_of_mut!(result),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status)).context("getgrnam_r failed");
    }
    if result.is_null() {
        return Err(anyhow!("group does not exist"));
    }
    Ok(group.gr_gid)
}

fn peer_groups_for_match(peer: &PeerIdentity) -> BTreeSet<u32> {
    let mut groups = BTreeSet::from([peer.gid]);
    if let Some((user, primary_gid)) = user_info_for_uid(peer.uid).ok().flatten() {
        if let Ok(supplementary_groups) = supplementary_groups(&user, primary_gid) {
            for gid in supplementary_groups {
                groups.insert(gid);
            }
        }
    }
    groups
}

fn peer_description(peer: &PeerIdentity) -> String {
    let user = username_for_uid(peer.uid)
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_owned());
    let group = groupname_for_gid(peer.gid)
        .ok()
        .flatten()
        .unwrap_or_else(|| "unknown".to_owned());
    format!(
        "user {user:?} uid {} group {group:?} gid {}",
        peer.uid, peer.gid
    )
}

fn username_for_uid(uid: u32) -> Result<Option<String>> {
    Ok(user_info_for_uid(uid)?.map(|(user, _)| user))
}

fn user_info_for_uid(uid: u32) -> Result<Option<(String, u32)>> {
    let mut pwd = unsafe { std::mem::zeroed::<libc::passwd>() };
    let mut result = std::ptr::null_mut::<libc::passwd>();
    let mut buffer = vec![0_u8; passwd_buffer_size()];
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            std::ptr::addr_of_mut!(pwd),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            std::ptr::addr_of_mut!(result),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status)).context("getpwuid_r failed");
    }
    if result.is_null() {
        return Ok(None);
    }
    let name = unsafe { CStr::from_ptr(pwd.pw_name) }
        .to_str()
        .context("user name is not valid UTF-8")?
        .to_owned();
    Ok(Some((name, pwd.pw_gid)))
}

fn groupname_for_gid(gid: u32) -> Result<Option<String>> {
    let mut group = unsafe { std::mem::zeroed::<libc::group>() };
    let mut result = std::ptr::null_mut::<libc::group>();
    let mut buffer = vec![0_u8; group_buffer_size()];
    let status = unsafe {
        libc::getgrgid_r(
            gid,
            std::ptr::addr_of_mut!(group),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            std::ptr::addr_of_mut!(result),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status)).context("getgrgid_r failed");
    }
    if result.is_null() {
        return Ok(None);
    }
    let name = unsafe { CStr::from_ptr(group.gr_name) }
        .to_str()
        .context("group name is not valid UTF-8")?
        .to_owned();
    Ok(Some(name))
}

#[cfg(target_os = "macos")]
fn supplementary_groups(user: &str, primary_gid: u32) -> Result<Vec<u32>> {
    let user = CString::new(user).context("user name contains a NUL byte")?;
    let primary_gid: libc::c_int = primary_gid
        .try_into()
        .context("primary gid does not fit platform c_int")?;
    let mut count: libc::c_int = 0;
    unsafe {
        libc::getgrouplist(
            user.as_ptr(),
            primary_gid,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(count),
        );
    }
    if count <= 0 {
        return Ok(Vec::new());
    }
    let mut groups = vec![0 as libc::c_int; count as usize];
    let status = unsafe {
        libc::getgrouplist(
            user.as_ptr(),
            primary_gid,
            groups.as_mut_ptr(),
            std::ptr::addr_of_mut!(count),
        )
    };
    if status < 0 {
        return Err(anyhow!("getgrouplist failed"));
    }
    groups.truncate(count as usize);
    Ok(groups.into_iter().map(|gid| gid as u32).collect())
}

#[cfg(target_os = "linux")]
fn supplementary_groups(user: &str, primary_gid: u32) -> Result<Vec<u32>> {
    let user = CString::new(user).context("user name contains a NUL byte")?;
    let mut count: libc::c_int = 0;
    unsafe {
        libc::getgrouplist(
            user.as_ptr(),
            primary_gid,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(count),
        );
    }
    if count <= 0 {
        return Ok(Vec::new());
    }
    let mut groups = vec![0 as libc::gid_t; count as usize];
    let status = unsafe {
        libc::getgrouplist(
            user.as_ptr(),
            primary_gid,
            groups.as_mut_ptr(),
            std::ptr::addr_of_mut!(count),
        )
    };
    if status < 0 {
        return Err(anyhow!("getgrouplist failed"));
    }
    groups.truncate(count as usize);
    Ok(groups)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn supplementary_groups(user: &str, primary_gid: u32) -> Result<Vec<u32>> {
    let user = CString::new(user).context("user name contains a NUL byte")?;
    let primary_gid: libc::gid_t = primary_gid
        .try_into()
        .context("primary gid does not fit platform gid_t")?;
    let mut count: libc::c_int = 0;
    unsafe {
        libc::getgrouplist(
            user.as_ptr(),
            primary_gid,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(count),
        );
    }
    if count <= 0 {
        return Ok(Vec::new());
    }
    let mut groups = vec![0 as libc::gid_t; count as usize];
    let status = unsafe {
        libc::getgrouplist(
            user.as_ptr(),
            primary_gid,
            groups.as_mut_ptr(),
            std::ptr::addr_of_mut!(count),
        )
    };
    if status < 0 {
        return Err(anyhow!("getgrouplist failed"));
    }
    groups.truncate(count as usize);
    groups
        .into_iter()
        .map(|gid| gid.try_into().context("supplementary gid does not fit u32"))
        .collect()
}

fn passwd_buffer_size() -> usize {
    sysconf_buffer_size(libc::_SC_GETPW_R_SIZE_MAX)
}

fn group_buffer_size() -> usize {
    sysconf_buffer_size(libc::_SC_GETGR_R_SIZE_MAX)
}

fn sysconf_buffer_size(name: libc::c_int) -> usize {
    let size = unsafe { libc::sysconf(name) };
    if size > 0 {
        size as usize
    } else {
        16 * 1024
    }
}
