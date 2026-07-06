use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};

use crate::audit;
use crate::bootstrap::{self, BootstrapArgs};
use crate::config::{configured_config_path, Backend, Config};
use crate::error::{Error, Result};
use crate::notification::{self, Notification, Severity};
use crate::policy::{is_allowed, validate_target, Action};
use crate::runner;

const VERSION: &str = match option_env!("AGENTKIT_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
const DEFAULT_LOG_LINES: u32 = 200;
const DEFAULT_AGENTD_SOCKET_PATH: &str = "/run/agentd/agentd.sock";

#[derive(Debug, Parser)]
#[command(name = "agentctl")]
#[command(version = VERSION)]
#[command(about = "Config-driven restricted executor for homelab operations")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Bootstrap local installation, config, sudoers, logrotate, and group access.
    Bootstrap(BootstrapArgs),
    /// Manage allowlisted services.
    Service(ServiceCommand),
    /// Show allowlisted service logs.
    Logs(LogsArgs),
    /// Send an allowlisted notification.
    Notify(NotifyArgs),
    /// Validate or inspect config.
    Config(ConfigCommand),
    /// Print the agentctl version.
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
struct NotifyArgs {
    #[command(subcommand)]
    provider: NotifyProviderCommand,
}

#[derive(Debug, Subcommand)]
enum NotifyProviderCommand {
    /// Send a Telegram notification through an agentd Telegram profile.
    Telegram(TelegramNotifyArgs),
    /// Send a Discord notification through an agentd Discord profile.
    Discord(DiscordNotifyArgs),
}

#[derive(Debug, Args)]
struct TelegramNotifyArgs {
    /// Telegram profile name from agentd config.
    profile: String,

    /// Telegram chat id to send to.
    #[arg(long)]
    chat_id: String,

    /// agentd Unix domain socket path.
    #[arg(
        long,
        env = "AGENTCTL_AGENTD_SOCKET",
        default_value = DEFAULT_AGENTD_SOCKET_PATH
    )]
    agentd_socket: PathBuf,

    /// Notification severity.
    #[arg(long, value_enum, default_value_t = Severity::Info)]
    severity: Severity,

    /// Optional short notification title.
    #[arg(long)]
    title: Option<String>,

    /// Notification message body.
    #[arg(long)]
    message: String,
}

#[derive(Debug, Args)]
struct DiscordNotifyArgs {
    /// Discord profile name from agentd config.
    profile: String,

    /// agentd Unix domain socket path.
    #[arg(
        long,
        env = "AGENTCTL_AGENTD_SOCKET",
        default_value = DEFAULT_AGENTD_SOCKET_PATH
    )]
    agentd_socket: PathBuf,

    /// Notification severity.
    #[arg(long, value_enum, default_value_t = Severity::Info)]
    severity: Severity,

    /// Optional short notification title.
    #[arg(long)]
    title: Option<String>,

    /// Notification message body.
    #[arg(long)]
    message: String,
}

#[derive(Debug, Args)]
struct ConfigCommand {
    #[command(subcommand)]
    command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    /// Load and validate the config file.
    Check(ConfigArgs),
    /// Dump the validated config and derived commands.
    Explain(ConfigArgs),
    /// Generate an example config template.
    Template(ConfigTemplateArgs),
}

#[derive(Debug, Args)]
struct ConfigArgs {
    /// Config file to read.
    #[arg(long, env = "AGENTCTL_CONFIG_PATH")]
    config_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ConfigTemplateArgs {
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
        args.push(OsString::from("agentctl"));
    }

    let cli = Cli::parse_from(args);
    let config_path = configured_config_path();

    match cli.command {
        Command::Bootstrap(args) => bootstrap::run(args),
        Command::Service(command) => match command.command {
            ServiceSubcommand::Start(args) => {
                execute(Action::ServiceStart, &args.service, None, &config_path)
            }
            ServiceSubcommand::Stop(args) => {
                execute(Action::ServiceStop, &args.service, None, &config_path)
            }
            ServiceSubcommand::Restart(args) => {
                execute(Action::ServiceRestart, &args.service, None, &config_path)
            }
            ServiceSubcommand::Reload(args) => {
                execute(Action::ServiceReload, &args.service, None, &config_path)
            }
            ServiceSubcommand::Status(args) => {
                execute(Action::ServiceStatus, &args.service, None, &config_path)
            }
        },
        Command::Logs(args) => execute(Action::Logs, &args.service, Some(args.lines), &config_path),
        Command::Notify(args) => notify(args),
        Command::Config(command) => match command.command {
            ConfigSubcommand::Check(args) => {
                check_config(args.config_path.as_deref().unwrap_or(&config_path))
            }
            ConfigSubcommand::Explain(args) => {
                explain_config(args.config_path.as_deref().unwrap_or(&config_path))
            }
            ConfigSubcommand::Template(args) => template_config(args),
        },
        Command::Version => {
            println!("agentctl {VERSION}");
            Ok(0)
        }
    }
}

fn load_valid_config(config_path: &Path) -> Result<Config> {
    let config = Config::load(config_path)?;
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

fn check_config(config_path: &Path) -> Result<i32> {
    let config = load_valid_config(config_path)?;
    let mut callers = config
        .callers
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    callers.sort();
    println!("config OK: {}", config_path.display());
    println!("callers: {}", callers.join(", "));
    Ok(0)
}

fn explain_config(config_path: &Path) -> Result<i32> {
    let config = load_valid_config(config_path)?;
    let mut callers = config.callers.iter().collect::<Vec<_>>();
    callers.sort_by_key(|(caller, _)| *caller);

    println!("config: {}", config_path.display());
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

fn template_config(args: ConfigTemplateArgs) -> Result<i32> {
    let template = bootstrap::sample_config(args.backend);
    let Some(output) = args.output else {
        print!("{template}");
        return Ok(0);
    };

    if output.exists() && !args.force {
        return Err(Error::InvalidConfigOption(format!(
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

fn notify(args: NotifyArgs) -> Result<i32> {
    match args.provider {
        NotifyProviderCommand::Telegram(args) => notify_via_agentd(
            "telegram",
            &args.profile,
            Some(&args.chat_id),
            &args.agentd_socket,
            args.severity,
            args.title.as_deref(),
            &args.message,
        ),
        NotifyProviderCommand::Discord(args) => notify_via_agentd(
            "discord",
            &args.profile,
            None,
            &args.agentd_socket,
            args.severity,
            args.title.as_deref(),
            &args.message,
        ),
    }
}

fn notify_via_agentd(
    provider: &'static str,
    profile: &str,
    chat_id: Option<&str>,
    agentd_socket: &Path,
    severity: Severity,
    title: Option<&str>,
    message: &str,
) -> Result<i32> {
    validate_target(profile)?;
    if let Some(chat_id) = chat_id {
        if chat_id.trim().is_empty() {
            return Err(Error::InvalidNotificationOption(
                "telegram chat_id must not be empty".to_owned(),
            ));
        }
    }
    notification::validate_message_text(title, message)?;

    let caller = caller_from_sudo()?;
    let target = format!("{provider}:{profile}");
    let audit_path = audit::configured_audit_log_path();
    audit::write(
        &audit_path,
        &caller,
        Action::Notify.as_str(),
        &target,
        "delegate",
        Some("agentd"),
    )?;
    let notification = Notification {
        caller: &caller,
        provider,
        profile,
        chat_id,
        severity,
        title,
        message,
    };
    notification::send_via_agentd(agentd_socket, &notification)?;
    audit::write(
        &audit_path,
        &caller,
        Action::Notify.as_str(),
        &target,
        "executed",
        Some("sent"),
    )?;

    Ok(0)
}

fn execute(
    action: Action,
    target: &str,
    requested_lines: Option<u32>,
    config_path: &Path,
) -> Result<i32> {
    validate_target(target)?;
    let caller = caller_from_sudo()?;
    let config = load_valid_config(config_path)?;

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
        Action::Notify => {
            return Err(Error::InvalidConfigOption(
                "notify must use notify command".to_owned(),
            ))
        }
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
