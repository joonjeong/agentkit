use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;

use crate::error::{Error, Result};
use crate::policy::{validate_caller, validate_target, Action};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/agentkit/agentctl.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub callers: HashMap<String, CallerPolicy>,
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

    pub fn backend(&self) -> Backend {
        self.defaults.backend.unwrap_or(Backend::Systemd)
    }
}

pub fn configured_config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("AGENTCTL_CONFIG_PATH") {
        return PathBuf::from(path);
    }

    PathBuf::from(DEFAULT_CONFIG_PATH)
}
