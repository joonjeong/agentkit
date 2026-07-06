use std::collections::BTreeMap;
use std::fs;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::config::{AgentdConfigFile, GithubConfigFile, GithubConfigProfile, PrivateKeySource};

#[derive(Debug)]
struct ResolvedGithubProfile {
    app_id: u64,
    installation_id: Option<u64>,
    private_key: PrivateKeySource,
    api_url: String,
    repos: Vec<String>,
    permissions: BTreeMap<String, String>,
}

pub(crate) struct GithubAppToken {
    pub(crate) token: String,
    pub(crate) expires_at: Option<String>,
    pub(crate) api_url: String,
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

pub(crate) fn installation_token(
    config: &AgentdConfigFile,
    requested_profile: Option<&str>,
    requested_repos: Vec<String>,
    requested_permissions: BTreeMap<String, String>,
) -> Result<GithubAppToken> {
    let profile = resolve_github_profile(
        config,
        requested_profile,
        requested_repos,
        requested_permissions,
    )?;
    let token = mint_installation_token(&profile)?;
    Ok(GithubAppToken {
        token: token.token,
        expires_at: token.expires_at,
        api_url: profile.api_url,
    })
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

fn mint_installation_token(profile: &ResolvedGithubProfile) -> Result<TokenResponse> {
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
