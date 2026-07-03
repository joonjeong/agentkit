use std::ffi::OsString;
use std::path::Path;

use clap::{Args, Parser, Subcommand};

use crate::audit;
use crate::bootstrap::{self, BootstrapArgs};
use crate::config::{configured_policy_path, Config};
use crate::error::{Error, Result};
use crate::policy::{is_allowed, validate_target, Action};
use crate::runner;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_LOG_LINES: u32 = 200;

#[derive(Debug, Parser)]
#[command(name = "ops-runbook")]
#[command(version = VERSION)]
#[command(about = "Policy-driven restricted executor for homelab operations")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Bootstrap local installation, policy, sudoers, logrotate, and group access.
    Bootstrap(BootstrapArgs),
    /// Manage allowlisted systemd services.
    Service(ServiceCommand),
    /// Show allowlisted service logs from journald.
    Logs(LogsArgs),
    /// Validate or explain policy decisions.
    Policy(PolicyCommand),
    /// Print the ops-runbook version.
    Version,
}

#[derive(Debug, Args)]
struct ServiceCommand {
    #[command(subcommand)]
    command: ServiceSubcommand,
}

#[derive(Debug, Subcommand)]
enum ServiceSubcommand {
    /// Restart an allowlisted service.
    Restart(TargetArgs),
    /// Reload an allowlisted service.
    Reload(TargetArgs),
    /// Show status for an allowlisted service.
    Status(TargetArgs),
}

#[derive(Debug, Args)]
struct TargetArgs {
    service: String,
}

#[derive(Debug, Args)]
struct LogsArgs {
    service: String,
    #[arg(long, default_value_t = DEFAULT_LOG_LINES)]
    lines: u32,
}

#[derive(Debug, Args)]
struct PolicyCommand {
    #[command(subcommand)]
    command: PolicySubcommand,
}

#[derive(Debug, Subcommand)]
enum PolicySubcommand {
    /// Load and validate the policy file.
    Check,
    /// Explain the current caller's decision for an action and target.
    Explain { action: String, target: String },
}

pub fn run<I, T>(args: I) -> Result<i32>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if args.is_empty() {
        args.push(OsString::from("ops-runbook"));
    }

    let cli = Cli::parse_from(args);
    let policy_path = configured_policy_path();

    match cli.command {
        Command::Bootstrap(args) => bootstrap::run(args),
        Command::Service(command) => match command.command {
            ServiceSubcommand::Restart(args) => {
                execute(Action::ServiceRestart, &args.service, None, &policy_path)
            }
            ServiceSubcommand::Reload(args) => {
                execute(Action::ServiceReload, &args.service, None, &policy_path)
            }
            ServiceSubcommand::Status(args) => {
                execute(Action::ServiceStatus, &args.service, None, &policy_path)
            }
        },
        Command::Logs(args) => execute(Action::Logs, &args.service, Some(args.lines), &policy_path),
        Command::Policy(command) => match command.command {
            PolicySubcommand::Check => check_policy(&policy_path),
            PolicySubcommand::Explain { action, target } => {
                explain_policy(&policy_path, &action, &target)
            }
        },
        Command::Version => {
            println!("ops-runbook {VERSION}");
            Ok(0)
        }
    }
}

fn load_valid_config(policy_path: &Path) -> Result<Config> {
    let config = Config::load(policy_path)?;
    config.validate()?;
    Ok(config)
}

fn caller_from_sudo() -> Result<String> {
    let caller = std::env::var("SUDO_USER").map_err(|_| Error::MissingSudoUser)?;
    if caller.is_empty() {
        return Err(Error::MissingSudoUser);
    }
    if caller == "root" {
        return Err(Error::DirectRootExecution);
    }
    crate::policy::validate_caller(&caller)?;
    Ok(caller)
}

fn check_policy(policy_path: &Path) -> Result<i32> {
    let config = load_valid_config(policy_path)?;
    let mut callers = config
        .callers
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    callers.sort();
    println!("policy OK: {}", policy_path.display());
    println!("callers: {}", callers.join(", "));
    Ok(0)
}

fn explain_policy(policy_path: &Path, action: &str, target: &str) -> Result<i32> {
    let action = Action::parse(action)?;
    validate_target(target)?;
    let caller = caller_from_sudo()?;
    let config = load_valid_config(policy_path)?;

    println!("caller: {caller}");
    println!("action: {}", action.as_str());
    println!("target: {target}");

    let Some(caller_policy) = config.callers.get(&caller) else {
        println!("decision: deny");
        println!("reason: caller not listed in policy");
        return Ok(0);
    };

    if is_allowed(caller_policy, action, target) {
        println!("decision: allow");
        println!("source: callers.{caller}.{}", action.source_field());
    } else {
        println!("decision: deny");
        println!(
            "reason: target not listed in callers.{caller}.{}",
            action.source_field()
        );
    }

    Ok(0)
}

fn execute(
    action: Action,
    target: &str,
    requested_lines: Option<u32>,
    policy_path: &Path,
) -> Result<i32> {
    validate_target(target)?;
    let caller = caller_from_sudo()?;
    let config = load_valid_config(policy_path)?;

    let audit_path = audit::configured_audit_log_path();
    let Some(caller_policy) = config.callers.get(&caller) else {
        audit::write(
            &audit_path,
            &caller,
            action.as_str(),
            target,
            "deny",
            Some("caller_not_allowed"),
        )?;
        return Err(Error::CallerNotAllowed(caller));
    };

    if !is_allowed(caller_policy, action, target) {
        audit::write(
            &audit_path,
            &caller,
            action.as_str(),
            target,
            "deny",
            Some("target_not_allowed"),
        )?;
        return Err(Error::TargetNotAllowed(target.to_owned()));
    }

    let lines = requested_lines.unwrap_or(DEFAULT_LOG_LINES);
    if action == Action::Logs && (lines == 0 || lines > config.max_log_lines()) {
        audit::write(
            &audit_path,
            &caller,
            action.as_str(),
            target,
            "deny",
            Some("invalid_line_count"),
        )?;
        return Err(Error::InvalidLineCount(lines));
    }

    audit::write(&audit_path, &caller, action.as_str(), target, "allow", None)?;

    let exit_code = match action {
        Action::ServiceRestart => runner::systemctl("restart", target),
        Action::ServiceReload => runner::systemctl("reload", target),
        Action::ServiceStatus => runner::systemctl("status", target),
        Action::Logs => runner::journalctl(target, lines),
    }?;

    let outcome = format!("exit_code={exit_code}");
    audit::write(
        &audit_path,
        &caller,
        action.as_str(),
        target,
        "executed",
        Some(&outcome),
    )?;

    Ok(exit_code)
}
