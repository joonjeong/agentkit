use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use serde::Serialize;

use crate::config::{DiscordDestination, NotificationDestination, TelegramDestination};
use crate::error::{Error, Result};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGE_LEN: usize = 1800;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

pub struct Alarm<'a> {
    pub caller: &'a str,
    pub destination: &'a str,
    pub severity: Severity,
    pub title: Option<&'a str>,
    pub message: &'a str,
}

pub fn validate_destination_ref(destination: &str) -> Result<()> {
    let Some((provider, name)) = destination.split_once('.') else {
        return Err(Error::InvalidNotificationOption(format!(
            "notification destination must be provider.name: {destination}"
        )));
    };

    if !matches!(provider, "telegram" | "discord") {
        return Err(Error::InvalidNotificationOption(format!(
            "unsupported notification provider: {provider}"
        )));
    }

    crate::policy::validate_target(name)
}

pub fn validate_alarm_text(title: Option<&str>, message: &str) -> Result<()> {
    if message.trim().is_empty() {
        return Err(Error::InvalidNotificationOption(
            "alarm message must not be empty".to_owned(),
        ));
    }
    if message.len() > MAX_MESSAGE_LEN {
        return Err(Error::InvalidNotificationOption(format!(
            "alarm message must be at most {MAX_MESSAGE_LEN} bytes"
        )));
    }
    if let Some(title) = title {
        if title.trim().is_empty() {
            return Err(Error::InvalidNotificationOption(
                "alarm title must not be empty".to_owned(),
            ));
        }
        if title.len() > 160 {
            return Err(Error::InvalidNotificationOption(
                "alarm title must be at most 160 bytes".to_owned(),
            ));
        }
    }

    Ok(())
}

pub fn send(destination: NotificationDestination<'_>, alarm: &Alarm<'_>) -> Result<()> {
    let client = Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .map_err(|source| Error::NotificationSend {
            destination: alarm.destination.to_owned(),
            reason: source.to_string(),
        })?;

    match destination {
        NotificationDestination::Telegram(destination) => {
            send_telegram(&client, destination, alarm)
        }
        NotificationDestination::Discord(destination) => send_discord(&client, destination, alarm),
    }
}

fn send_telegram(
    client: &Client,
    destination: &TelegramDestination,
    alarm: &Alarm<'_>,
) -> Result<()> {
    let token = read_secret(
        "telegram bot token",
        destination.bot_token_env.as_deref(),
        destination.bot_token_file.as_deref(),
    )?;
    let base_url =
        if cfg!(debug_assertions) && std::env::var_os("OPS_RUNBOOK_TEST_OVERRIDES").is_some() {
            std::env::var("OPS_RUNBOOK_TELEGRAM_API_BASE")
                .unwrap_or_else(|_| "https://api.telegram.org".to_owned())
        } else {
            "https://api.telegram.org".to_owned()
        };
    let url = format!("{}/bot{token}/sendMessage", base_url.trim_end_matches('/'));
    let payload = TelegramPayload {
        chat_id: &destination.chat_id,
        text: &format_alarm(alarm),
        disable_web_page_preview: true,
    };

    post_json(client, alarm.destination, &url, &payload)
}

fn send_discord(
    client: &Client,
    destination: &DiscordDestination,
    alarm: &Alarm<'_>,
) -> Result<()> {
    let url = read_secret(
        "discord webhook url",
        destination.webhook_url_env.as_deref(),
        destination.webhook_url_file.as_deref(),
    )?;
    let payload = DiscordPayload {
        content: &format_alarm(alarm),
    };

    post_json(client, alarm.destination, &url, &payload)
}

fn post_json<T>(client: &Client, destination: &str, url: &str, payload: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    if cfg!(debug_assertions) && std::env::var_os("OPS_RUNBOOK_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os("OPS_RUNBOOK_NOTIFICATION_RECORD_PATH") {
            let path = PathBuf::from(path);
            let payload =
                serde_json::to_string(payload).map_err(|source| Error::NotificationSend {
                    destination: destination.to_owned(),
                    reason: source.to_string(),
                })?;
            fs::write(
                &path,
                format!("destination={destination}\nurl={url}\n{payload}\n"),
            )
            .map_err(|source| Error::Io { path, source })?;
            return Ok(());
        }
    }

    let response =
        client
            .post(url)
            .json(payload)
            .send()
            .map_err(|source| Error::NotificationSend {
                destination: destination.to_owned(),
                reason: source.to_string(),
            })?;
    if response.status().is_success() {
        return Ok(());
    }

    Err(Error::NotificationSend {
        destination: destination.to_owned(),
        reason: format!("http status {}", response.status()),
    })
}

fn read_secret(name: &str, env: Option<&str>, file: Option<&Path>) -> Result<String> {
    let value = match (env, file) {
        (Some(env), None) => std::env::var(env).map_err(|_| {
            Error::InvalidNotificationOption(format!("{name} env var is not set: {env}"))
        })?,
        (None, Some(file)) => fs::read_to_string(file).map_err(|source| Error::Io {
            path: file.to_owned(),
            source,
        })?,
        _ => {
            return Err(Error::InvalidNotificationOption(format!(
                "{name} must configure exactly one secret source"
            )))
        }
    };
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(Error::InvalidNotificationOption(format!(
            "{name} secret is empty"
        )));
    }
    Ok(value)
}

fn format_alarm(alarm: &Alarm<'_>) -> String {
    let mut lines = Vec::new();
    match alarm.title {
        Some(title) => lines.push(format!("[{}] {title}", alarm.severity.as_str())),
        None => lines.push(format!("[{}] ops-runbook alarm", alarm.severity.as_str())),
    }
    lines.push(format!("caller: {}", alarm.caller));
    lines.push(alarm.message.to_owned());
    lines.join("\n")
}

#[derive(Serialize)]
struct TelegramPayload<'a> {
    chat_id: &'a str,
    text: &'a str,
    disable_web_page_preview: bool,
}

#[derive(Serialize)]
struct DiscordPayload<'a> {
    content: &'a str,
}
