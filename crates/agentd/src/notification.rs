use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGE_LEN: usize = 1600;
const MAX_ERROR_BODY_LEN: usize = 512;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    Info,
    Warning,
    Critical,
}

impl Severity {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TelegramProfile {
    pub(crate) bot_token_env: Option<String>,
    pub(crate) bot_token_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DiscordProfile {
    pub(crate) webhook_url_env: Option<String>,
    pub(crate) webhook_url_file: Option<PathBuf>,
}

pub(crate) struct Notification<'a> {
    pub(crate) caller: &'a str,
    pub(crate) profile: &'a str,
    pub(crate) severity: Severity,
    pub(crate) title: Option<&'a str>,
    pub(crate) message: &'a str,
}

pub(crate) fn validate_telegram_profile(name: &str, profile: &TelegramProfile) -> Result<()> {
    validate_secret_ref(
        &format!("telegram.profiles.{name}.bot_token"),
        profile.bot_token_env.as_deref(),
        profile.bot_token_file.as_deref(),
    )
}

pub(crate) fn validate_discord_profile(name: &str, profile: &DiscordProfile) -> Result<()> {
    validate_secret_ref(
        &format!("discord.profiles.{name}.webhook_url"),
        profile.webhook_url_env.as_deref(),
        profile.webhook_url_file.as_deref(),
    )
}

pub(crate) fn validate_message_text(title: Option<&str>, message: &str) -> Result<()> {
    if message.trim().is_empty() {
        return Err(anyhow!("notify message must not be empty"));
    }
    if message.len() > MAX_MESSAGE_LEN {
        return Err(anyhow!(
            "notify message must be at most {MAX_MESSAGE_LEN} bytes"
        ));
    }
    if let Some(title) = title {
        if title.trim().is_empty() {
            return Err(anyhow!("notify title must not be empty"));
        }
        if title.len() > 160 {
            return Err(anyhow!("notify title must be at most 160 bytes"));
        }
    }

    Ok(())
}

pub(crate) fn send_telegram(
    profile: &TelegramProfile,
    chat_id: &str,
    notification: &Notification<'_>,
) -> Result<()> {
    validate_message_text(notification.title, notification.message)?;
    if chat_id.trim().is_empty() {
        return Err(anyhow!("telegram chat_id must not be empty"));
    }
    let client = Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .with_context(|| {
            format!(
                "failed to create notification client for {}",
                notification.profile
            )
        })?;
    let token = read_secret(
        "telegram bot token",
        profile.bot_token_env.as_deref(),
        profile.bot_token_file.as_deref(),
    )?;
    let base_url = if cfg!(debug_assertions) && std::env::var_os("AGENTD_TEST_OVERRIDES").is_some()
    {
        std::env::var("AGENTD_TELEGRAM_API_BASE")
            .unwrap_or_else(|_| "https://api.telegram.org".to_owned())
    } else {
        "https://api.telegram.org".to_owned()
    };
    let url = format!("{}/bot{token}/sendMessage", base_url.trim_end_matches('/'));
    let payload = TelegramPayload {
        chat_id,
        text: &format_notification(notification),
        disable_web_page_preview: true,
    };

    post_json(&client, notification.profile, &url, &payload)
}

pub(crate) fn send_discord(
    profile: &DiscordProfile,
    notification: &Notification<'_>,
) -> Result<()> {
    validate_message_text(notification.title, notification.message)?;
    let client = Client::builder()
        .timeout(DEFAULT_TIMEOUT)
        .build()
        .with_context(|| {
            format!(
                "failed to create notification client for {}",
                notification.profile
            )
        })?;
    let url = read_secret(
        "discord webhook url",
        profile.webhook_url_env.as_deref(),
        profile.webhook_url_file.as_deref(),
    )?;
    let payload = DiscordPayload {
        content: &format_notification(notification),
    };

    post_json(&client, notification.profile, &url, &payload)
}

fn post_json<T>(client: &Client, channel: &str, url: &str, payload: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    if cfg!(debug_assertions) && std::env::var_os("AGENTD_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os("AGENTD_NOTIFICATION_RECORD_PATH") {
            let path = PathBuf::from(path);
            let payload = serde_json::to_string(payload)
                .context("failed to serialize notification payload")?;
            fs::write(&path, format!("channel={channel}\nurl={url}\n{payload}\n"))
                .with_context(|| format!("failed to write {}", path.display()))?;
            return Ok(());
        }
    }

    let response = client
        .post(url)
        .json(payload)
        .send()
        .with_context(|| format!("failed to send notification to {channel}"))?;
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

    Err(anyhow!(
        "notification send failed for {channel}: {}",
        http_error_reason(status, &body)
    ))
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
        (Some(env), None) => {
            std::env::var(env).with_context(|| format!("{name} env var is not set: {env}"))?
        }
        (None, Some(file)) => fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?,
        _ => return Err(anyhow!("{name} must configure exactly one secret source")),
    };
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(anyhow!("{name} secret is empty"));
    }
    Ok(value)
}

fn validate_secret_ref(name: &str, env: Option<&str>, file: Option<&Path>) -> Result<()> {
    match (env, file) {
        (Some(env), None) if !env.trim().is_empty() => Ok(()),
        (None, Some(file)) if file.is_absolute() => Ok(()),
        (Some(_), Some(_)) => Err(anyhow!("{name} must use either env or file, not both")),
        (None, Some(file)) => Err(anyhow!("{name}_file must be absolute: {}", file.display())),
        _ => Err(anyhow!("{name} must configure env or file")),
    }
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
