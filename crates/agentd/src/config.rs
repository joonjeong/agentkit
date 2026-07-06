use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use reqwest::Url;
use serde::Deserialize;

use crate::notification::{DiscordProfile, TelegramProfile};

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentdConfigFile {
    pub(crate) github_app: Option<GithubConfigFile>,
    pub(crate) telegram: Option<TelegramConfigFile>,
    pub(crate) discord: Option<DiscordConfigFile>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct GithubConfigFile {
    pub(crate) api_url: Option<String>,
    pub(crate) default_profile: Option<String>,
    #[serde(default)]
    pub(crate) profiles: BTreeMap<String, GithubConfigProfile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum PrivateKeySource {
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
pub(crate) struct GithubConfigProfile {
    pub(crate) app_id: Option<u64>,
    pub(crate) installation_id: Option<u64>,
    pub(crate) api_url: Option<String>,
    pub(crate) private_key: PrivateKeySource,
    #[serde(default)]
    pub(crate) repos: Vec<String>,
    #[serde(default)]
    pub(crate) permissions: BTreeMap<String, String>,
}

pub(crate) fn load(path: &Path) -> Result<AgentdConfigFile> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read agentd config {}", path.display()))?;
    toml::from_str(&contents)
        .with_context(|| format!("failed to parse agentd config {}", path.display()))
}

pub(crate) fn validate(config: &AgentdConfigFile, path: &Path) -> Result<()> {
    if config.github_app.is_none() && config.telegram.is_none() && config.discord.is_none() {
        return Err(anyhow!("missing provider section in {}", path.display()));
    };
    if let Some(github_app) = &config.github_app {
        validate_github_app(github_app, path)?;
    }
    if let Some(telegram) = &config.telegram {
        validate_telegram(telegram, path)?;
    }
    if let Some(discord) = &config.discord {
        validate_discord(discord, path)?;
    }
    Ok(())
}

pub(crate) fn example() -> &'static str {
    include_str!("../resources/examples/config.example.toml")
}

fn validate_github_app(github_app: &GithubConfigFile, path: &Path) -> Result<()> {
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
        if profile.app_id.is_none() {
            return Err(anyhow!(
                "profile {name:?} must define app_id in {}",
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

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelegramConfigFile {
    #[serde(default)]
    pub(crate) profiles: BTreeMap<String, TelegramProfile>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscordConfigFile {
    #[serde(default)]
    pub(crate) profiles: BTreeMap<String, DiscordProfile>,
}

fn validate_telegram(telegram: &TelegramConfigFile, path: &Path) -> Result<()> {
    if telegram.profiles.is_empty() {
        return Err(anyhow!("missing telegram profiles in {}", path.display()));
    }
    for (name, profile) in &telegram.profiles {
        validate_profile_name(name, path)?;
        crate::notification::validate_telegram_profile(name, profile)
            .with_context(|| format!("invalid telegram profile {name:?} in {}", path.display()))?;
    }
    Ok(())
}

fn validate_discord(discord: &DiscordConfigFile, path: &Path) -> Result<()> {
    if discord.profiles.is_empty() {
        return Err(anyhow!("missing discord profiles in {}", path.display()));
    }
    for (name, profile) in &discord.profiles {
        validate_profile_name(name, path)?;
        crate::notification::validate_discord_profile(name, profile)
            .with_context(|| format!("invalid discord profile {name:?} in {}", path.display()))?;
    }
    Ok(())
}

fn validate_profile_name(name: &str, path: &Path) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!(
            "profile name must not be empty in {}",
            path.display()
        ));
    }
    if name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        Ok(())
    } else {
        Err(anyhow!(
            "profile name may contain only ASCII letters, numbers, '_', '-', or '.': {name:?}"
        ))
    }
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
