use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::audit;
use crate::bootstrap::{self, BootstrapArgs};
use crate::config::{configured_policy_path, Backend, Config};
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
    /// Manage allowlisted services.
    Service(ServiceCommand),
    /// Show allowlisted service logs.
    Logs(LogsArgs),
    /// Validate or inspect policy.
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
    /// Start an allowlisted service.
    Start(TargetArgs),
    /// Stop an allowlisted service.
    Stop(TargetArgs),
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
    Check(PolicyArgs),
    /// Dump the validated policy and derived commands.
    Explain(PolicyArgs),
    /// Generate an example policy template.
    Template(PolicyTemplateArgs),
}

#[derive(Debug, Args)]
struct PolicyArgs {
    /// Policy file to read.
    #[arg(long, env = "OPS_RUNBOOK_POLICY_PATH")]
    policy_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct PolicyTemplateArgs {
    /// Service manager backend for the generated template.
    #[arg(long, value_enum, default_value_t = Backend::Systemd)]
    backend: Backend,

    /// Write the template to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
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
            ServiceSubcommand::Start(args) => {
                execute(Action::ServiceStart, &args.service, None, &policy_path)
            }
            ServiceSubcommand::Stop(args) => {
                execute(Action::ServiceStop, &args.service, None, &policy_path)
            }
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
            PolicySubcommand::Check(args) => {
                check_policy(args.policy_path.as_deref().unwrap_or(&policy_path))
            }
            PolicySubcommand::Explain(args) => {
                explain_policy(args.policy_path.as_deref().unwrap_or(&policy_path))
            }
            PolicySubcommand::Template(args) => template_policy(args),
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

fn explain_policy(policy_path: &Path) -> Result<i32> {
    let config = load_valid_config(policy_path)?;
    let mut callers = config.callers.iter().collect::<Vec<_>>();
    callers.sort_by_key(|(caller, _)| *caller);

    println!("policy: {}", policy_path.display());
    println!("version: {}", config.version);
    println!("backend: {}", config.backend());
    println!("max_log_lines: {}", config.max_log_lines());
    println!("callers:");

    for (caller, caller_policy) in callers {
        println!("  {caller}:");
        println!(
            "    service_control: {}",
            format_string_list(&caller_policy.service_control)
        );
        println!(
            "    service_read: {}",
            format_string_list(&caller_policy.service_read)
        );
        println!("    commands:");
        for service in &caller_policy.service_control {
            println!("      service start {service}");
            println!("      service stop {service}");
            println!("      service restart {service}");
            println!("      service reload {service}");
        }
        for service in &caller_policy.service_read {
            println!("      service status {service}");
            if config.backend() == crate::config::Backend::Systemd {
                println!("      logs {service}");
            }
        }
    }

    Ok(0)
}

fn format_string_list(items: &[String]) -> String {
    let quoted = items
        .iter()
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>();
    format!("[{}]", quoted.join(", "))
}

fn template_policy(args: PolicyTemplateArgs) -> Result<i32> {
    let template = bootstrap::sample_policy(args.backend);
    let Some(output) = args.output else {
        print!("{template}");
        return Ok(0);
    };

    if output.exists() && !args.force {
        return Err(Error::InvalidPolicyOption(format!(
            "output already exists: {}",
            output.display()
        )));
    }

    fs::write(&output, template).map_err(|source| Error::Io {
        path: output,
        source,
    })?;
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

    if action == Action::Logs && config.backend() == crate::config::Backend::Openrc {
        audit::write(
            &audit_path,
            &caller,
            action.as_str(),
            target,
            "deny",
            Some("unsupported_backend_action"),
        )?;
        return Err(Error::UnsupportedBackendAction {
            backend: config.backend().as_str(),
            action: action.as_str(),
        });
    }

    audit::write(&audit_path, &caller, action.as_str(), target, "allow", None)?;

    let exit_code = match action {
        Action::ServiceStart => runner::service(config.backend(), "start", target),
        Action::ServiceStop => runner::service(config.backend(), "stop", target),
        Action::ServiceRestart => runner::service(config.backend(), "restart", target),
        Action::ServiceReload => runner::service(config.backend(), "reload", target),
        Action::ServiceStatus => runner::service(config.backend(), "status", target),
        Action::Logs => runner::logs(config.backend(), target, lines),
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
