use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{Args, Parser, Subcommand};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

const DEFAULT_CONFIG_PATH: &str = "/etc/agentd/config.toml";
const DEFAULT_SOCKET_PATH: &str = "/run/agentd/agentd.sock";

const VERSION: &str = match option_env!("AGENTD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Debug, Parser)]
#[command(name = "agentd")]
#[command(version = VERSION)]
#[command(about = "Local agent credential broker")]
struct AgentdCli {
    #[command(subcommand)]
    command: CommandLine,
}

#[derive(Debug, Subcommand)]
enum CommandLine {
    /// Serve local credential requests over a Unix domain socket.
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

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct AgentdConfigFile {
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
struct ResolvedGithubProfile {
    app_id: u64,
    installation_id: Option<u64>,
    private_key: PrivateKeySource,
    api_url: String,
    repos: Vec<String>,
    permissions: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum AgentRequest {
    GithubAppToken {
        profile: Option<String>,
        #[serde(default)]
        repos: Vec<String>,
        #[serde(default)]
        permissions: BTreeMap<String, String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum AgentResponse {
    Ok {
        token: String,
        expires_at: Option<String>,
        api_url: String,
    },
    Error {
        error: String,
    },
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

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationResponse {
    id: u64,
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
        CommandLine::Serve(args) => serve(args),
        CommandLine::Config(args) => config(args),
    }
}

fn serve(args: ServeArgs) -> Result<()> {
    let config = load_config(&args.config_path)?;
    validate_config(&config, &args.config_path)?;

    if let Some(parent) = args.socket_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }
    if args.socket_path.exists() {
        fs::remove_file(&args.socket_path).with_context(|| {
            format!(
                "failed to remove stale socket {}",
                args.socket_path.display()
            )
        })?;
    }

    let listener = UnixListener::bind(&args.socket_path)
        .with_context(|| format!("failed to bind {}", args.socket_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&args.socket_path, fs::Permissions::from_mode(0o660)).with_context(
            || {
                format!(
                    "failed to set permissions on {}",
                    args.socket_path.display()
                )
            },
        )?;
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_stream(stream, &config)?,
            Err(error) => return Err(error).context("failed to accept agentd connection"),
        }
        if args.once {
            break;
        }
    }
    Ok(())
}

fn handle_stream(stream: UnixStream, config: &AgentdConfigFile) -> Result<()> {
    let mut writer = stream
        .try_clone()
        .context("failed to clone agentd client stream")?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("failed to read agentd request")?;

    let response = match serde_json::from_str::<AgentRequest>(&line) {
        Ok(request) => match handle_request(request, config) {
            Ok(response) => response,
            Err(error) => AgentResponse::Error {
                error: format!("{error:#}"),
            },
        },
        Err(error) => AgentResponse::Error {
            error: format!("invalid request: {error}"),
        },
    };

    serde_json::to_writer(&mut writer, &response).context("failed to write agentd response")?;
    writer
        .write_all(b"\n")
        .context("failed to finish agentd response")?;
    Ok(())
}

fn handle_request(request: AgentRequest, config: &AgentdConfigFile) -> Result<AgentResponse> {
    match request {
        AgentRequest::GithubAppToken {
            profile,
            repos,
            permissions,
        } => {
            let profile = resolve_github_profile(config, profile.as_deref(), repos, permissions)?;
            let token = installation_token(&profile)?;
            Ok(AgentResponse::Ok {
                token: token.token,
                expires_at: token.expires_at,
                api_url: profile.api_url,
            })
        }
    }
}

fn config(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigSubcommand::Check(args) => {
            let config = load_config(&args.config_path)?;
            validate_config(&config, &args.config_path)?;
            println!("config OK: {}", args.config_path.display());
            if let Some(github_app) = config.github_app {
                println!("github_app profiles: {}", github_app.profiles.len());
                if let Some(default_profile) = github_app.default_profile {
                    println!("default_profile: {default_profile}");
                }
            }
            Ok(())
        }
        ConfigSubcommand::Example => {
            print!("{}", example_config());
            Ok(())
        }
    }
}

fn load_config(path: &Path) -> Result<AgentdConfigFile> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read agentd config {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("failed to parse agentd config {}", path.display()))
}

fn validate_config(config: &AgentdConfigFile, path: &Path) -> Result<()> {
    let Some(github_app) = &config.github_app else {
        return Err(anyhow!(
            "missing [github_app] section in {}",
            path.display()
        ));
    };
    if github_app.profiles.is_empty() {
        return Err(anyhow!("missing github_app profiles in {}", path.display()));
    }
    if let Some(api_url) = &github_app.api_url {
        validate_api_url(api_url, path)?;
    }
    if let Some(default_profile) = &github_app.default_profile {
        if !github_app.profiles.contains_key(default_profile) {
            return Err(anyhow!(
                "default_profile {:?} is not defined in {}",
                default_profile,
                path.display()
            ));
        }
    }
    for (name, profile) in &github_app.profiles {
        if name.is_empty() {
            return Err(anyhow!(
                "profile name must not be empty in {}",
                path.display()
            ));
        }
        if profile.repos.is_empty() {
            return Err(anyhow!(
                "profile {name:?} must include at least one repo in {}",
                path.display()
            ));
        }
        if github_app.app_id.is_none() && profile.app_id.is_none() {
            return Err(anyhow!(
                "profile {name:?} must resolve app_id in {}",
                path.display()
            ));
        }
        if let Some(api_url) = &profile.api_url {
            validate_api_url(api_url, path)?;
        }
        validate_private_key_source(name, &profile.private_key, path)?;
        for repo in &profile.repos {
            validate_repo_scope(repo).with_context(|| {
                format!("invalid repo in profile {name:?} in {}", path.display())
            })?;
        }
        for (key, value) in &profile.permissions {
            if key.is_empty() || value.is_empty() {
                return Err(anyhow!(
                    "profile {name:?} permissions must use non-empty key/value entries in {}",
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

fn resolve_github_profile(
    config: &AgentdConfigFile,
    requested_profile: Option<&str>,
    requested_repos: Vec<String>,
    requested_permissions: BTreeMap<String, String>,
) -> Result<ResolvedGithubProfile> {
    let github_app = config
        .github_app
        .as_ref()
        .ok_or_else(|| anyhow!("missing [github_app] section"))?;
    let (profile_name, profile) = select_profile(github_app, requested_profile)?;

    let repos = if requested_repos.is_empty() {
        profile.repos.clone()
    } else {
        for repo in &requested_repos {
            if !profile.repos.contains(repo) {
                return Err(anyhow!(
                    "repo {repo:?} is not allowed by GitHub App profile {profile_name:?}"
                ));
            }
        }
        requested_repos
    };

    let permissions = if requested_permissions.is_empty() {
        profile.permissions.clone()
    } else {
        for (key, value) in &requested_permissions {
            match profile.permissions.get(key) {
                Some(allowed) if allowed == value => {}
                Some(allowed) => {
                    return Err(anyhow!(
                        "permission {key}={value} is not allowed by GitHub App profile {profile_name:?}; configured value is {allowed}"
                    ));
                }
                None => {
                    return Err(anyhow!(
                        "permission {key} is not allowed by GitHub App profile {profile_name:?}"
                    ));
                }
            }
        }
        requested_permissions
    };

    Ok(ResolvedGithubProfile {
        app_id: profile
            .app_id
            .or(github_app.app_id)
            .ok_or_else(|| anyhow!("profile {profile_name:?} must resolve app_id"))?,
        installation_id: profile.installation_id.or(github_app.installation_id),
        private_key: profile.private_key.clone(),
        api_url: profile
            .api_url
            .clone()
            .or_else(|| github_app.api_url.clone())
            .unwrap_or_else(|| "https://api.github.com".to_string()),
        repos,
        permissions,
    })
}

fn select_profile<'a>(
    github_app: &'a GithubConfigFile,
    requested_profile: Option<&str>,
) -> Result<(&'a str, &'a GithubConfigProfile)> {
    if let Some(profile_name) = requested_profile {
        return github_app
            .profiles
            .get_key_value(profile_name)
            .map(|(name, profile)| (name.as_str(), profile))
            .ok_or_else(|| anyhow!("unknown GitHub App profile {profile_name:?}"));
    }
    if let Some(default_profile) = &github_app.default_profile {
        return github_app
            .profiles
            .get_key_value(default_profile)
            .map(|(name, profile)| (name.as_str(), profile))
            .ok_or_else(|| anyhow!("default_profile {default_profile:?} is not defined"));
    }
    if github_app.profiles.len() == 1 {
        return github_app
            .profiles
            .iter()
            .next()
            .map(|(name, profile)| (name.as_str(), profile))
            .ok_or_else(|| anyhow!("missing GitHub App profiles"));
    }
    Err(anyhow!(
        "multiple GitHub App profiles are defined; request a profile"
    ))
}

fn read_private_key(source: &PrivateKeySource) -> Result<String> {
    match source {
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

fn create_jwt(app_id: u64, private_key: &str) -> Result<String> {
    const JWT_LIFETIME_SECONDS: i64 = 8 * 60;
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

fn installation_token(profile: &ResolvedGithubProfile) -> Result<TokenResponse> {
    let jwt = create_jwt(profile.app_id, &read_private_key(&profile.private_key)?)?;
    let client = github_client(&jwt)?;
    let installation_id = resolve_installation_id(profile, &client)?;
    create_installation_token(profile, &client, installation_id)
}

fn resolve_installation_id(profile: &ResolvedGithubProfile, client: &Client) -> Result<u64> {
    if let Some(installation_id) = profile.installation_id {
        return Ok(installation_id);
    }
    let repo = profile
        .repos
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing repository in GitHub App profile"))?;
    let (owner, name) = repo.split_once('/').ok_or_else(|| {
        anyhow!("repo must be OWNER/REPO so the GitHub installation can be discovered")
    })?;
    let url = format!(
        "{}/repos/{}/{}/installation",
        profile.api_url.trim_end_matches('/'),
        owner,
        name
    );
    let text = send_github_request(client.get(url), "GitHub repository installation API")?;
    let response: InstallationResponse =
        serde_json::from_str(&text).context("failed to parse GitHub installation response")?;
    Ok(response.id)
}

fn create_installation_token(
    profile: &ResolvedGithubProfile,
    client: &Client,
    installation_id: u64,
) -> Result<TokenResponse> {
    let url = format!(
        "{}/app/installations/{}/access_tokens",
        profile.api_url.trim_end_matches('/'),
        installation_id
    );
    let body = TokenRequest {
        repositories: repository_names(&profile.repos),
        permissions: profile.permissions.clone(),
    };
    let text = send_github_request(
        client.post(url).json(&body),
        "GitHub installation token API",
    )?;
    serde_json::from_str(&text).context("failed to parse GitHub token response")
}

fn github_client(jwt: &str) -> Result<Client> {
    Client::builder()
        .default_headers(default_headers(jwt)?)
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
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("agentd/github-app"));
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
        HeaderValue::from_str(&format!("Bearer {jwt}"))
            .context("failed to build authorization header")?,
    );
    Ok(headers)
}

fn validate_api_url(api_url: &str, path: &Path) -> Result<()> {
    let parsed = Url::parse(api_url)
        .with_context(|| format!("invalid api_url in agentd config {}", path.display()))?;
    if parsed.host_str().is_none() {
        return Err(anyhow!(
            "api_url must include a host in agentd config {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_private_key_source(name: &str, source: &PrivateKeySource, path: &Path) -> Result<()> {
    match source {
        PrivateKeySource::File { path: key_path } => {
            if !key_path.is_absolute() {
                return Err(anyhow!(
                    "github_app.profiles.{name}.private_key.path must be absolute in {}",
                    path.display()
                ));
            }
        }
        PrivateKeySource::Command { command, .. } => {
            if !command.is_absolute() {
                return Err(anyhow!(
                    "github_app.profiles.{name}.private_key.command must be absolute in {}",
                    path.display()
                ));
            }
        }
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

fn example_config() -> &'static str {
    r#"[github_app]
app_id = 123456
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/agentd/secrets/codex-review-github-app.private-key.pem"

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"
issues = "read"
"#
}
