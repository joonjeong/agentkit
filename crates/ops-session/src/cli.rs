use std::ffi::{OsStr, OsString};
use std::path::Path;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::github;

const VERSION: &str = match option_env!("OPS_SESSION_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Debug, Parser)]
#[command(name = "ops-session")]
#[command(version = VERSION)]
#[command(about = "Run a command in an authenticated operations session")]
#[command(after_long_help = "Invocation forms:
  ops-session github [OPTIONS] -- COMMAND [ARG]...
  ops-session github app-auth [OPTIONS]
  ops-session agent-skill --install-path DIR

For now, GitHub App authentication is the only supported session provider.")]
struct OpsSessionCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a command through a GitHub App installation token.
    Github(github::GithubSessionArgs),
    /// Create the GitHub App agent workflow skill.
    AgentSkill(github::AppAgentWorkflowSkillArgs),
}

#[derive(Debug, Parser)]
#[command(name = "ops-session github app-auth")]
#[command(version = VERSION)]
struct AppAuthCli {
    #[command(flatten)]
    args: github::AppAuthArgs,
}

pub fn run<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    if args.is_empty() {
        args.push(OsString::from("ops-session"));
    }

    let invoked_as = args
        .first()
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(OsStr::to_str)
        .unwrap_or("ops-session")
        .to_string();
    if invoked_as != "ops-session" {
        args[0] = OsString::from("ops-session");
    }

    match args.get(1).and_then(|arg| arg.to_str()) {
        Some("github") if args.get(2).and_then(|arg| arg.to_str()) == Some("app-auth") => {
            args.remove(2);
            args.remove(1);
            let cli = AppAuthCli::parse_from(args);
            github::app_auth(cli.args)
        }
        _ => {
            let cli = OpsSessionCli::parse_from(args);
            match cli.command {
                Command::Github(args) => github::github_session(args),
                Command::AgentSkill(args) => github::create_app_agent_workflow_skill(args),
            }
        }
    }
}
