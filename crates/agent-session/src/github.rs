use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use reqwest::Url;
use serde::{Deserialize, Serialize};

const AGENT_SESSION_WORKFLOW_SKILL_NAME: &str = "agent-session-workflow";
const AGENT_SESSION_WORKFLOW_SKILL: &str =
    include_str!("../resources/skills/agent-session-workflow/SKILL.md");
const DEFAULT_AGENTD_SOCKET_PATH: &str = "/run/agentd/agentd.sock";
const AGENTD_WIRE_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Args)]
#[command(
    about = "GitHub App-backed operations session commands",
    after_long_help = "Invocation forms:
  agent-session github-app run [OPTIONS] -- COMMAND [ARG]..."
)]
pub struct GithubAppArgs {
    #[command(subcommand)]
    command: GithubAppSubcommand,
}

#[derive(Debug, Subcommand)]
enum GithubAppSubcommand {
    /// Run a command through a GitHub App installation token.
    Run(GithubSessionArgs),
}

#[derive(Debug, Args)]
#[command(
    about = "Run a command in a GitHub App-backed operations session",
    long_about = "Request a GitHub App installation token from agentd and run a command with GH_TOKEN and GITHUB_TOKEN set for that process.

Use this command from coding agents or automation that need temporary GitHub repository access through a GitHub App installation without exporting a token into the parent shell.",
    after_long_help = "Purpose:
  Request a GitHub App installation token from agentd and run a command with GH_TOKEN and GITHUB_TOKEN set for that process.

Invocation forms:
  agent-session github-app run [OPTIONS] -- COMMAND [ARG]...

Examples:
  agent-session github-app run \\
    --repo OWNER/REPO \\
    -- gh pr comment 123 --body \"Done\"

Environment:
  AGENT_SESSION_AGENTD_SOCKET
  AGENT_SESSION_GITHUB_PROFILE

Repository scoping:
  Set AGENT_SESSION_GITHUB_PROFILE as the default profile selector for agent services. Use --profile NAME only for one-off overrides. Use --repo OWNER/REPO to request a subset of the selected profile's repository list. Repeat --repo for multiple repositories. agentd validates requested repositories and permissions against its system-wide profile before minting a token.

Execution:
  The command after -- is run directly with GH_TOKEN and GITHUB_TOKEN set to the temporary installation token. GitHub App credential environment variables are removed from the child environment. The child process inherits stdin, stdout, stderr, working directory, PATH, and other ordinary environment variables. Shell syntax such as pipes, redirects, aliases, and shell functions requires an explicit shell command, for example -- sh -c 'gh issue view 123 | jq .url'.

Git HTTPS authentication:
  Git does not automatically use GH_TOKEN or GITHUB_TOKEN as HTTPS credentials. Pass --git-credentials to install a child-only Git credential helper that answers HTTPS credential requests for the GitHub host with username x-access-token and the temporary installation token."
)]
pub struct GithubSessionArgs {
    /// agentd Unix domain socket path.
    #[arg(
        long,
        env = "AGENT_SESSION_AGENTD_SOCKET",
        default_value = DEFAULT_AGENTD_SOCKET_PATH
    )]
    agentd_socket: PathBuf,

    /// One-off GitHub App profile override.
    ///
    /// Prefer AGENT_SESSION_GITHUB_PROFILE as the default profile selector for
    /// long-running agent services.
    #[arg(long, env = "AGENT_SESSION_GITHUB_PROFILE")]
    profile: Option<String>,

    /// Scope the token to a repository.
    ///
    /// Repeat for multiple repositories. agentd validates requested repositories
    /// against the selected profile before minting a token.
    #[arg(long = "repo", value_name = "OWNER/REPO")]
    repos: Vec<String>,

    /// Limit installation token permissions.
    ///
    /// Repeat as key=value, for example --permission contents=read. Values are
    /// sent unchanged to GitHub's installation token API.
    #[arg(long = "permission", value_name = "KEY=VALUE")]
    permissions: Vec<PermissionArg>,

    /// Configure a child-only Git credential helper for HTTPS GitHub remotes.
    ///
    /// The helper responds only for the GitHub host returned by agentd and reads
    /// the temporary installation token from GITHUB_TOKEN in the child environment.
    #[arg(long)]
    git_credentials: bool,

    /// Command to run with GH_TOKEN and GITHUB_TOKEN set.
    #[arg(
        value_name = "COMMAND",
        required = true,
        num_args = 1..,
        last = true,
        allow_hyphen_values = true
    )]
    command: Vec<OsString>,
}

#[derive(Debug, Args)]
#[command(
    about = "Create the agent-session agent workflow skill",
    long_about = "Create the bundled agent-session-workflow skill under a target skills directory.

The command writes INSTALL_PATH/agent-session-workflow/SKILL.md. Use it to install the agent-facing workflow guidance next to Codex, Hermes, or another agent's skill directory without copying files manually.",
    after_long_help = "Examples:
  agent-session agent-skill --install-path ~/.codex/skills
  agent-session agent-skill -i ./skills --force

Output:
  Prints the created skill directory path."
)]
pub struct AppAgentWorkflowSkillArgs {
    /// Directory where the skill folder should be created.
    ///
    /// The command creates <INSTALL_PATH>/agent-session-workflow/SKILL.md.
    #[arg(long, short = 'i', value_name = "INSTALL_PATH")]
    install_path: PathBuf,

    /// Overwrite an existing SKILL.md.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Serialize)]
struct AgentdWireGithubTokenRequest {
    version: u32,
    #[serde(rename = "type")]
    request_type: &'static str,
    profile: Option<String>,
    repos: Vec<String>,
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum AgentdWireTokenResponse {
    Ok {
        version: u32,
        token: String,
        #[allow(dead_code)]
        expires_at: Option<String>,
        api_url: String,
    },
    Error {
        version: u32,
        error: String,
    },
}

struct AgentGithubToken {
    token: String,
    api_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PermissionArg {
    key: String,
    value: String,
}

pub fn github_app(args: GithubAppArgs) -> Result<()> {
    match args.command {
        GithubAppSubcommand::Run(args) => github_session(args),
    }
}

pub fn github_session(args: GithubSessionArgs) -> Result<()> {
    let token = request_agentd_github_token(&args)?;
    run_with_installation_token(
        &args.command,
        &token.token,
        args.git_credentials,
        &token.api_url,
    )
}

fn request_agentd_github_token(args: &GithubSessionArgs) -> Result<AgentGithubToken> {
    let request = AgentdWireGithubTokenRequest {
        version: AGENTD_WIRE_PROTOCOL_VERSION,
        request_type: "github_app_token",
        profile: args.profile.clone(),
        repos: args.repos.clone(),
        permissions: permissions_map(&args.permissions),
    };

    let mut stream = UnixStream::connect(&args.agentd_socket).with_context(|| {
        format!(
            "failed to connect to agentd socket {}",
            args.agentd_socket.display()
        )
    })?;
    serde_json::to_writer(&mut stream, &request).context("failed to write agentd request")?;
    stream
        .write_all(b"\n")
        .context("failed to finish agentd request")?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader
        .read_line(&mut response)
        .context("failed to read agentd response")?;
    match serde_json::from_str::<AgentdWireTokenResponse>(&response)
        .context("failed to parse agentd response")?
    {
        AgentdWireTokenResponse::Ok {
            version,
            token,
            expires_at: _,
            api_url,
        } => {
            validate_agentd_wire_version(version)?;
            Ok(AgentGithubToken { token, api_url })
        }
        AgentdWireTokenResponse::Error { version, error } => {
            validate_agentd_wire_version(version)?;
            Err(anyhow!("agentd rejected GitHub App token request: {error}"))
        }
    }
}

fn validate_agentd_wire_version(version: u32) -> Result<()> {
    if version != AGENTD_WIRE_PROTOCOL_VERSION {
        return Err(anyhow!(
            "unsupported agentd wire protocol version {version}; expected {AGENTD_WIRE_PROTOCOL_VERSION}"
        ));
    }
    Ok(())
}

pub fn create_app_agent_workflow_skill(args: AppAgentWorkflowSkillArgs) -> Result<()> {
    let skill_dir = args.install_path.join(AGENT_SESSION_WORKFLOW_SKILL_NAME);
    let skill_file = skill_dir.join("SKILL.md");

    if skill_file.exists() && !args.force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite it",
            skill_file.display()
        ));
    }

    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;
    fs::write(&skill_file, AGENT_SESSION_WORKFLOW_SKILL)
        .with_context(|| format!("failed to write {}", skill_file.display()))?;

    println!("{}", skill_dir.display());
    Ok(())
}

fn run_with_installation_token(
    command: &[OsString],
    token: &str,
    git_credentials: bool,
    api_url: &str,
) -> Result<()> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow!("missing command after --"))?;

    let git_credential_environment = if git_credentials {
        Some(GitCredentialEnvironment::create(api_url)?)
    } else {
        None
    };

    let mut child = Command::new(program);
    child
        .args(args)
        .env_remove("GITHUB_APP_ID")
        .env_remove("GITHUB_APP_INSTALLATION_ID")
        .env_remove("GITHUB_APP_PRIVATE_KEY")
        .env_remove("GITHUB_APP_PRIVATE_KEY_FILE")
        .env_remove("GITHUB_APP_PRIVATE_KEY_PATH")
        .env_remove("GITHUB_API_URL")
        .env("GH_TOKEN", token)
        .env("GITHUB_TOKEN", token);

    if let Some(environment) = &git_credential_environment {
        environment.configure_child(&mut child);
    }

    let status = child
        .status()
        .with_context(|| format!("failed to run {}", program.to_string_lossy()))?;
    drop(git_credential_environment);

    if status.success() {
        return Ok(());
    }

    if let Some(code) = status.code() {
        std::process::exit(code);
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            std::process::exit(128 + signal);
        }
    }

    Err(anyhow!("command terminated before exiting"))
}

struct GitCredentialEnvironment {
    temp_dir: PathBuf,
    helper_path: PathBuf,
}

impl GitCredentialEnvironment {
    fn create(api_url: &str) -> Result<Self> {
        let host = git_credential_host(api_url)?;
        let temp_dir = unique_temp_dir("agent-session-git-credentials");
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&temp_dir)
            .with_context(|| format!("failed to create {}", temp_dir.display()))?;

        let helper_path = temp_dir.join("git-credential-agent-session");
        fs::write(&helper_path, git_credential_helper_script(&host))
            .with_context(|| format!("failed to write {}", helper_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&helper_path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("failed to make {} executable", helper_path.display()))?;
        }

        Ok(Self {
            temp_dir,
            helper_path,
        })
    }

    fn configure_child(&self, child: &mut Command) {
        let config_index = std::env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);

        child
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_COUNT", (config_index + 2).to_string())
            .env(
                format!("GIT_CONFIG_KEY_{config_index}"),
                "credential.helper",
            )
            .env(format!("GIT_CONFIG_VALUE_{config_index}"), "")
            .env(
                format!("GIT_CONFIG_KEY_{}", config_index + 1),
                "credential.helper",
            )
            .env(
                format!("GIT_CONFIG_VALUE_{}", config_index + 1),
                &self.helper_path,
            );
    }
}

impl Drop for GitCredentialEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()))
}

fn git_credential_host(api_url: &str) -> Result<String> {
    let url = Url::parse(api_url).with_context(|| format!("invalid GitHub API URL: {api_url}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("GitHub API URL must include a host"))?;

    if host.eq_ignore_ascii_case("api.github.com") {
        Ok("github.com".to_string())
    } else {
        Ok(host.to_ascii_lowercase())
    }
}

fn git_credential_helper_script(host: &str) -> String {
    format!(
        r#"#!/bin/sh
test "$1" = get || exit 0

protocol=
host=
while IFS= read -r line; do
    test -n "$line" || break
    case "$line" in
        protocol=*) protocol=${{line#protocol=}} ;;
        host=*) host=${{line#host=}} ;;
    esac
done

test "$protocol" = https || exit 0
host_no_port=${{host%%:*}}
test "$host_no_port" = {quoted_host} || exit 0
test -n "$GITHUB_TOKEN" || exit 0

printf '%s\n' username=x-access-token
printf '%s\n' "password=$GITHUB_TOKEN"
"#,
        quoted_host = shell_single_quote(host)
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r#"'\''"#))
}

fn permissions_map(permissions: &[PermissionArg]) -> BTreeMap<String, String> {
    permissions
        .iter()
        .map(|permission| (permission.key.clone(), permission.value.clone()))
        .collect()
}

impl std::str::FromStr for PermissionArg {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        let (key, permission_value) = value
            .split_once('=')
            .ok_or_else(|| anyhow!("--permission must be KEY=VALUE"))?;
        if key.is_empty() || permission_value.is_empty() {
            return Err(anyhow!("--permission must be KEY=VALUE"));
        }
        Ok(Self {
            key: key.to_string(),
            value: permission_value.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{git_credential_host, permissions_map, PermissionArg};

    #[test]
    fn parses_permission_arguments() {
        let permissions = vec![
            "contents=read".parse::<PermissionArg>().unwrap(),
            "pull_requests=write".parse::<PermissionArg>().unwrap(),
        ];
        let mapped = permissions_map(&permissions);

        assert_eq!(mapped.get("contents").map(String::as_str), Some("read"));
        assert_eq!(
            mapped.get("pull_requests").map(String::as_str),
            Some("write")
        );
    }

    #[test]
    fn maps_github_api_host_to_git_credential_host() {
        assert_eq!(
            git_credential_host("https://api.github.com").expect("host parses"),
            "github.com"
        );
        assert_eq!(
            git_credential_host("https://ghe.example.com/api/v3").expect("host parses"),
            "ghe.example.com"
        );
    }
}
