use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::policy::{validate_caller, validate_target, Action};

pub const DEFAULT_POLICY_PATH: &str = "/etc/ops-runbook/policy.toml";

#[derive(Debug, Deserialize)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub callers: HashMap<String, CallerPolicy>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Defaults {
    pub max_log_lines: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CallerPolicy {
    #[serde(default)]
    pub service_restart: Vec<String>,
    #[serde(default)]
    pub service_reload: Vec<String>,
    #[serde(default)]
    pub service_status: Vec<String>,
    #[serde(default)]
    pub logs: Vec<String>,
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
                Action::ServiceRestart,
                Action::ServiceReload,
                Action::ServiceStatus,
                Action::Logs,
            ] {
                for target in crate::policy::allowed_targets(policy, action) {
                    validate_target(target)?;
                }
            }
        }

        Ok(())
    }

    pub fn max_log_lines(&self) -> u32 {
        self.defaults.max_log_lines.unwrap_or(1000)
    }
}

pub fn configured_policy_path() -> PathBuf {
    if cfg!(debug_assertions) && std::env::var_os("OPS_RUNBOOK_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os("OPS_RUNBOOK_POLICY_PATH") {
            return PathBuf::from(path);
        }
    }

    PathBuf::from(DEFAULT_POLICY_PATH)
}
