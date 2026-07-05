use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use clap::{Args, Subcommand};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const OPS_SESSION_WORKFLOW_SKILL_NAME: &str = "ops-session-workflow";
const OPS_SESSION_WORKFLOW_SKILL: &str =
    include_str!("../resources/skills/ops-session-workflow/SKILL.md");
const GITHUB_APP_CONFIG_EXAMPLE: &str =
    include_str!("../resources/examples/github-app/config.example.toml");
const TOKEN_CACHE_EXPIRY_GRACE_SECONDS: i64 = 60;
const DEFAULT_GITHUB_CONFIG_PATH: &str = "/etc/ops-session/config.toml";

#[derive(Debug, Args)]
#[command(
    about = "GitHub App-backed operations session commands",
    after_long_help = "Invocation forms:
  ops-session github-app run [OPTIONS] -- COMMAND [ARG]...
  ops-session github-app config check
  ops-session github-app config example"
)]
pub struct GithubAppArgs {
    #[command(subcommand)]
    command: GithubAppSubcommand,
}

#[derive(Debug, Subcommand)]
enum GithubAppSubcommand {
    /// Run a command through a GitHub App installation token.
    Run(GithubSessionArgs),
    /// Validate or print GitHub App auth config examples.
    Config(GithubAuthConfigArgs),
}

#[derive(Debug, Args)]
#[command(
    about = "Run a command in a GitHub App-backed operations session",
    long_about = "Sign a GitHub App JWT, exchange it for an installation access token, and run a command with GH_TOKEN and GITHUB_TOKEN set for that process.

Use this command from coding agents or automation that need temporary GitHub repository access through a GitHub App installation without exporting a token into the parent shell.",
    after_long_help = "Purpose:
  Sign a GitHub App JWT, exchange it for an installation access token, and run a command with GH_TOKEN and GITHUB_TOKEN set for that process.

Invocation forms:
  ops-session github-app run [OPTIONS] -- COMMAND [ARG]...

Examples:
  ops-session github-app run \\
    --app-id \"$GITHUB_APP_ID\" \\
    --repo OWNER/REPO \\
    -- gh pr comment 123 --body \"Done\"

Environment:
  GITHUB_APP_ID
  GITHUB_APP_INSTALLATION_ID
  GITHUB_API_URL
  OPS_SESSION_GITHUB_CONFIG_PATH
  OPS_SESSION_GITHUB_PROFILE

Repository scoping:
  Use --profile NAME or OPS_SESSION_GITHUB_PROFILE to select the profile for the current agent. Use --repo OWNER/REPO to override the selected profile's repository list. Repeat --repo for multiple repositories. When --installation-id is omitted, the first repository is also used to discover the installation. OWNER/REPO is accepted for user-facing clarity; only repository names are sent to GitHub's installation token API.

Execution:
  The command after -- is run directly with GH_TOKEN and GITHUB_TOKEN set to the temporary installation token. GitHub App credential environment variables are removed from the child environment. The child process inherits stdin, stdout, stderr, working directory, PATH, and other ordinary environment variables. Shell syntax such as pipes, redirects, aliases, and shell functions requires an explicit shell command, for example -- sh -c 'gh issue view 123 | jq .url'.

Git HTTPS authentication:
  Git does not automatically use GH_TOKEN or GITHUB_TOKEN as HTTPS credentials. Pass --git-credentials to install a child-only Git credential helper that answers HTTPS credential requests for the GitHub host with username x-access-token and the temporary installation token."
)]
pub struct GithubSessionArgs {
    /// Path to the GitHub provider config file.
    ///
    /// Defaults to the user config path, then /etc/ops-session/config.toml.
    #[arg(long, env = "OPS_SESSION_GITHUB_CONFIG_PATH")]
    config_path: Option<PathBuf>,

    /// GitHub App ID.
    ///
    /// Can also be set with GITHUB_APP_ID or config file app_id.
    #[arg(long, env = "GITHUB_APP_ID")]
    app_id: Option<u64>,

    /// GitHub App installation ID.
    ///
    /// Can also be set with GITHUB_APP_INSTALLATION_ID. Prefer --repo OWNER/REPO
    /// unless the installation ID is already known.
    #[arg(long, env = "GITHUB_APP_INSTALLATION_ID")]
    installation_id: Option<u64>,

    /// GitHub API base URL.
    ///
    /// Override for GitHub Enterprise Server. Can also be set with
    /// GITHUB_API_URL or config file api_url.
    #[arg(long, env = "GITHUB_API_URL")]
    api_url: Option<String>,

    /// Named auth profile from the config file.
    ///
    /// Use this when multiple agents on a node need different repository scopes
    /// or token permissions.
    #[arg(long, env = "OPS_SESSION_GITHUB_PROFILE")]
    profile: Option<String>,

    /// Reuse a valid cached installation token.
    ///
    /// Disabled by default. When enabled, the selected config profile is part
    /// of the cache key.
    #[arg(long)]
    token_cache: bool,

    /// Scope the token to a repository.
    ///
    /// Repeat for multiple repositories. Without --installation-id, the first
    /// --repo value is also used to discover the installation. Public repository
    /// access alone is not enough; the GitHub App must be installed on the repo
    /// or owner.
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
    /// The helper responds only for the GitHub host derived from --api-url and
    /// reads the temporary installation token from GITHUB_TOKEN in the child
    /// environment.
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
#[command(about = "Validate GitHub App auth config or print examples")]
pub struct GithubAuthConfigArgs {
    #[command(subcommand)]
    command: GithubAuthConfigSubcommand,
}

#[derive(Debug, Subcommand)]
enum GithubAuthConfigSubcommand {
    /// Load and validate the GitHub App auth config file.
    Check(GithubAuthConfigCheckArgs),
    /// Print an example GitHub App auth config.
    Example(GithubAuthConfigExampleArgs),
}

#[derive(Debug, Args)]
struct GithubAuthConfigCheckArgs {
    /// Path to the GitHub App auth config file.
    ///
    /// Defaults to the user config path, then /etc/ops-session/config.toml.
    #[arg(long, env = "OPS_SESSION_GITHUB_CONFIG_PATH")]
    config_path: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct GithubAuthConfigExampleArgs {
    /// Write the example config to a file instead of stdout.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Args)]
#[command(
    about = "Create the ops-session agent workflow skill",
    long_about = "Create the bundled ops-session-workflow skill under a target skills directory.

The command writes INSTALL_PATH/ops-session-workflow/SKILL.md. Use it to install the agent-facing workflow guidance next to Codex, Hermes, or another agent's skill directory without copying files manually.",
    after_long_help = "Examples:
  ops-session agent-skill --install-path ~/.codex/skills
  ops-session agent-skill -i ./skills --force

Output:
  Prints the created skill directory path."
)]
pub struct AppAgentWorkflowSkillArgs {
    /// Directory where the skill folder should be created.
    ///
    /// The command creates <INSTALL_PATH>/ops-session-workflow/SKILL.md.
    #[arg(long, short = 'i', value_name = "INSTALL_PATH")]
    install_path: PathBuf,

    /// Overwrite an existing SKILL.md.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Serialize)]
struct Claims {
    iat: i64,
    exp: i64,
    iss: String,
}

#[derive(Debug, Serialize)]
struct TokenRequest {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    repositories: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct TokenCacheKey {
    config_path: String,
    profile: Option<String>,
    app_id: u64,
    installation_id: u64,
    api_url: String,
    repositories: Vec<String>,
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedToken {
    token: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationResponse {
    id: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PermissionArg {
    key: String,
    value: String,
}

#[derive(Debug, Deserialize, Default)]
struct OpsSessionConfigFile {
    github_app: Option<GithubConfigFile>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct GithubConfigFile {
    app_id: Option<u64>,
    installation_id: Option<u64>,
    api_url: Option<String>,
    default_profile: Option<String>,
    #[serde(default)]
    profiles: BTreeMap<String, GithubConfigProfile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum PrivateKeySource {
    File {
        path: PathBuf,
    },
    Command {
        command: PathBuf,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl PrivateKeySource {
    fn describe(&self) -> String {
        match self {
            Self::File { path } => format!("file:{}", path.display()),
            Self::Command { command, args } => {
                format!("command:{} {}", command.display(), args.join(" "))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GithubConfigProfile {
    app_id: Option<u64>,
    installation_id: Option<u64>,
    api_url: Option<String>,
    private_key: PrivateKeySource,
    #[serde(default)]
    repos: Vec<String>,
    #[serde(default)]
    permissions: BTreeMap<String, String>,
}

#[derive(Debug)]
struct ResolvedGithubConfig {
    config_path: PathBuf,
    profile_name: Option<String>,
    app_id: u64,
    installation_id: Option<u64>,
    private_key: PrivateKeySource,
    api_url: String,
    repos: Vec<String>,
    permissions: Vec<PermissionArg>,
}

trait AppTokenConfig {
    fn app_id(&self) -> u64;
    fn installation_id(&self) -> Option<u64>;
    fn private_key(&self) -> &PrivateKeySource;
    fn api_url(&self) -> &str;
    fn repos(&self) -> &[String];
    fn permissions(&self) -> &[PermissionArg];
    fn config_path(&self) -> &Path;
    fn profile_name(&self) -> Option<&str>;
}

trait GithubConfigArgs {
    fn config_path(&self) -> Option<&PathBuf>;
    fn app_id(&self) -> Option<u64>;
    fn installation_id(&self) -> Option<u64>;
    fn api_url(&self) -> Option<&str>;
    fn profile(&self) -> Option<&str>;
    fn repos(&self) -> &[String];
    fn permissions(&self) -> &[PermissionArg];
}

impl GithubConfigArgs for GithubSessionArgs {
    fn config_path(&self) -> Option<&PathBuf> {
        self.config_path.as_ref()
    }

    fn app_id(&self) -> Option<u64> {
        self.app_id
    }

    fn installation_id(&self) -> Option<u64> {
        self.installation_id
    }

    fn api_url(&self) -> Option<&str> {
        self.api_url.as_deref()
    }

    fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    fn repos(&self) -> &[String] {
        &self.repos
    }

    fn permissions(&self) -> &[PermissionArg] {
        &self.permissions
    }
}

impl AppTokenConfig for ResolvedGithubConfig {
    fn app_id(&self) -> u64 {
        self.app_id
    }

    fn installation_id(&self) -> Option<u64> {
        self.installation_id
    }

    fn private_key(&self) -> &PrivateKeySource {
        &self.private_key
    }

    fn api_url(&self) -> &str {
        &self.api_url
    }

    fn repos(&self) -> &[String] {
        &self.repos
    }

    fn permissions(&self) -> &[PermissionArg] {
        &self.permissions
    }

    fn config_path(&self) -> &Path {
        &self.config_path
    }

    fn profile_name(&self) -> Option<&str> {
        self.profile_name.as_deref()
    }
}

pub fn github_app(args: GithubAppArgs) -> Result<()> {
    match args.command {
        GithubAppSubcommand::Run(args) => github_session(args),
        GithubAppSubcommand::Config(args) => github_auth_config(args),
    }
}

pub fn github_session(args: GithubSessionArgs) -> Result<()> {
    let use_token_cache = args.token_cache;
    let config = resolve_github_config(&args)?;
    let token = if use_token_cache {
        cached_or_fresh_installation_token(&config)?.token
    } else {
        installation_token(&config)?.token
    };
    run_with_installation_token(
        &args.command,
        &token,
        args.git_credentials,
        config.api_url(),
    )
}

pub fn github_auth_config(args: GithubAuthConfigArgs) -> Result<()> {
    match args.command {
        GithubAuthConfigSubcommand::Check(args) => check_github_auth_config(args),
        GithubAuthConfigSubcommand::Example(args) => example_github_auth_config(args),
    }
}

fn check_github_auth_config(args: GithubAuthConfigCheckArgs) -> Result<()> {
    let config_path = github_config_path(args.config_path.as_ref())?;
    let file = read_required_github_config(&config_path)?;
    validate_github_config_file(&file, &config_path)?;

    println!("config OK: {}", config_path.display());
    if let Some(app_id) = file.app_id {
        println!("app_id: {app_id}");
    }
    if let Some(installation_id) = file.installation_id {
        println!("installation_id: {installation_id}");
    }
    println!(
        "api_url: {}",
        file.api_url
            .unwrap_or_else(|| "https://api.github.com".to_string())
    );
    if let Some(default_profile) = file.default_profile {
        println!("default_profile: {default_profile}");
    }
    println!("profiles:");
    for (name, profile) in file.profiles {
        println!("  {name}:");
        if let Some(app_id) = profile.app_id {
            println!("    app_id: {app_id}");
        }
        if let Some(installation_id) = profile.installation_id {
            println!("    installation_id: {installation_id}");
        }
        if let Some(api_url) = profile.api_url {
            println!("    api_url: {api_url}");
        }
        println!("    private_key: {}", profile.private_key.describe());
        println!("    repos: {}", format_string_list(&profile.repos));
        println!(
            "    permissions: {}",
            format_permission_map(&profile.permissions)
        );
    }

    Ok(())
}

fn example_github_auth_config(args: GithubAuthConfigExampleArgs) -> Result<()> {
    let example = github_app_config_example();
    let Some(output) = args.output else {
        print!("{example}");
        return Ok(());
    };

    if output.exists() && !args.force {
        return Err(anyhow!("output already exists: {}", output.display()));
    }

    fs::write(&output, example).with_context(|| format!("failed to write {}", output.display()))?;
    Ok(())
}

pub fn create_app_agent_workflow_skill(args: AppAgentWorkflowSkillArgs) -> Result<()> {
    let skill_dir = args.install_path.join(OPS_SESSION_WORKFLOW_SKILL_NAME);
    let skill_file = skill_dir.join("SKILL.md");

    if skill_file.exists() && !args.force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite it",
            skill_file.display()
        ));
    }

    fs::create_dir_all(&skill_dir)
        .with_context(|| format!("failed to create {}", skill_dir.display()))?;
    fs::write(&skill_file, OPS_SESSION_WORKFLOW_SKILL)
        .with_context(|| format!("failed to write {}", skill_file.display()))?;

    println!("{}", skill_dir.display());
    Ok(())
}

fn read_private_key(args: &impl AppTokenConfig) -> Result<String> {
    match args.private_key() {
        PrivateKeySource::File { path } => fs::read_to_string(path)
            .with_context(|| format!("failed to read private key from {}", path.display())),
        PrivateKeySource::Command { command, args } => {
            let output = Command::new(command).args(args).output().with_context(|| {
                format!("failed to run private key command {}", command.display())
            })?;
            if !output.status.success() {
                return Err(anyhow!(
                    "private key command {} exited with {}",
                    command.display(),
                    output.status
                ));
            }
            String::from_utf8(output.stdout).with_context(|| {
                format!(
                    "private key command {} returned non-UTF-8 stdout",
                    command.display()
                )
            })
        }
    }
}

fn resolve_github_config(args: &impl GithubConfigArgs) -> Result<ResolvedGithubConfig> {
    let config_path = github_config_path(args.config_path())?;
    let file = read_github_config(&config_path)?;
    let selected_profile = select_github_config_profile(&file, args, &config_path)?;
    let (profile_name, profile) = selected_profile.ok_or_else(|| {
        anyhow!(
            "missing GitHub auth config profile in {}; configure [github_app.profiles.<profile>]",
            config_path.display()
        )
    })?;
    let permissions = if args.permissions().is_empty() {
        permission_args_from_map(profile.permissions.clone())
    } else {
        args.permissions().to_vec()
    };
    let repos = if args.repos().is_empty() {
        profile.repos.clone()
    } else {
        args.repos().to_vec()
    };

    Ok(ResolvedGithubConfig {
        config_path: config_path.clone(),
        profile_name: Some(profile_name.to_string()),
        app_id: args
            .app_id()
            .or(profile.app_id)
            .or(file.app_id)
            .ok_or_else(|| {
                anyhow!(
                    "missing GitHub App ID; set --app-id, GITHUB_APP_ID, or app_id in {}",
                    config_path.display()
                )
            })?,
        installation_id: args
            .installation_id()
            .or(profile.installation_id)
            .or(file.installation_id),
        private_key: profile.private_key.clone(),
        api_url: args
            .api_url()
            .map(str::to_string)
            .or_else(|| profile.api_url.clone())
            .or(file.api_url)
            .unwrap_or_else(|| "https://api.github.com".to_string()),
        repos,
        permissions,
    })
}

fn select_github_config_profile<'a>(
    file: &'a GithubConfigFile,
    args: &impl GithubConfigArgs,
    path: &Path,
) -> Result<Option<(&'a str, &'a GithubConfigProfile)>> {
    if let Some(profile_name) = args.profile() {
        return file
            .profiles
            .get_key_value(profile_name)
            .map(|(name, profile)| Some((name.as_str(), profile)))
            .ok_or_else(|| {
                anyhow!(
                    "unknown GitHub auth config profile {profile_name:?} in {}",
                    path.display()
                )
            });
    }

    if let Some(default_profile) = &file.default_profile {
        return file
            .profiles
            .get_key_value(default_profile)
            .map(|(name, profile)| Some((name.as_str(), profile)))
            .ok_or_else(|| {
                anyhow!(
                    "default_profile {default_profile:?} is not defined in {}",
                    path.display()
                )
            });
    }

    if file.profiles.len() == 1 {
        return Ok(file
            .profiles
            .iter()
            .next()
            .map(|(name, profile)| (name.as_str(), profile)));
    }

    if file.profiles.len() > 1 && args.repos().is_empty() {
        return Err(anyhow!(
            "multiple GitHub auth config profiles are defined in {}; set --profile or default_profile",
            path.display()
        ));
    }

    Ok(None)
}

fn github_config_path(path: Option<&PathBuf>) -> Result<PathBuf> {
    if let Some(path) = path {
        return Ok(path.clone());
    }
    let system_path = PathBuf::from(DEFAULT_GITHUB_CONFIG_PATH);
    if let Some(user_path) = user_github_config_path() {
        if user_path.exists() {
            return Ok(user_path);
        }
    }
    Ok(system_path)
}

fn user_github_config_path() -> Option<PathBuf> {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(config_home)
                .join("ops-session")
                .join("config.toml"),
        );
    }
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("ops-session")
            .join("config.toml")
    })
}

fn read_github_config(path: &PathBuf) -> Result<GithubConfigFile> {
    match fs::read_to_string(path) {
        Ok(contents) => parse_github_config(&contents, path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(GithubConfigFile::default())
        }
        Err(error) => {
            Err(error).with_context(|| format!("failed to read GitHub config {}", path.display()))
        }
    }
}

fn read_required_github_config(path: &Path) -> Result<GithubConfigFile> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read GitHub config {}", path.display()))?;
    parse_github_config(&contents, path)
}

fn parse_github_config(contents: &str, path: &Path) -> Result<GithubConfigFile> {
    let file: OpsSessionConfigFile = toml::from_str(contents)
        .with_context(|| format!("failed to parse ops-session config {}", path.display()))?;
    file.github_app.ok_or_else(|| {
        anyhow!(
            "missing [github_app] section in ops-session config {}",
            path.display()
        )
    })
}

fn validate_github_config_file(file: &GithubConfigFile, path: &Path) -> Result<()> {
    if let Some(api_url) = &file.api_url {
        validate_api_url(api_url, path)?;
    }

    if file.profiles.is_empty() {
        return Err(anyhow!(
            "missing profiles in GitHub config file {}",
            path.display()
        ));
    }

    for (name, profile) in &file.profiles {
        if name.is_empty() {
            return Err(anyhow!(
                "profile name must not be empty in GitHub config file {}",
                path.display()
            ));
        }

        if profile.repos.is_empty() {
            return Err(anyhow!(
                "profile {:?} must include at least one repo in GitHub config file {}",
                name,
                path.display()
            ));
        }

        if let Some(api_url) = &profile.api_url {
            validate_api_url(api_url, path)?;
        }

        validate_private_key_source(
            &format!("github_app.profiles.{name}.private_key"),
            &profile.private_key,
            path,
        )?;

        if file.app_id.is_none() && profile.app_id.is_none() {
            return Err(anyhow!(
                "profile {:?} must resolve a GitHub App ID from app_id or profile app_id in GitHub config file {}",
                name,
                path.display()
            ));
        }

        for repo in &profile.repos {
            validate_repo_scope(repo).with_context(|| {
                format!("invalid repo in GitHub config file {}", path.display())
            })?;
        }

        for (key, value) in &profile.permissions {
            if key.is_empty() || value.is_empty() {
                return Err(anyhow!(
                    "profile {:?} permissions must use non-empty key/value entries in GitHub config file {}",
                    name,
                    path.display()
                ));
            }
        }
    }

    if let Some(default_profile) = &file.default_profile {
        if !file.profiles.contains_key(default_profile) {
            return Err(anyhow!(
                "default_profile {:?} is not defined in GitHub config file {}",
                default_profile,
                path.display()
            ));
        }
    }

    Ok(())
}

fn validate_api_url(api_url: &str, path: &Path) -> Result<()> {
    let parsed = Url::parse(api_url)
        .with_context(|| format!("invalid api_url in GitHub config file {}", path.display()))?;
    if parsed.host_str().is_none() {
        return Err(anyhow!(
            "api_url must include a host in GitHub config file {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_private_key_source(name: &str, source: &PrivateKeySource, path: &Path) -> Result<()> {
    match source {
        PrivateKeySource::File { path: key_path } => {
            validate_private_key_file_path(&format!("{name}.path"), key_path, path)
        }
        PrivateKeySource::Command { command, .. } => {
            validate_private_key_command(&format!("{name}.command"), command, path)
        }
    }
}

fn validate_private_key_command(name: &str, command: &Path, path: &Path) -> Result<()> {
    if !command.is_absolute() {
        return Err(anyhow!(
            "{name} must be absolute in GitHub config file {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_private_key_file_path(name: &str, key_path: &Path, path: &Path) -> Result<()> {
    if !key_path.is_absolute() {
        return Err(anyhow!(
            "{name} must be absolute in GitHub config file {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_repo_scope(repo: &str) -> Result<()> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return Err(anyhow!("expected OWNER/REPO, got {repo:?}"));
    }
    Ok(())
}

fn github_app_config_example() -> String {
    GITHUB_APP_CONFIG_EXAMPLE.to_string()
}

fn format_string_list(items: &[String]) -> String {
    let quoted = items
        .iter()
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>();
    format!("[{}]", quoted.join(", "))
}

fn format_permission_map(items: &BTreeMap<String, String>) -> String {
    let formatted = items
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    format!("[{}]", formatted.join(", "))
}

fn permission_args_from_map(map: BTreeMap<String, String>) -> Vec<PermissionArg> {
    map.into_iter()
        .map(|(key, value)| PermissionArg { key, value })
        .collect()
}

fn create_jwt(app_id: u64, private_key: &str) -> Result<String> {
    // The JWT is valid for 8 minutes, which is less than the 10-minute maximum.
    const JWT_LIFETIME_SECONDS: i64 = 8 * 60;
    // Account for clock skew by setting the "issued at" time to 60 seconds in the past.
    const JWT_IAT_SKEW_SECONDS: i64 = 60;

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let claims = Claims {
        iat: now - JWT_IAT_SKEW_SECONDS,
        exp: now + JWT_LIFETIME_SECONDS,
        iss: app_id.to_string(),
    };

    let key = EncodingKey::from_rsa_pem(private_key.as_bytes())
        .context("private key must be an RSA PEM key")?;
    jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &key)
        .context("failed to create GitHub App JWT")
}

fn installation_token(args: &impl AppTokenConfig) -> Result<TokenResponse> {
    let jwt = create_jwt(args.app_id(), &read_private_key(args)?)?;
    let client = github_client(&jwt)?;
    let installation_id = resolve_installation_id(args, &client)?;
    create_installation_token(args, &client, installation_id)
}

fn cached_or_fresh_installation_token(args: &impl AppTokenConfig) -> Result<TokenResponse> {
    let cache_path = token_cache_path(args).ok();

    if let Some(cache_path) = cache_path.as_ref() {
        if let Ok(Some(cached)) = read_cached_token(cache_path) {
            if cached_token_is_fresh(&cached)
                && validate_cached_token(args.api_url(), &cached.token).is_ok()
            {
                return Ok(TokenResponse {
                    token: cached.token,
                    expires_at: cached.expires_at,
                });
            }
        }
    }

    let response = installation_token(args)?;
    if let Some(cache_path) = cache_path.as_ref() {
        let _ = write_cached_token(cache_path, &response);
    }
    Ok(response)
}

fn token_cache_key(args: &impl AppTokenConfig) -> Result<TokenCacheKey> {
    let mut repositories = args.repos().to_vec();
    repositories.sort();

    Ok(TokenCacheKey {
        config_path: cache_config_path(args.config_path()),
        profile: args.profile_name().map(str::to_string),
        app_id: args.app_id(),
        installation_id: resolve_cache_installation_id(args)?,
        api_url: args.api_url().trim_end_matches('/').to_string(),
        repositories,
        permissions: permissions_map(args.permissions()),
    })
}

fn cache_config_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn resolve_cache_installation_id(args: &impl AppTokenConfig) -> Result<u64> {
    if let Some(installation_id) = args.installation_id() {
        return Ok(installation_id);
    }

    let jwt = create_jwt(args.app_id(), &read_private_key(args)?)?;
    let client = github_client(&jwt)?;
    resolve_installation_id(args, &client)
}

fn token_cache_path(args: &impl AppTokenConfig) -> Result<PathBuf> {
    let cache_key = token_cache_key(args)?;
    let encoded = serde_json::to_vec(&cache_key).context("failed to serialize token cache key")?;
    let digest = Sha256::digest(encoded);
    let file_name = format!(
        "{}.json",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    );
    Ok(token_cache_dir()?.join(file_name))
}

fn token_cache_dir() -> Result<PathBuf> {
    if let Some(cache_home) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(cache_home).join("ops-session"));
    }

    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".cache").join("ops-session"));
    }

    Err(anyhow!(
        "cannot determine token cache directory; set XDG_CACHE_HOME or HOME"
    ))
}

fn read_cached_token(path: &PathBuf) -> Result<Option<CachedToken>> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let token = serde_json::from_str(&contents).ok();
            Ok(token)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("failed to read token cache {}", path.display()))
        }
    }
}

fn write_cached_token(path: &PathBuf, response: &TokenResponse) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("token cache path has no parent directory"));
    };
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create token cache directory {}",
            parent.display()
        )
    })?;

    let contents = serde_json::to_vec(&CachedToken {
        token: response.token.clone(),
        expires_at: response.expires_at.clone(),
    })
    .context("failed to serialize token cache")?;

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options
        .open(path)
        .with_context(|| format!("failed to write token cache {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("failed to set token cache permissions {}", path.display()))?;
    }
    file.write_all(&contents)
        .with_context(|| format!("failed to write token cache {}", path.display()))?;
    Ok(())
}

fn cached_token_is_fresh(cached: &CachedToken) -> bool {
    let Some(expires_at) = cached.expires_at.as_deref() else {
        return false;
    };
    let Ok(expires_at) = OffsetDateTime::parse(expires_at, &Rfc3339) else {
        return false;
    };
    let refresh_after = expires_at - time::Duration::seconds(TOKEN_CACHE_EXPIRY_GRACE_SECONDS);
    OffsetDateTime::now_utc() < refresh_after
}

fn validate_cached_token(api_url: &str, token: &str) -> Result<()> {
    let client = installation_token_client(token)?;
    let url = format!(
        "{}/installation/repositories",
        api_url.trim_end_matches('/')
    );
    send_github_request(
        client.get(url),
        "GitHub cached installation token validation",
    )?;
    Ok(())
}

fn resolve_installation_id(args: &impl AppTokenConfig, client: &Client) -> Result<u64> {
    if let Some(installation_id) = args.installation_id() {
        return Ok(installation_id);
    }

    let repo = args
        .repos()
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing repository; set --repo OWNER/REPO"))?;
    discover_installation_id(args, client, repo)
}

fn discover_installation_id(
    args: &impl AppTokenConfig,
    client: &Client,
    repo: &str,
) -> Result<u64> {
    let (owner, name) = repo.split_once('/').ok_or_else(|| {
        anyhow!("--repo must be OWNER/REPO so the GitHub installation can be discovered")
    })?;
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(anyhow!(
            "--repo must be OWNER/REPO so the GitHub installation can be discovered"
        ));
    }

    let url = format!(
        "{}/repos/{}/{}/installation",
        args.api_url().trim_end_matches('/'),
        owner,
        name
    );
    let text = send_github_request(client.get(url), "GitHub repository installation API")
        .with_context(|| {
            format!(
                "failed to discover GitHub App installation for {repo}; public repository access is not enough. The GitHub App must be installed on the repository or owner before an installation token can be minted"
            )
        })?;
    let response: InstallationResponse =
        serde_json::from_str(&text).context("failed to parse GitHub installation response")?;
    Ok(response.id)
}

fn create_installation_token(
    args: &impl AppTokenConfig,
    client: &Client,
    installation_id: u64,
) -> Result<TokenResponse> {
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        args.api_url().trim_end_matches('/'),
        installation_id
    );
    let body = TokenRequest {
        repositories: token_repository_names(args),
        permissions: permissions_map(args.permissions()),
    };

    let text = send_github_request(
        client.post(url).json(&body),
        "GitHub installation token API",
    )?;

    let response: TokenResponse =
        serde_json::from_str(&text).context("failed to parse GitHub token response")?;
    Ok(response)
}

fn github_client(jwt: &str) -> Result<Client> {
    Client::builder()
        .default_headers(default_headers(jwt)?)
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitHub API client")
}

fn installation_token_client(token: &str) -> Result<Client> {
    Client::builder()
        .default_headers(default_token_headers(token)?)
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build GitHub API client")
}

fn send_github_request(
    request: reqwest::blocking::RequestBuilder,
    api_name: &str,
) -> Result<String> {
    let response = request
        .send()
        .with_context(|| format!("failed to call {api_name}"))?;

    let status = response.status();
    let text = response
        .text()
        .context("failed to read GitHub API response body")?;

    if !status.is_success() {
        return Err(anyhow!("GitHub API returned {status}: {text}"));
    }

    Ok(text)
}

fn default_headers(jwt: &str) -> Result<HeaderMap> {
    auth_headers(&format!("Bearer {jwt}"))
}

fn default_token_headers(token: &str) -> Result<HeaderMap> {
    auth_headers(&format!("Bearer {token}"))
}

fn auth_headers(authorization: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("ops-session/github-app"),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/vnd.github+json"),
    );
    headers.insert(
        "X-GitHub-Api-Version",
        HeaderValue::from_static("2022-11-28"),
    );
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(authorization).context("failed to build authorization header")?,
    );
    Ok(headers)
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
        let temp_dir = unique_temp_dir("ops-session-git-credentials");
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&temp_dir)
            .with_context(|| format!("failed to create {}", temp_dir.display()))?;

        let helper_path = temp_dir.join("git-credential-ops-session");
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

fn token_repository_names(args: &impl AppTokenConfig) -> Vec<String> {
    repository_names(args.repos())
}

fn repository_names(repositories: &[String]) -> Vec<String> {
    repositories
        .iter()
        .map(|repository| {
            repository
                .rsplit_once('/')
                .map_or(repository.as_str(), |(_, name)| name)
                .to_string()
        })
        .collect()
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
    use super::{
        git_credential_host, permissions_map, repository_names, token_cache_key, PermissionArg,
        PrivateKeySource, ResolvedGithubConfig,
    };
    use std::path::PathBuf;

    #[test]
    fn extracts_repository_names_for_installation_token_request() {
        let repositories = vec![
            "acme/service".to_string(),
            "plain-repo".to_string(),
            "owner/nested/name".to_string(),
        ];

        assert_eq!(
            repository_names(&repositories),
            vec!["service", "plain-repo", "name"]
        );
    }

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

    #[test]
    fn token_cache_key_sorts_repository_scope() {
        let mut first = test_args();
        first.repos = vec!["acme/service".to_string(), "joonjeong/other".to_string()];
        let mut second = test_args();
        second.repos = vec!["joonjeong/other".to_string(), "acme/service".to_string()];

        let first_key = token_cache_key(&first).expect("cache key");
        let second_key = token_cache_key(&second).expect("cache key");

        assert_eq!(first_key.repositories, second_key.repositories);
    }

    #[test]
    fn token_cache_key_includes_profile_isolation_fields() {
        let mut first = test_args();
        first.profile_name = Some("read".to_string());

        let mut second = test_args();
        second.profile_name = Some("write".to_string());

        let first_key = token_cache_key(&first).expect("cache key");
        let second_key = token_cache_key(&second).expect("cache key");

        assert_ne!(first_key.profile, second_key.profile);
    }

    fn test_args() -> ResolvedGithubConfig {
        ResolvedGithubConfig {
            config_path: PathBuf::from("/tmp/config.toml"),
            profile_name: Some("default".to_string()),
            app_id: 1,
            installation_id: Some(2),
            private_key: PrivateKeySource::File {
                path: PathBuf::from("/tmp/private-key.pem"),
            },
            api_url: "https://api.github.com".to_string(),
            repos: vec!["acme/service".to_string()],
            permissions: Vec::new(),
        }
    }
}
