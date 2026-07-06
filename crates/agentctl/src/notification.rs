use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::Serialize;

use crate::config::{DiscordChannel, NotificationChannel, TelegramChannel};
use crate::error::{Error, Result};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGE_LEN: usize = 1600;
const MAX_ERROR_BODY_LEN: usize = 512;

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

pub struct Notification<'a> {
    pub caller: &'a str,
    pub channel: &'a str,
    pub severity: Severity,
    pub title: Option<&'a str>,
    pub message: &'a str,
}

pub fn validate_message_text(title: Option<&str>, message: &str) -> Result<()> {
    if message.trim().is_empty() {
        return Err(Error::InvalidNotificationOption(
            "notify message must not be empty".to_owned(),
        ));
    }
    if message.len() > MAX_MESSAGE_LEN {
        return Err(Error::InvalidNotificationOption(format!(
            "notify message must be at most {MAX_MESSAGE_LEN} bytes"
        )));
    }
    if let Some(title) = title {
        if title.trim().is_empty() {
            return Err(Error::InvalidNotificationOption(
                "notify title must not be empty".to_owned(),
            ));
        }
        if title.len() > 160 {
            return Err(Error::InvalidNotificationOption(
                "notify title must be at most 160 bytes".to_owned(),
            ));
        }
    }

    Ok(())
}

pub fn send(channel: &NotificationChannel, notification: &Notification<'_>) -> Result<()> {
    let client = Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .map_err(|source| Error::NotificationSend {
            destination: notification.channel.to_owned(),
            reason: source.to_string(),
        })?;

    match channel {
        NotificationChannel::Telegram(channel) => send_telegram(&client, channel, notification),
        NotificationChannel::Discord(channel) => send_discord(&client, channel, notification),
    }
}

fn send_telegram(
    client: &Client,
    channel: &TelegramChannel,
    notification: &Notification<'_>,
) -> Result<()> {
    let token = read_secret(
        "telegram bot token",
        channel.bot_token_env.as_deref(),
        channel.bot_token_file.as_deref(),
    )?;
    let base_url =
        if cfg!(debug_assertions) && std::env::var_os("AGENTCTL_TEST_OVERRIDES").is_some() {
            std::env::var("AGENTCTL_TELEGRAM_API_BASE")
                .unwrap_or_else(|_| "https://api.telegram.org".to_owned())
        } else {
            "https://api.telegram.org".to_owned()
        };
    let url = format!("{}/bot{token}/sendMessage", base_url.trim_end_matches('/'));
    let payload = TelegramPayload {
        chat_id: &channel.chat_id,
        text: &format_notification(notification),
        disable_web_page_preview: true,
    };

    post_json(client, notification.channel, &url, &payload)
}

fn send_discord(
    client: &Client,
    channel: &DiscordChannel,
    notification: &Notification<'_>,
) -> Result<()> {
    let url = read_secret(
        "discord webhook url",
        channel.webhook_url_env.as_deref(),
        channel.webhook_url_file.as_deref(),
    )?;
    let payload = DiscordPayload {
        content: &format_notification(notification),
    };

    post_json(client, notification.channel, &url, &payload)
}

fn post_json<T>(client: &Client, channel: &str, url: &str, payload: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    if cfg!(debug_assertions) && std::env::var_os("AGENTCTL_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os("AGENTCTL_NOTIFICATION_RECORD_PATH") {
            let path = PathBuf::from(path);
            let payload =
                serde_json::to_string(payload).map_err(|source| Error::NotificationSend {
                    destination: channel.to_owned(),
                    reason: source.to_string(),
                })?;
            fs::write(&path, format!("channel={channel}\nurl={url}\n{payload}\n"))
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
                destination: channel.to_owned(),
                reason: source.to_string(),
            })?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }

    let body = response
        .text()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(MAX_ERROR_BODY_LEN)
        .collect::<String>();
    let reason = http_error_reason(status, &body);

    Err(Error::NotificationSend {
        destination: channel.to_owned(),
        reason,
    })
}

fn http_error_reason(status: StatusCode, body: &str) -> String {
    let body = body
        .trim()
        .chars()
        .take(MAX_ERROR_BODY_LEN)
        .collect::<String>();
    if body.is_empty() {
        format!("http status {status}")
    } else {
        format!("http status {status}: {body}")
    }
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

fn format_notification(notification: &Notification<'_>) -> String {
    let mut lines = Vec::new();
    match notification.title {
        Some(title) => lines.push(format!("[{}] {title}", notification.severity.as_str())),
        None => lines.push(format!(
            "[{}] agentctl notify",
            notification.severity.as_str()
        )),
    }
    lines.push(format!("caller: {}", notification.caller));
    lines.push(notification.message.to_owned());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_error_reason_includes_response_body() {
        assert_eq!(
            http_error_reason(StatusCode::BAD_REQUEST, "invalid chat id"),
            "http status 400 Bad Request: invalid chat id"
        );
    }

    #[test]
    fn http_error_reason_truncates_long_response_body() {
        let reason = http_error_reason(StatusCode::TOO_MANY_REQUESTS, &"x".repeat(600));

        assert_eq!(
            reason,
            format!("http status 429 Too Many Requests: {}", "x".repeat(512))
        );
    }
}
