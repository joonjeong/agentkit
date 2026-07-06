use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum WireResponse {
    Ok {
        version: u32,
        token: String,
        expires_at: Option<String>,
        api_url: String,
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
    pub(crate) fn ok(token: String, expires_at: Option<String>, api_url: String) -> Self {
        Self::Ok {
            version: WIRE_PROTOCOL_VERSION,
            token,
            expires_at,
            api_url,
        }
    }

    pub(crate) fn error(error: impl Into<String>) -> Self {
        Self::Error {
            version: WIRE_PROTOCOL_VERSION,
            error: error.into(),
        }
    }
}
