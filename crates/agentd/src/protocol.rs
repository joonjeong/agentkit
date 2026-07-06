use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::notification::Severity;

pub(crate) const WIRE_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum WireRequest {
    GithubAppToken {
        version: u32,
        profile: Option<String>,
        #[serde(default)]
        repos: Vec<String>,
        #[serde(default)]
        permissions: BTreeMap<String, String>,
    },
    Notify {
        version: u32,
        caller: String,
        channel: String,
        severity: Severity,
        title: Option<String>,
        message: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum WireResponse {
    Ok {
        version: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expires_at: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        api_url: Option<String>,
    },
    Error {
        version: u32,
        error: String,
    },
}

impl WireRequest {
    pub(crate) fn validate_version(version: u32) -> Result<()> {
        if version != WIRE_PROTOCOL_VERSION {
            return Err(anyhow!(
                "unsupported agentd wire protocol version {}; expected {}",
                version,
                WIRE_PROTOCOL_VERSION
            ));
        }
        Ok(())
    }
}

impl WireResponse {
    pub(crate) fn github_token(token: String, expires_at: Option<String>, api_url: String) -> Self {
        Self::Ok {
            version: WIRE_PROTOCOL_VERSION,
            token: Some(token),
            expires_at,
            api_url: Some(api_url),
        }
    }

    pub(crate) fn ok() -> Self {
        Self::Ok {
            version: WIRE_PROTOCOL_VERSION,
            token: None,
            expires_at: None,
            api_url: None,
        }
    }

    pub(crate) fn error(error: impl Into<String>) -> Self {
        Self::Error {
            version: WIRE_PROTOCOL_VERSION,
            error: error.into(),
        }
    }
}
