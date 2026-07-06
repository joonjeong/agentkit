use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Args;

use crate::audit;
use crate::config::{Backend, DEFAULT_CONFIG_PATH};
use crate::error::{Error, Result};
use crate::policy::validate_caller;

const DEFAULT_BINARY_PATH: &str = "/usr/local/sbin/agentctl";
const DEFAULT_GROUP: &str = "agent";
const DEFAULT_LOGROTATE_PATH: &str = "/etc/logrotate.d/agentctl";
const DEFAULT_SUDOERS_PATH: &str = "/etc/sudoers.d/agent";
const DEFAULT_SUDO_LOG_PATH: &str = "/var/log/agentctl/sudo.log";
const SUDOERS_TEMPLATE: &str =
    include_str!("../../../resources/agentctl/templates/sudoers.agent.template");

#[derive(Debug, Args)]
pub struct BootstrapArgs {
    /// Unix group allowed to run operational agentctl commands.
    #[arg(long, default_value = DEFAULT_GROUP)]
    group: String,

    /// Existing agent user to append to the agentctl group. Repeatable.
    #[arg(long = "user")]
    users: Vec<String>,

    /// Service manager backend written to the default config.
    #[arg(long, value_enum, default_value_t = Backend::Systemd)]
    backend: Backend,

    /// Source binary to install. Defaults to the currently running executable.
    #[arg(long)]
    source_binary: Option<PathBuf>,

    /// Installed agentctl path referenced from sudoers.
    #[arg(long, default_value = DEFAULT_BINARY_PATH)]
    binary_path: PathBuf,

    /// Sudoers file to create or replace.
    #[arg(long, default_value = DEFAULT_SUDOERS_PATH)]
    sudoers_path: PathBuf,

    /// Config file to create when missing.
    #[arg(long, default_value = DEFAULT_CONFIG_PATH)]
    config_path: PathBuf,

    /// Audit log path used by agentctl.
    #[arg(long, default_value = audit::DEFAULT_AUDIT_LOG_PATH)]
    audit_log_path: PathBuf,

    /// Sudo output log path embedded in sudoers.
    #[arg(long, default_value = DEFAULT_SUDO_LOG_PATH)]
    sudo_log_path: PathBuf,

    /// Logrotate config path to create or replace.
    #[arg(long, default_value = DEFAULT_LOGROTATE_PATH)]
    logrotate_path: PathBuf,

    /// Replace an existing config file with the sample config.
    #[arg(long)]
    force_config: bool,

    /// Do not install or replace the binary.
    #[arg(long)]
    skip_install: bool,

    /// Do not create the group or update user group membership.
    #[arg(long)]
    skip_system_accounts: bool,

    /// Do not validate the generated sudoers file with visudo.
    #[arg(long)]
    skip_visudo: bool,
}

pub fn run(args: BootstrapArgs) -> Result<i32> {
    ensure_root()?;
    validate_args(&args)?;

    if !args.skip_install {
        install_binary(&args)?;
        println!("binary ready: {}", args.binary_path.display());
    }

    if !args.skip_system_accounts {
        ensure_group(&args.group)?;
        for user in &args.users {
            validate_caller(user)?;
            run_command(
                "AGENTCTL_USERMOD_PATH",
                "/usr/sbin/usermod",
                &["-aG", &args.group, user],
            )?;
            println!("user added to group: {user} -> {}", args.group);
        }
    }

    ensure_parent_dir(&args.config_path, 0o755)?;
    ensure_parent_dir(&args.audit_log_path, 0o755)?;
    ensure_parent_dir(&args.sudo_log_path, 0o755)?;
    ensure_parent_dir(&args.sudoers_path, 0o755)?;
    ensure_parent_dir(&args.logrotate_path, 0o755)?;

    write_if_missing_or_forced(
        &args.config_path,
        sample_config(args.backend).as_bytes(),
        0o644,
        args.force_config,
    )?;
    println!("config ready: {}", args.config_path.display());

    let sudoers = sudoers_contents(&args);
    write_sudoers(&args.sudoers_path, sudoers.as_bytes(), args.skip_visudo)?;
    println!("sudoers ready: {}", args.sudoers_path.display());

    let logrotate = logrotate_contents(&args.audit_log_path, &args.sudo_log_path);
    write_atomic(&args.logrotate_path, logrotate.as_bytes(), 0o644)?;
    println!("logrotate ready: {}", args.logrotate_path.display());

    Ok(0)
}

fn ensure_root() -> Result<()> {
    if cfg!(debug_assertions) && std::env::var_os("AGENTCTL_TEST_OVERRIDES").is_some() {
        return Ok(());
    }

    // SAFETY: geteuid has no preconditions and does not dereference pointers.
    let euid = unsafe { libc::geteuid() };
    if euid == 0 {
        Ok(())
    } else {
        Err(Error::BootstrapRequiresRoot)
    }
}

fn validate_args(args: &BootstrapArgs) -> Result<()> {
    validate_caller(&args.group)?;
    for user in &args.users {
        validate_caller(user)?;
    }
    if let Some(source_binary) = &args.source_binary {
        validate_path_option("source-binary", source_binary, PathKind::Command)?;
    }
    validate_path_option("binary-path", &args.binary_path, PathKind::Command)?;
    validate_path_option("sudoers-path", &args.sudoers_path, PathKind::Config)?;
    validate_path_option("config-path", &args.config_path, PathKind::Config)?;
    validate_path_option("audit-log-path", &args.audit_log_path, PathKind::Config)?;
    validate_path_option("sudo-log-path", &args.sudo_log_path, PathKind::Config)?;
    validate_path_option("logrotate-path", &args.logrotate_path, PathKind::Config)?;
    Ok(())
}

enum PathKind {
    Command,
    Config,
}

fn validate_path_option(name: &str, path: &Path, kind: PathKind) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::InvalidBootstrapOption(format!(
            "--{name} must be absolute: {}",
            path.display()
        )));
    }

    let value = path.as_os_str().to_string_lossy();
    if value.contains('\n') || value.contains('\r') || value.contains('"') {
        return Err(Error::InvalidBootstrapOption(format!(
            "--{name} contains unsupported characters: {}",
            path.display()
        )));
    }

    if matches!(kind, PathKind::Command) && value.chars().any(char::is_whitespace) {
        return Err(Error::InvalidBootstrapOption(format!(
            "--{name} cannot contain whitespace: {}",
            path.display()
        )));
    }

    Ok(())
}

fn install_binary(args: &BootstrapArgs) -> Result<()> {
    ensure_parent_dir(&args.binary_path, 0o755)?;
    let source = match &args.source_binary {
        Some(path) => path.clone(),
        None => std::env::current_exe().map_err(|source| Error::Io {
            path: PathBuf::from("current executable"),
            source,
        })?,
    };

    if same_existing_file(&source, &args.binary_path) {
        set_mode(&args.binary_path, 0o755)?;
        return Ok(());
    }

    let bytes = fs::read(&source).map_err(|source_error| Error::Io {
        path: source.clone(),
        source: source_error,
    })?;
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

fn ensure_group(group: &str) -> Result<()> {
    let getent = command_path("AGENTCTL_GETENT_PATH", "/usr/bin/getent");
    let status = Command::new(&getent)
        .args(["group", group])
        .status()
        .map_err(Error::CommandStart)?;
    if status.success() {
        println!("group exists: {group}");
        return Ok(());
    }

    run_command(
        "AGENTCTL_GROUPADD_PATH",
        "/usr/sbin/groupadd",
        &["--system", group],
    )?;
    println!("group created: {group}");
    Ok(())
}

fn run_command(env_name: &str, default_path: &str, args: &[&str]) -> Result<()> {
    let program = command_path(env_name, default_path);
    let status = Command::new(&program)
        .args(args)
        .status()
        .map_err(Error::CommandStart)?;
    if status.success() {
        return Ok(());
    }

    Err(Error::CommandFailed {
        program: program.display().to_string(),
        args: args.join(" "),
    })
}

fn command_path(env_name: &str, default_path: &str) -> PathBuf {
    if cfg!(debug_assertions) && std::env::var_os("AGENTCTL_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os(env_name) {
            return PathBuf::from(path);
        }
    }

    PathBuf::from(default_path)
}

fn ensure_parent_dir(path: &Path, mode: u32) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(Error::InvalidBootstrapOption(format!(
            "path has no parent: {}",
            path.display()
        )));
    };

    fs::create_dir_all(parent).map_err(|source| Error::Io {
        path: parent.to_owned(),
        source,
    })?;
    set_mode(parent, mode)
}

fn write_if_missing_or_forced(path: &Path, contents: &[u8], mode: u32, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Ok(());
    }

    write_atomic(path, contents, mode)
}

fn write_sudoers(path: &Path, contents: &[u8], skip_visudo: bool) -> Result<()> {
    let temp_path = temporary_path(path);
    fs::write(&temp_path, contents).map_err(|source| Error::Io {
        path: temp_path.clone(),
        source,
    })?;
    set_mode(&temp_path, 0o440)?;

    if !skip_visudo {
        let program = command_path("AGENTCTL_VISUDO_PATH", "/usr/sbin/visudo");
        let status = Command::new(&program)
            .arg("-cf")
            .arg(&temp_path)
            .status()
            .map_err(Error::CommandStart)?;
        if !status.success() {
            return Err(Error::CommandFailed {
                program: program.display().to_string(),
                args: format!("-cf {}", temp_path.display()),
            });
        }
    }

    fs::rename(&temp_path, path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let temp_path = temporary_path(path);
    fs::write(&temp_path, contents).map_err(|source| Error::Io {
        path: temp_path.clone(),
        source,
    })?;
    set_mode(&temp_path, mode)?;
    fs::rename(&temp_path, path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path.file_name().unwrap_or_else(|| OsStr::new("agentctl"));
    path.with_file_name(format!(".{}.tmp", file_name.to_string_lossy()))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, permissions).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn sudoers_contents(args: &BootstrapArgs) -> String {
    let group = &args.group;
    let binary = args.binary_path.display();
    let sudo_log = args.sudo_log_path.display();

    SUDOERS_TEMPLATE
        .replace("{group}", group)
        .replace("{binary}", &binary.to_string())
        .replace("{sudo_log}", &sudo_log.to_string())
}

fn logrotate_contents(audit_log_path: &Path, sudo_log_path: &Path) -> String {
    format!(
        r#"{} {} {{
    weekly
    rotate 8
    compress
    missingok
    notifempty
    create 0640 root root
}}
"#,
        audit_log_path.display(),
        sudo_log_path.display()
    )
}

pub(crate) fn sample_config(backend: Backend) -> String {
    match backend {
        Backend::Systemd => {
            include_str!("../../../resources/agentctl/examples/config/systemd.example.toml")
        }
        Backend::Openrc => {
            include_str!("../../../resources/agentctl/examples/config/openrc.example.toml")
        }
    }
    .to_owned()
}
