use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn shows_top_level_help() {
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("github-app")
            .and(predicate::str::contains("agent-session"))
            .and(predicate::str::contains("agent-skill"))
            .and(predicate::str::contains(
                "agent-session github-app run [OPTIONS]",
            )),
    );
}

#[test]
fn shows_version() {
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");
    let assert = cmd.arg("--version").assert().success();

    if let Some(agent_session_version) = option_env!("AGENT_SESSION_VERSION") {
        assert.stdout(
            predicate::str::contains(env!("CARGO_PKG_VERSION"))
                .or(predicate::str::contains(agent_session_version)),
        );
    } else {
        assert.stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn shows_agent_session_agent_usage() {
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("Run a command in an authenticated operations session")
            .and(predicate::str::contains(
                "agent-session github-app run [OPTIONS]",
            ))
            .and(predicate::str::contains("agent-skill")),
    );

    let mut github = Command::cargo_bin("agent-session").expect("binary exists");
    github
        .args(["github-app", "run", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Request a GitHub App installation token from agentd")
                .and(predicate::str::contains("agent-session"))
                .and(predicate::str::contains("GH_TOKEN"))
                .and(predicate::str::contains("GITHUB_TOKEN"))
                .and(predicate::str::contains("--git-credentials"))
                .and(predicate::str::contains("-- <COMMAND>"))
                .and(predicate::str::contains("--repo <OWNER/REPO>")),
        );
}

#[cfg(unix)]
#[test]
fn agent_session_symlink_style_help_does_not_duplicate_subcommand_name() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("agent-session"));
    cmd.arg0("agent-session");

    let output = cmd.arg("--help").output().expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("Usage: agent-session <COMMAND>"));
    assert!(!stdout.contains("agent-session agent-session [OPTIONS]"));
}

#[cfg(unix)]
#[test]
fn agent_session_runs_command_with_installation_token_environment() {
    let (socket_dir, socket_path, server) =
        agentd_token_response_server("test-token", "https://api.github.com");
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--agentd-socket",
        socket_path.to_str().expect("utf-8 socket path"),
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token && \
         test \"$GITHUB_TOKEN\" = test-token && \
         test -z \"${GITHUB_APP_ID+x}\" && \
         test -z \"${GITHUB_APP_INSTALLATION_ID+x}\" && \
         test \"$1\" = --body && \
         test \"$2\" = Done",
        "child-command",
        "--body",
        "Done",
    ])
    .env("GITHUB_APP_ID", "1")
    .env("GITHUB_APP_INSTALLATION_ID", "42")
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.contains(r#""type":"github_app_token""#));
    assert!(request.contains(r#""version":1"#));
    fs::remove_dir_all(socket_dir).expect("socket dir removed");
}

#[cfg(unix)]
#[test]
fn agent_session_can_configure_child_only_git_credentials() {
    let api_url = "http://127.0.0.1:1";
    let (socket_dir, socket_path, server) = agentd_token_response_server("test-token", api_url);
    let output_dir = unique_temp_dir("agent-session-git-credentials-test");
    fs::create_dir(&output_dir).expect("temporary output directory created");
    let output_file = output_dir.join("credentials");
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--agentd-socket",
        socket_path.to_str().expect("utf-8 socket path"),
        "--git-credentials",
        "--",
        "sh",
        "-c",
        "test \"$GIT_TERMINAL_PROMPT\" = 0 && \
         test \"$GIT_CONFIG_COUNT\" = 2 && \
         test \"$GIT_CONFIG_KEY_0\" = credential.helper && \
         test \"$GIT_CONFIG_VALUE_0\" = \"\" && \
         test \"$GIT_CONFIG_KEY_1\" = credential.helper && \
         helper=$GIT_CONFIG_VALUE_1 && \
         api_host=${2#http://} && \
         api_host=${api_host%%:*} && \
         printf 'protocol=https\\nhost=%s\\n\\n' \"$api_host\" | \
           GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_GLOBAL=/dev/null git credential fill > \"$1\" && \
         grep -qx username=x-access-token \"$1\" && \
         grep -qx password=test-token \"$1\" && \
         printf 'protocol=https\\nhost=%s:8443\\n\\n' \"$api_host\" | \"$helper\" get > \"$1\" && \
         grep -qx username=x-access-token \"$1\" && \
         grep -qx password=test-token \"$1\" && \
         test -z \"$(printf 'protocol=https\\nhost=example.com\\n\\n' | \"$helper\" get)\" && \
         test -z \"$(printf 'protocol=http\\nhost=%s\\n\\n' \"$api_host\" | \"$helper\" get)\"",
        "child-command",
        output_file.to_str().expect("utf-8 path"),
        api_url,
    ])
    .env_remove("GIT_CONFIG_COUNT")
    .env_remove("GIT_CONFIG_KEY_0")
    .env_remove("GIT_CONFIG_VALUE_0")
    .env_remove("GIT_CONFIG_KEY_1")
    .env_remove("GIT_CONFIG_VALUE_1")
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.contains(r#""type":"github_app_token""#));
    assert!(request.contains(r#""version":1"#));
    fs::remove_dir_all(output_dir).expect("temporary output directory removed");
    fs::remove_dir_all(socket_dir).expect("socket dir removed");
}

#[cfg(unix)]
#[test]
fn agent_session_exits_with_child_exit_code() {
    let (socket_dir, socket_path, server) =
        agentd_token_response_server("test-token", "https://api.github.com");
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--agentd-socket",
        socket_path.to_str().expect("utf-8 socket path"),
        "--",
        "sh",
        "-c",
        "exit 42",
    ])
    .assert()
    .code(42);

    let request = server.join().expect("server thread completed");
    assert!(request.contains(r#""type":"github_app_token""#));
    assert!(request.contains(r#""version":1"#));
    fs::remove_dir_all(socket_dir).expect("socket dir removed");
}

#[cfg(unix)]
#[test]
fn agent_session_uses_named_config_profile() {
    let (socket_dir, socket_path, server) =
        agentd_token_response_server("test-token", "https://api.github.com");
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--agentd-socket",
        socket_path.to_str().expect("utf-8 socket path"),
        "--profile",
        "write",
        "--repo",
        "OWNER/OTHER",
        "--permission",
        "contents=write",
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token",
    ])
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.contains(r#""profile":"write""#));
    assert!(request.contains(r#""repos":["OWNER/OTHER"]"#));
    assert!(request.contains(r#""permissions":{"contents":"write"}"#));
    fs::remove_dir_all(socket_dir).expect("socket dir removed");
}

#[cfg(unix)]
#[test]
fn agent_session_reports_agentd_error() {
    let (socket_dir, socket_path, server) = agentd_error_response_server("repo is not allowed");
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--agentd-socket",
        socket_path.to_str().expect("utf-8 socket path"),
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "agentd rejected GitHub App token request",
    ));

    let _ = server.join().expect("server thread completed");
    fs::remove_dir_all(socket_dir).expect("socket dir removed");
}

#[cfg(unix)]
#[test]
fn agent_session_exits_with_child_signal_status() {
    let (socket_dir, socket_path, server) =
        agentd_token_response_server("test-token", "https://api.github.com");
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--agentd-socket",
        socket_path.to_str().expect("utf-8 socket path"),
        "--",
        "sh",
        "-c",
        "kill -TERM $$",
    ])
    .assert()
    .code(143);

    let request = server.join().expect("server thread completed");
    assert!(request.contains(r#""type":"github_app_token""#));
    assert!(request.contains(r#""version":1"#));
    fs::remove_dir_all(socket_dir).expect("socket dir removed");
}

#[test]
fn agent_session_requires_command_after_separator() {
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

    cmd.args(["github-app", "run", "--repo", "OWNER/REPO"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<COMMAND>"));
}

#[test]
fn agent_session_accepts_command_options_after_separator() {
    #[cfg(unix)]
    {
        let (socket_dir, socket_path, server) =
            agentd_token_response_server("test-token", "https://api.github.com");
        let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");

        cmd.args([
            "github-app",
            "run",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
            "--repo",
            "OWNER/REPO",
            "--",
            "sh",
            "-c",
            "test \"$1\" = --body && test \"$2\" = Done",
            "child-command",
            "--body",
            "Done",
        ])
        .assert()
        .success();

        let request = server.join().expect("server thread completed");
        assert!(request.contains(r#""repos":["OWNER/REPO"]"#));
        assert!(request.contains(r#""version":1"#));
        fs::remove_dir_all(socket_dir).expect("socket dir removed");
    }

    #[cfg(not(unix))]
    {
        let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");
        cmd.args([
            "github-app",
            "run",
            "--repo",
            "OWNER/REPO",
            "--",
            "gh",
            "pr",
            "comment",
            "123",
            "--body",
            "Done",
        ])
        .assert()
        .failure();
    }
}

#[test]
fn creates_github_app_agent_workflow_skill() {
    let mut cmd = Command::cargo_bin("agent-session").expect("binary exists");
    let skills_dir = unique_temp_dir("agent-session-skill-test");

    cmd.args([
        "agent-skill",
        "--install-path",
        skills_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("agent-session-workflow"));

    let skill_file = skills_dir.join("agent-session-workflow").join("SKILL.md");
    let skill = fs::read_to_string(&skill_file).expect("skill file exists");
    assert!(skill.contains("name: agent-session-workflow"));
    assert!(skill.contains("agent-session github-app run"));
    assert!(skill.contains("agentd config check"));
    assert!(skill.contains("provider-scoped session runner"));
    assert!(skill.contains("/etc/agentd/config.toml"));
    assert!(skill.contains("AGENT_SESSION_GITHUB_PROFILE"));

    fs::remove_dir_all(skills_dir).expect("temporary skill directory removed");
}

#[test]
fn refuses_to_overwrite_existing_skill_without_force() {
    let skills_dir = unique_temp_dir("agent-session-skill-test");

    let mut create = Command::cargo_bin("agent-session").expect("binary exists");
    create
        .args([
            "agent-skill",
            "-i",
            skills_dir.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();

    assert!(skills_dir
        .join("agent-session-workflow")
        .join("SKILL.md")
        .exists());

    let mut overwrite = Command::cargo_bin("agent-session").expect("binary exists");
    overwrite
        .args([
            "agent-skill",
            "-i",
            skills_dir.to_str().expect("utf-8 path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));

    fs::remove_dir_all(skills_dir).expect("temporary skill directory removed");
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::path::PathBuf::from("/tmp").join(format!(
        "{prefix}-{}-{unique}-{counter}",
        std::process::id()
    ))
}

#[cfg(unix)]
fn agentd_token_response_server(
    token: &'static str,
    api_url: &'static str,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    thread::JoinHandle<String>,
) {
    agentd_response_server(&format!(
        r#"{{"status":"ok","version":1,"token":"{token}","expires_at":"2026-06-15T00:00:00Z","api_url":"{api_url}"}}"#
    ))
}

#[cfg(unix)]
fn agentd_error_response_server(
    error: &'static str,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    thread::JoinHandle<String>,
) {
    agentd_response_server(&format!(
        r#"{{"status":"error","version":1,"error":"{error}"}}"#
    ))
}

#[cfg(unix)]
fn agentd_response_server(
    response_body: &str,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    thread::JoinHandle<String>,
) {
    let socket_dir = unique_temp_dir("agent-session-agentd-test");
    fs::create_dir(&socket_dir).expect("socket dir created");
    let socket_path = socket_dir.join("agentd.sock");
    let listener = UnixListener::bind(&socket_path).expect("agentd test socket binds");
    let response_body = response_body.to_string();
    let socket_path_for_return = socket_path.clone();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("agentd test server accepts");
        let mut reader = BufReader::new(stream.try_clone().expect("stream clones"));
        let mut request = String::new();
        reader
            .read_line(&mut request)
            .expect("agentd test server reads request");
        stream
            .write_all(response_body.as_bytes())
            .expect("agentd test server writes response");
        stream
            .write_all(b"\n")
            .expect("agentd test server writes newline");
        request
    });

    (socket_dir, socket_path_for_return, handle)
}
