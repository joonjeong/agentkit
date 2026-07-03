use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::error::{Error, Result};

pub const DEFAULT_AUDIT_LOG_PATH: &str = "/var/log/ops-runbook/audit.log";

pub fn configured_audit_log_path() -> PathBuf {
    if cfg!(debug_assertions) && std::env::var_os("OPS_RUNBOOK_TEST_OVERRIDES").is_some() {
        if let Some(path) = std::env::var_os("OPS_RUNBOOK_AUDIT_LOG") {
            return PathBuf::from(path);
        }
    }

    PathBuf::from(DEFAULT_AUDIT_LOG_PATH)
}

pub fn write(
    path: &Path,
    caller: &str,
    action: &str,
    target: &str,
    result: &str,
    reason: Option<&str>,
) -> Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o640);
    }

    let mut file = options.open(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;

    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_owned());

    match reason {
        Some(reason) => writeln!(
            file,
            "{timestamp} caller={caller} action={action} target={target} result={result} reason={reason}"
        ),
        None => writeln!(
            file,
            "{timestamp} caller={caller} action={action} target={target} result={result}"
        ),
    }
    .map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}
