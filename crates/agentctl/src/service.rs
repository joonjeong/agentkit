use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const AGENTD_WIRE_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct AgentdServiceRequest<'a> {
    version: u32,
    #[serde(rename = "type")]
    request_type: &'static str,
    action: &'a str,
    service: &'a str,
    lines: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum AgentdServiceResponse {
    Ok {
        version: u32,
        exit_code: i32,
        stdout: String,
        stderr: String,
    },
    Error {
        version: u32,
        error: String,
    },
}

pub fn execute_via_agentd(
    socket_path: &Path,
    action: &str,
    target: &str,
    lines: Option<u32>,
) -> Result<i32> {
    let request = AgentdServiceRequest {
        version: AGENTD_WIRE_PROTOCOL_VERSION,
        request_type: "service",
        action,
        service: target,
        lines,
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

    match serde_json::from_str::<AgentdServiceResponse>(&response).map_err(|source| {
        Error::AgentdProtocol {
            reason: source.to_string(),
        }
    })? {
        AgentdServiceResponse::Ok {
            version,
            exit_code,
            stdout,
            stderr,
        } => {
            validate_agentd_wire_version(version)?;
            io::stdout()
                .write_all(stdout.as_bytes())
                .map_err(|source| Error::AgentdProtocol {
                    reason: source.to_string(),
                })?;
            io::stderr()
                .write_all(stderr.as_bytes())
                .map_err(|source| Error::AgentdProtocol {
                    reason: source.to_string(),
                })?;
            Ok(exit_code)
        }
        AgentdServiceResponse::Error { version, error } => {
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
