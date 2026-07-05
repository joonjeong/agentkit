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
  ops-session github-app run [OPTIONS] -- COMMAND [ARG]...
  ops-session github-app config check
  ops-session github-app config template
  ops-session agent-skill --install-path DIR

For now, GitHub App authentication is the only supported session provider.")]
struct OpsSessionCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// GitHub App-backed operations session commands.
    #[command(name = "github-app")]
    GithubApp(github::GithubAppArgs),
    /// Create the GitHub App agent workflow skill.
    AgentSkill(github::AppAgentWorkflowSkillArgs),
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

    let cli = OpsSessionCli::parse_from(args);
    match cli.command {
        Command::GithubApp(args) => github::github_app(args),
        Command::AgentSkill(args) => github::create_app_agent_workflow_skill(args),
    }
}
