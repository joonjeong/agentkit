use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::AgentdConfigFile;
use crate::github;
use crate::notification::{self, Notification};
use crate::peer::{self, PeerIdentity};
use crate::protocol::{WireRequest, WireResponse};
use crate::service;

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
            Ok(stream) => {
                if once {
                    handle_stream(stream, &config)?;
                    break;
                }
                let config = config.clone();
                std::thread::spawn(move || {
                    if let Err(error) = handle_stream(stream, &config) {
                        eprintln!("agentd connection error: {error:#}");
                    }
                });
            }
            Err(error) => return Err(error).context("failed to accept agentd connection"),
        }
    }
    Ok(())
}

fn handle_stream(stream: UnixStream, config: &AgentdConfigFile) -> Result<()> {
    let peer = peer::identity(&stream)?;
    let mut writer = stream
        .try_clone()
        .context("failed to clone agentd client stream")?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .context("failed to read agentd request")?;

    let response = match serde_json::from_str::<WireRequest>(&line) {
        Ok(request) => match handle_request(request, config, &peer) {
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

fn handle_request(
    request: WireRequest,
    config: &AgentdConfigFile,
    peer: &PeerIdentity,
) -> Result<WireResponse> {
    match request {
        WireRequest::GithubAppToken {
            version,
            profile,
            repos,
            permissions,
        } => {
            WireRequest::validate_version(version)?;
            let token = github::installation_token(config, profile.as_deref(), repos, permissions)?;
            Ok(WireResponse::github_token(
                token.token,
                token.expires_at,
                token.api_url,
            ))
        }
        WireRequest::Notify {
            version,
            caller,
            provider,
            profile,
            chat_id,
            severity,
            title,
            message,
        } => {
            WireRequest::validate_version(version)?;
            let notification = Notification {
                caller: &caller,
                provider: &provider,
                profile: &profile,
                severity,
                title: title.as_deref(),
                message: &message,
            };
            match provider.as_str() {
                "telegram" => {
                    let telegram = config
                        .telegram
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("telegram is not configured"))?;
                    let configured_profile = telegram
                        .profiles
                        .get(&profile)
                        .ok_or_else(|| anyhow::anyhow!("telegram profile not found: {profile}"))?;
                    let chat_id = chat_id
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("telegram chat_id is required"))?;
                    notification::send_telegram(configured_profile, chat_id, &notification)?;
                }
                "discord" => {
                    let discord = config
                        .discord
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("discord is not configured"))?;
                    let configured_profile = discord
                        .profiles
                        .get(&profile)
                        .ok_or_else(|| anyhow::anyhow!("discord profile not found: {profile}"))?;
                    notification::send_discord(configured_profile, &notification)?;
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "unsupported notification provider: {provider}"
                    ))
                }
            }
            Ok(WireResponse::ok())
        }
        WireRequest::Service {
            version,
            action,
            service: target,
            lines,
        } => {
            WireRequest::validate_version(version)?;
            let service_config = config
                .service
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("service control is not configured"))?;
            let output = service::run(service_config, peer, &action, &target, lines)?;
            Ok(WireResponse::service(
                output.exit_code,
                output.stdout,
                output.stderr,
            ))
        }
    }
}
