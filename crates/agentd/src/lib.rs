mod bootstrap;
mod config;
mod github;
mod notification;
mod peer;
mod protocol;
mod server;
mod service;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

const DEFAULT_CONFIG_PATH: &str = "/etc/agentkit/agentd.toml";
const DEFAULT_SOCKET_PATH: &str = "/run/agentd/agentd.sock";

const VERSION: &str = match option_env!("AGENTKIT_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Debug, Parser)]
#[command(name = "agentd")]
#[command(version = VERSION)]
#[command(about = "Local agent service broker")]
struct AgentdCli {
    #[command(subcommand)]
    command: CommandLine,
}

#[derive(Debug, Subcommand)]
enum CommandLine {
    /// Bootstrap local installation and system-wide config.
    Bootstrap(bootstrap::BootstrapArgs),
    /// Serve local credential and notification requests over a Unix domain socket.
    Serve(ServeArgs),
    /// Validate or print agentd config.
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
struct ServeArgs {
    /// Path to the system-wide agentd config.
    #[arg(long, env = "AGENTD_CONFIG_PATH", default_value = DEFAULT_CONFIG_PATH)]
    config_path: PathBuf,

    /// Unix domain socket path to listen on.
    #[arg(long, env = "AGENTD_SOCKET_PATH", default_value = DEFAULT_SOCKET_PATH)]
    socket_path: PathBuf,

    /// Handle one request and exit. Intended for local tests and supervised checks.
    #[arg(long)]
    once: bool,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
enum ConfigSubcommand {
    /// Load and validate the agentd config file.
    Check(ConfigCheckArgs),
    /// Print an example agentd config.
    Example,
}

#[derive(Debug, Args)]
struct ConfigCheckArgs {
    /// Path to the system-wide agentd config.
    #[arg(long, env = "AGENTD_CONFIG_PATH", default_value = DEFAULT_CONFIG_PATH)]
    config_path: PathBuf,
}

pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if args.is_empty() {
        args.push(OsString::from("agentd"));
    }
    let cli = AgentdCli::parse_from(args);
    match cli.command {
        CommandLine::Bootstrap(args) => bootstrap::run(args),
        CommandLine::Serve(args) => serve(args),
        CommandLine::Config(args) => config(args),
    }
}

fn serve(args: ServeArgs) -> Result<()> {
    let config = config::load(&args.config_path)?;
    config::validate(&config, &args.config_path)?;
    server::serve(config, &args.socket_path, args.once)
}

fn config(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigSubcommand::Check(args) => {
            let config = config::load(&args.config_path)?;
            config::validate(&config, &args.config_path)?;
            println!("config OK: {}", args.config_path.display());
            if let Some(github_app) = config.github_app {
                println!("github_app profiles: {}", github_app.profiles.len());
                if let Some(default_profile) = github_app.default_profile {
                    println!("default_profile: {default_profile}");
                }
            }
            if let Some(telegram) = config.telegram {
                println!("telegram profiles: {}", telegram.profiles.len());
            }
            if let Some(discord) = config.discord {
                println!("discord profiles: {}", discord.profiles.len());
            }
            if let Some(service) = config.service {
                println!("service callers: {}", service.callers.len());
            }
            Ok(())
        }
        ConfigSubcommand::Example => {
            print!("{}", config::example());
            Ok(())
        }
    }
}
