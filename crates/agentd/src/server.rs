use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::AgentdConfigFile;
use crate::github;
use crate::protocol::{WireRequest, WireResponse};

pub(crate) fn serve(config: AgentdConfigFile, socket_path: &Path, once: bool) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create socket directory {}", parent.display()))?;
    }
    if socket_path.exists() {
        fs::remove_file(socket_path)
            .with_context(|| format!("failed to remove stale socket {}", socket_path.display()))?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind {}", socket_path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(socket_path, fs::Permissions::from_mode(0o660))
            .with_context(|| format!("failed to set permissions on {}", socket_path.display()))?;
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_stream(stream, &config)?,
            Err(error) => return Err(error).context("failed to accept agentd connection"),
        }
        if once {
            break;
        }
    }
    Ok(())
}

fn handle_stream(stream: UnixStream, config: &AgentdConfigFile) -> Result<()> {
    let mut writer = stream
        .try_clone()
        .context("failed to clone agentd client stream")?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("failed to read agentd request")?;

    let response = match serde_json::from_str::<WireRequest>(&line) {
        Ok(request) => match handle_request(request, config) {
            Ok(response) => response,
            Err(error) => WireResponse::error(format!("{error:#}")),
        },
        Err(error) => WireResponse::error(format!("invalid request: {error}")),
    };

    serde_json::to_writer(&mut writer, &response).context("failed to write agentd response")?;
    writer
        .write_all(b"\n")
        .context("failed to finish agentd response")?;
    Ok(())
}

fn handle_request(request: WireRequest, config: &AgentdConfigFile) -> Result<WireResponse> {
    match request {
        WireRequest::GithubAppToken {
            version,
            profile,
            repos,
            permissions,
        } => {
            WireRequest::validate_version(version)?;
            let token = github::installation_token(config, profile.as_deref(), repos, permissions)?;
            Ok(WireResponse::ok(
                token.token,
                token.expires_at,
                token.api_url,
            ))
        }
    }
}
