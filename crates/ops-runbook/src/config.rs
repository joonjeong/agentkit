use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::notification;
use crate::policy::{validate_caller, validate_target, Action};

pub const DEFAULT_POLICY_PATH: &str = "/etc/ops-runbook/policy.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub callers: HashMap<String, CallerPolicy>,
    #[serde(default)]
    pub notifications: NotificationConfig,
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
    pub alarms: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationConfig {
    #[serde(default)]
    pub telegram: HashMap<String, TelegramDestination>,
    #[serde(default)]
    pub discord: HashMap<String, DiscordDestination>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TelegramDestination {
    pub chat_id: String,
    pub bot_token_env: Option<String>,
    pub bot_token_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscordDestination {
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
            return Err(Error::UnsupportedPolicyVersion(self.version));
        }

        for (caller, policy) in &self.callers {
            validate_caller(caller)?;
            for action in [
                Action::ServiceStart,
                Action::ServiceStop,
                Action::ServiceRestart,
                Action::ServiceReload,
                Action::ServiceStatus,
                Action::AlarmSend,
                Action::Logs,
            ] {
                for target in crate::policy::allowed_targets(policy, action) {
                    if action == Action::AlarmSend {
                        notification::validate_destination_ref(target)?;
                    } else {
                        validate_target(target)?;
                    }
                }
            }
        }
        self.validate_notifications()?;

        Ok(())
    }

    fn validate_notifications(&self) -> Result<()> {
        for name in self.notifications.telegram.keys() {
            validate_target(name)?;
        }
        for name in self.notifications.discord.keys() {
            validate_target(name)?;
        }

        for (name, destination) in &self.notifications.telegram {
            validate_secret_ref(
                &format!("notifications.telegram.{name}.bot_token"),
                destination.bot_token_env.as_deref(),
                destination.bot_token_file.as_deref(),
            )?;
            if destination.chat_id.trim().is_empty() {
                return Err(Error::InvalidNotificationOption(format!(
                    "notifications.telegram.{name}.chat_id must not be empty"
                )));
            }
        }

        for (name, destination) in &self.notifications.discord {
            validate_secret_ref(
                &format!("notifications.discord.{name}.webhook_url"),
                destination.webhook_url_env.as_deref(),
                destination.webhook_url_file.as_deref(),
            )?;
        }

        for (caller, policy) in &self.callers {
            for destination in &policy.alarms {
                if self.notification_destination(destination).is_none() {
                    return Err(Error::InvalidNotificationOption(format!(
                        "callers.{caller}.alarms references unknown destination: {destination}"
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn notification_destination(
        &self,
        destination: &str,
    ) -> Option<NotificationDestination<'_>> {
        let (provider, name) = destination.split_once('.')?;
        match provider {
            "telegram" => self
                .notifications
                .telegram
                .get(name)
                .map(NotificationDestination::Telegram),
            "discord" => self
                .notifications
                .discord
                .get(name)
                .map(NotificationDestination::Discord),
            _ => None,
        }
    }

    pub fn max_log_lines(&self) -> u32 {
        self.defaults.max_log_lines.unwrap_or(1000)
    }

    pub fn backend(&self) -> Backend {
        self.defaults.backend.unwrap_or(Backend::Systemd)
    }
}

pub enum NotificationDestination<'a> {
    Telegram(&'a TelegramDestination),
    Discord(&'a DiscordDestination),
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

pub fn configured_policy_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OPS_RUNBOOK_POLICY_PATH") {
        return PathBuf::from(path);
    }

    PathBuf::from(DEFAULT_POLICY_PATH)
}
