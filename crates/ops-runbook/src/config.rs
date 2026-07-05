use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::policy::{validate_caller, validate_target, Action};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/ops-runbook/config.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub callers: HashMap<String, CallerPolicy>,
    #[serde(default)]
    pub channels: HashMap<String, NotificationChannel>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub backend: Option<Backend>,
    pub max_log_lines: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Systemd,
    Openrc,
}

impl Backend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Openrc => "openrc",
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallerPolicy {
    #[serde(default)]
    pub service_control: Vec<String>,
    #[serde(default)]
    pub service_read: Vec<String>,
    #[serde(default)]
    pub notify: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum NotificationChannel {
    Telegram(TelegramChannel),
    Discord(DiscordChannel),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramChannel {
    pub chat_id: String,
    pub bot_token_env: Option<String>,
    pub bot_token_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordChannel {
    pub webhook_url_env: Option<String>,
    pub webhook_url_file: Option<PathBuf>,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        let config = toml::from_slice(&bytes).map_err(|source| Error::Config {
            path: path.to_owned(),
            source,
        })?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(Error::UnsupportedConfigVersion(self.version));
        }

        for (caller, policy) in &self.callers {
            validate_caller(caller)?;
            for action in [
                Action::ServiceStart,
                Action::ServiceStop,
                Action::ServiceRestart,
                Action::ServiceReload,
                Action::ServiceStatus,
                Action::Notify,
                Action::Logs,
            ] {
                for target in crate::policy::allowed_targets(policy, action) {
                    validate_target(target)?;
                }
            }
        }
        self.validate_channels()?;

        Ok(())
    }

    fn validate_channels(&self) -> Result<()> {
        for (name, channel) in &self.channels {
            validate_target(name)?;
            match channel {
                NotificationChannel::Telegram(channel) => {
                    validate_secret_ref(
                        &format!("channels.{name}.bot_token"),
                        channel.bot_token_env.as_deref(),
                        channel.bot_token_file.as_deref(),
                    )?;
                    if channel.chat_id.trim().is_empty() {
                        return Err(Error::InvalidNotificationOption(format!(
                            "channels.{name}.chat_id must not be empty"
                        )));
                    }
                }
                NotificationChannel::Discord(channel) => {
                    validate_secret_ref(
                        &format!("channels.{name}.webhook_url"),
                        channel.webhook_url_env.as_deref(),
                        channel.webhook_url_file.as_deref(),
                    )?;
                }
            }
        }

        for (caller, policy) in &self.callers {
            for channel in &policy.notify {
                if !self.channels.contains_key(channel) {
                    return Err(Error::InvalidNotificationOption(format!(
                        "callers.{caller}.notify references unknown channel: {channel}"
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn notification_channel(&self, channel: &str) -> Option<&NotificationChannel> {
        self.channels.get(channel)
    }

    pub fn max_log_lines(&self) -> u32 {
        self.defaults.max_log_lines.unwrap_or(1000)
    }

    pub fn backend(&self) -> Backend {
        self.defaults.backend.unwrap_or(Backend::Systemd)
    }
}

fn validate_secret_ref(name: &str, env: Option<&str>, file: Option<&Path>) -> Result<()> {
    match (env, file) {
        (Some(env), None) if !env.trim().is_empty() => Ok(()),
        (None, Some(file)) if file.is_absolute() => Ok(()),
        (Some(_), Some(_)) => Err(Error::InvalidNotificationOption(format!(
            "{name} must use either env or file, not both"
        ))),
        (None, Some(file)) => Err(Error::InvalidNotificationOption(format!(
            "{name}_file must be absolute: {}",
            file.display()
        ))),
        _ => Err(Error::InvalidNotificationOption(format!(
            "{name} must configure env or file"
        ))),
    }
}

pub fn configured_config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OPS_RUNBOOK_CONFIG_PATH") {
        return PathBuf::from(path);
    }

    PathBuf::from(DEFAULT_CONFIG_PATH)
}
