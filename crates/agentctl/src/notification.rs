use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const AGENTD_WIRE_PROTOCOL_VERSION: u32 = 1;
const MAX_MESSAGE_LEN: usize = 1600;

#[derive(Debug, Clone, Copy, clap::ValueEnum, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

pub struct Notification<'a> {
    pub caller: &'a str,
    pub provider: &'a str,
    pub profile: &'a str,
    pub chat_id: Option<&'a str>,
    pub severity: Severity,
    pub title: Option<&'a str>,
    pub message: &'a str,
}

#[derive(Debug, Serialize)]
struct AgentdNotifyRequest<'a> {
    version: u32,
    #[serde(rename = "type")]
    request_type: &'static str,
    caller: &'a str,
    provider: &'a str,
    profile: &'a str,
    chat_id: Option<&'a str>,
    severity: Severity,
    title: Option<&'a str>,
    message: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum AgentdNotifyResponse {
    Ok { version: u32 },
    Error { version: u32, error: String },
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

pub fn send_via_agentd(socket_path: &Path, notification: &Notification<'_>) -> Result<()> {
    let request = AgentdNotifyRequest {
        version: AGENTD_WIRE_PROTOCOL_VERSION,
        request_type: "notify",
        caller: notification.caller,
        provider: notification.provider,
        profile: notification.profile,
        chat_id: notification.chat_id,
        severity: notification.severity,
        title: notification.title,
        message: notification.message,
    };
    let mut stream = UnixStream::connect(socket_path).map_err(|source| Error::AgentdConnect {
        path: socket_path.to_owned(),
        source,
    })?;
    serde_json::to_writer(&mut stream, &request).map_err(|source| Error::AgentdProtocol {
        reason: source.to_string(),
    })?;
    stream
        .write_all(b"\n")
        .map_err(|source| Error::AgentdConnect {
            path: socket_path.to_owned(),
            source,
        })?;

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|source| Error::AgentdConnect {
            path: socket_path.to_owned(),
            source,
        })?;

    match serde_json::from_str::<AgentdNotifyResponse>(&response).map_err(|source| {
        Error::AgentdProtocol {
            reason: source.to_string(),
        }
    })? {
        AgentdNotifyResponse::Ok { version } => {
            validate_agentd_wire_version(version)?;
            Ok(())
        }
        AgentdNotifyResponse::Error { version, error } => {
            validate_agentd_wire_version(version)?;
            Err(Error::AgentdRejected(error))
        }
    }
}

fn validate_agentd_wire_version(version: u32) -> Result<()> {
    if version != AGENTD_WIRE_PROTOCOL_VERSION {
        return Err(Error::AgentdProtocol {
            reason: format!(
                "unsupported agentd wire protocol version {version}; expected {AGENTD_WIRE_PROTOCOL_VERSION}"
            ),
        });
    }
    Ok(())
}
