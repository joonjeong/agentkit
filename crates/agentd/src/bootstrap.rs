use std::ffi::{CString, OsStr};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::{Args, ValueEnum};

use crate::config;

const DEFAULT_BINARY_PATH: &str = "/usr/local/sbin/agentd";
const DEFAULT_OPENRC_SERVICE_PATH: &str = "/etc/init.d/agentd";
const DEFAULT_SYSTEMD_SERVICE_PATH: &str = "/etc/systemd/system/agentd.service";
const DEFAULT_SOCKET_PATH: &str = crate::DEFAULT_SOCKET_PATH;
const OPENRC_SERVICE_TEMPLATE: &str = include_str!("../resources/templates/agentd.openrc.template");
const SYSTEMD_SERVICE_TEMPLATE: &str =
    include_str!("../resources/templates/agentd.service.template");

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Backend {
    Systemd,
    Openrc,
}

#[derive(Debug, Args)]
pub(crate) struct BootstrapArgs {
    /// Service manager backend written by bootstrap.
    #[arg(long, value_enum, default_value_t = Backend::Systemd)]
    backend: Backend,

    /// Source binary to install. Defaults to the currently running executable.
    #[arg(long)]
    source_binary: Option<PathBuf>,

    /// Installed agentd path.
    #[arg(long, default_value = DEFAULT_BINARY_PATH)]
    binary_path: PathBuf,

    /// Config file to create when missing.
    #[arg(long, default_value = crate::DEFAULT_CONFIG_PATH)]
    config_path: PathBuf,

    /// Unix domain socket path embedded in the generated service.
    #[arg(long, default_value = DEFAULT_SOCKET_PATH)]
    socket_path: PathBuf,

    /// Group to own the socket directory. When set, bootstrap sets the
    /// socket directory's group and adds group-read and group-execute
    /// permissions so `agentctl` clients in that group can connect.
    #[arg(long)]
    socket_group: Option<String>,

    /// Service file to create when missing.
    #[arg(long)]
    service_path: Option<PathBuf>,

    /// Replace an existing config file with the example config.
    #[arg(long)]
    force_config: bool,

    /// Replace an existing service file.
    #[arg(long)]
    force_service: bool,

    /// Do not install or replace the binary.
    #[arg(long)]
    skip_install: bool,
}

pub(crate) fn run(args: BootstrapArgs) -> Result<()> {
    ensure_root()?;
    validate_args(&args)?;

    if !args.skip_install {
        install_binary(&args)?;
        println!("binary ready: {}", args.binary_path.display());
    }

    ensure_parent_dir(&args.config_path, 0o755)?;
    write_if_missing_or_forced(
        &args.config_path,
        config::example().as_bytes(),
        0o644,
        args.force_config,
    )?;
    println!("config ready: {}", args.config_path.display());

    let service_path = args
        .service_path
        .clone()
        .unwrap_or_else(|| default_service_path(args.backend));
    ensure_parent_dir(&service_path, 0o755)?;
    let service = service_contents(
        args.backend,
        &args.binary_path,
        &args.config_path,
        &args.socket_path,
    );
    write_if_missing_or_forced(
        &service_path,
        service.as_bytes(),
        service_mode(args.backend),
        args.force_service,
    )?;
    println!("service ready: {}", service_path.display());

    if let Some(group) = &args.socket_group {
        let socket_dir = args.socket_path.parent().unwrap_or(&args.socket_path);
        set_socket_dir_group(socket_dir, group)?;
        println!("socket group set: {group} on {}", socket_dir.display());
    }

    Ok(())
}

fn ensure_root() -> Result<()> {
    if cfg!(debug_assertions) && std::env::var_os("AGENTD_TEST_OVERRIDES").is_some() {
        return Ok(());
    }

    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn geteuid() -> u32;
        }

        // SAFETY: geteuid has no preconditions and does not dereference pointers.
        if unsafe { geteuid() } == 0 {
            return Ok(());
        }
    }

    Err(anyhow!("bootstrap must be run as root"))
}

fn validate_args(args: &BootstrapArgs) -> Result<()> {
    if let Some(source_binary) = &args.source_binary {
        validate_path_option("source-binary", source_binary, PathKind::Command)?;
    }
    validate_path_option("binary-path", &args.binary_path, PathKind::Command)?;
    validate_path_option("config-path", &args.config_path, PathKind::Config)?;
    validate_path_option("socket-path", &args.socket_path, PathKind::Config)?;
    if let Some(service_path) = &args.service_path {
        validate_path_option("service-path", service_path, PathKind::Config)?;
    }
    Ok(())
}

enum PathKind {
    Command,
    Config,
}

fn validate_path_option(name: &str, path: &Path, kind: PathKind) -> Result<()> {
    if !path.is_absolute() {
        return Err(anyhow!("--{name} must be absolute: {}", path.display()));
    }

    let value = path.as_os_str().to_string_lossy();
    if value.contains('\n') || value.contains('\r') || value.contains('"') {
        return Err(anyhow!(
            "--{name} contains unsupported characters: {}",
            path.display()
        ));
    }

    if matches!(kind, PathKind::Command) && value.chars().any(char::is_whitespace) {
        return Err(anyhow!(
            "--{name} cannot contain whitespace: {}",
            path.display()
        ));
    }

    Ok(())
}

fn install_binary(args: &BootstrapArgs) -> Result<()> {
    ensure_parent_dir(&args.binary_path, 0o755)?;
    let source = match &args.source_binary {
        Some(path) => path.clone(),
        None => std::env::current_exe().context("failed to resolve current executable")?,
    };

    if same_existing_file(&source, &args.binary_path) {
        set_mode(&args.binary_path, 0o755)?;
        return Ok(());
    }

    let bytes = fs::read(&source)
        .with_context(|| format!("failed to read source binary {}", source.display()))?;
    write_atomic(&args.binary_path, &bytes, 0o755)
}

fn same_existing_file(left: &Path, right: &Path) -> bool {
    let Ok(left) = fs::canonicalize(left) else {
        return false;
    };
    let Ok(right) = fs::canonicalize(right) else {
        return false;
    };
    left == right
}

fn ensure_parent_dir(path: &Path, mode: u32) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("path has no parent: {}", path.display()));
    };
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    set_mode(parent, mode)
}

fn write_if_missing_or_forced(path: &Path, contents: &[u8], mode: u32, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Ok(());
    }

    write_atomic(path, contents, mode)
}

fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let temp_path = temporary_path(path);
    fs::write(&temp_path, contents)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    set_mode(&temp_path, mode)?;
    fs::rename(&temp_path, path).with_context(|| format!("failed to rename {}", path.display()))
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path.file_name().unwrap_or_else(|| OsStr::new("agentd"));
    path.with_file_name(format!(".{}.tmp", file_name.to_string_lossy()))
}

fn default_service_path(backend: Backend) -> PathBuf {
    match backend {
        Backend::Systemd => PathBuf::from(DEFAULT_SYSTEMD_SERVICE_PATH),
        Backend::Openrc => PathBuf::from(DEFAULT_OPENRC_SERVICE_PATH),
    }
}

fn service_mode(backend: Backend) -> u32 {
    match backend {
        Backend::Systemd => 0o644,
        Backend::Openrc => 0o755,
    }
}

fn service_contents(
    backend: Backend,
    binary_path: &Path,
    config_path: &Path,
    socket_path: &Path,
) -> String {
    let template = match backend {
        Backend::Systemd => SYSTEMD_SERVICE_TEMPLATE,
        Backend::Openrc => OPENRC_SERVICE_TEMPLATE,
    };

    template
        .replace("{binary}", &binary_path.display().to_string())
        .replace("{config}", &config_path.display().to_string())
        .replace("{socket}", &socket_path.display().to_string())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("failed to set permissions on {}", path.display()))
}

#[cfg(unix)]
fn set_socket_dir_group(path: &Path, group: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    // Resolve group name to GID.
    let name = CString::new(group).context("group name contains a NUL byte")?;
    let mut grp = unsafe { std::mem::zeroed::<libc::group>() };
    let mut result = std::ptr::null_mut::<libc::group>();
    let mut buffer = vec![0_u8; 16 * 1024];
    let status = unsafe {
        libc::getgrnam_r(
            name.as_ptr(),
            std::ptr::addr_of_mut!(grp),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            std::ptr::addr_of_mut!(result),
        )
    };
    if status != 0 {
        return Err(std::io::Error::from_raw_os_error(status))
            .context(format!("failed to resolve group {group:?}"));
    }
    if result.is_null() {
        return Err(anyhow!("group does not exist: {group:?}"));
    }
    let gid = grp.gr_gid;

    // Set group ownership on the directory.
    unsafe {
        if libc::chown(
            path.as_os_str().as_encoded_bytes().as_ptr().cast(),
            !0, // -1: don't change owner
            gid,
        ) != 0
        {
            return Err(std::io::Error::last_os_error())
                .context(format!("failed to chgrp {} to {group:?}", path.display()));
        }
    }

    // Ensure group read+execute on the directory so members can traverse to the socket.
    let mut perms = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .permissions();
    let mode = perms.mode();
    let new_mode = mode | 0o050; // group read + execute
    if new_mode != mode {
        fs::set_permissions(path, fs::Permissions::from_mode(new_mode))
            .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    }

    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn set_socket_dir_group(_path: &Path, _group: &str) -> Result<()> {
    Ok(())
}
