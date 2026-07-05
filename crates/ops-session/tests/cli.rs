use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(unix)]
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn shows_top_level_help() {
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("github-app")
            .and(predicate::str::contains("config"))
            .and(predicate::str::contains("ops-session"))
            .and(predicate::str::contains("agent-skill"))
            .and(predicate::str::contains(
                "ops-session github-app run [OPTIONS]",
            )),
    );
}

#[test]
fn shows_version() {
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let assert = cmd.arg("--version").assert().success();

    if let Some(ops_session_version) = option_env!("OPS_SESSION_VERSION") {
        assert.stdout(
            predicate::str::contains(env!("CARGO_PKG_VERSION"))
                .or(predicate::str::contains(ops_session_version)),
        );
    } else {
        assert.stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn shows_ops_session_agent_usage() {
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("Run a command in an authenticated operations session")
            .and(predicate::str::contains(
                "ops-session github-app run [OPTIONS]",
            ))
            .and(predicate::str::contains("agent-skill")),
    );

    let mut github = Command::cargo_bin("ops-session").expect("binary exists");
    github
        .args(["github-app", "run", "--help"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Sign a GitHub App JWT")
                .and(predicate::str::contains("ops-session"))
                .and(predicate::str::contains("GH_TOKEN"))
                .and(predicate::str::contains("GITHUB_TOKEN"))
                .and(predicate::str::contains("--git-credentials"))
                .and(predicate::str::contains("-- <COMMAND>"))
                .and(predicate::str::contains("--repo <OWNER/REPO>")),
        );
}

#[cfg(unix)]
#[test]
fn ops_session_symlink_style_help_does_not_duplicate_subcommand_name() {
    let mut cmd = std::process::Command::new(assert_cmd::cargo::cargo_bin("ops-session"));
    cmd.arg0("ops-session");

    let output = cmd.arg("--help").output().expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout is utf-8");
    assert!(stdout.contains("Usage: ops-session <COMMAND>"));
    assert!(!stdout.contains("ops-session ops-session [OPTIONS]"));
}

#[test]
fn github_app_config_check_reads_default_user_config_path() {
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let config_home = unique_temp_dir("ops-session-config-home-test");
    let ops_session_config_dir = config_home.join("ops-session");
    fs::create_dir_all(&ops_session_config_dir).expect("config dir created");
    let private_key_path = ops_session_config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    fs::write(
        ops_session_config_dir.join("config.toml"),
        format!(
            "[github_app]\napp_id = 1\nprivate_key_path = \"{}\"\ndefault_profile = \"default\"\n\n[github_app.profiles.default]\nrepos = [\"OWNER/REPO\"]\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");

    cmd.args(["github-app", "config", "check"])
        .env("XDG_CONFIG_HOME", &config_home)
        .env_remove("OPS_SESSION_GITHUB_CONFIG_PATH")
        .assert()
        .success()
        .stdout(predicate::str::contains("config OK"));

    fs::remove_dir_all(config_home).expect("config dir removed");
}

#[test]
fn github_app_config_example_prints_user_level_config() {
    let config_home = unique_temp_dir("ops-session-config-example-test");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args(["github-app", "config", "example"])
        .env("XDG_CONFIG_HOME", &config_home)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("app_id = 123456")
                .and(predicate::str::contains("private_key_path = "))
                .and(predicate::str::contains(
                    "/home/me/.config/ops-session/github-app.private-key.pem",
                ))
                .and(predicate::str::contains(
                    "[github_app.profiles.codex-review.permissions]",
                )),
        );

    fs::remove_dir_all(config_home).ok();
}

#[test]
fn github_app_config_example_refuses_to_overwrite_without_force() {
    let output_dir = unique_temp_dir("ops-session-config-example-test");
    fs::create_dir(&output_dir).expect("output dir created");
    let output = output_dir.join("config.toml");
    fs::write(&output, "existing").expect("output written");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args([
        "github-app",
        "config",
        "example",
        "--output",
        output.to_str().expect("utf-8 path"),
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("output already exists"));

    fs::remove_dir_all(output_dir).expect("output dir removed");
}

#[test]
fn github_app_config_check_accepts_valid_config() {
    let config_dir = unique_temp_dir("ops-session-config-check-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\napp_id = 1\nprivate_key_path = \"{}\"\ndefault_profile = \"read\"\n\n[github_app.profiles.read]\nrepos = [\"OWNER/REPO\"]\n\n[github_app.profiles.read.permissions]\ncontents = \"read\"\n\n[github_app.profiles.write]\nrepos = [\"OWNER/OTHER\"]\n\n[github_app.profiles.write.permissions]\ncontents = \"write\"\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args(["github-app", "config", "check", "--config-path"])
        .arg(&config_path)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("config OK")
                .and(predicate::str::contains("app_id: 1"))
                .and(predicate::str::contains("default_profile: read"))
                .and(predicate::str::contains("  read:"))
                .and(predicate::str::contains("  write:")),
        );

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn github_app_config_check_allows_other_provider_sections() {
    let config_dir = unique_temp_dir("ops-session-config-check-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\napp_id = 1\nprivate_key_path = \"{}\"\ndefault_profile = \"read\"\n\n[github_app.profiles.read]\nrepos = [\"OWNER/REPO\"]\n\n[other_provider]\nenabled = true\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args(["github-app", "config", "check", "--config-path"])
        .arg(&config_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("config OK"));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn github_app_config_check_rejects_invalid_repo_scope() {
    let config_dir = unique_temp_dir("ops-session-config-check-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\napp_id = 1\nprivate_key_path = \"{}\"\n\n[github_app.profiles.bad]\nrepos = [\"OWNER/REPO/EXTRA\"]\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args(["github-app", "config", "check", "--config-path"])
        .arg(&config_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid repo"));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_runs_command_with_installation_token_environment() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--api-url",
        &api_url,
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
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    assert!(request.contains("authorization: bearer "));
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_can_configure_child_only_git_credentials() {
    let (api_url, server) = one_token_response_server();
    let output_dir = unique_temp_dir("ops-session-git-credentials-test");
    fs::create_dir(&output_dir).expect("temporary output directory created");
    let output_file = output_dir.join("credentials");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--api-url",
        &api_url,
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
        &api_url,
    ])
    .env("GITHUB_APP_ID", "1")
    .env("GITHUB_APP_INSTALLATION_ID", "42")
    .env_remove("GIT_CONFIG_COUNT")
    .env_remove("GIT_CONFIG_KEY_0")
    .env_remove("GIT_CONFIG_VALUE_0")
    .env_remove("GIT_CONFIG_KEY_1")
    .env_remove("GIT_CONFIG_VALUE_1")
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    fs::remove_dir_all(output_dir).expect("temporary output directory removed");
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_exits_with_child_exit_code() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--",
        "sh",
        "-c",
        "exit 42",
    ])
    .assert()
    .code(42);

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_uses_named_config_profile() {
    let (api_url, server) = one_token_response_server();
    let config_dir = unique_temp_dir("ops-session-config-profile-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\napp_id = 1\ninstallation_id = 42\nprivate_key_path = \"{}\"\n\n[github_app.profiles.read]\nrepos = [\"OWNER/REPO\"]\n\n[github_app.profiles.read.permissions]\ncontents = \"read\"\n\n[github_app.profiles.write]\nrepos = [\"OWNER/OTHER\"]\n\n[github_app.profiles.write.permissions]\ncontents = \"write\"\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--profile",
        "write",
        "--api-url",
        &api_url,
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token",
    ])
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.contains(r#""repositories":["other"]"#));
    assert!(request.contains(r#""permissions":{"contents":"write"}"#));
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_reuses_valid_cached_installation_token() {
    let cache_dir = unique_temp_dir("ops-session-token-cache-test");
    let (api_url, server) = token_cache_response_server();
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    let mut first = Command::cargo_bin("ops-session").expect("binary exists");
    first
        .args([
            "github-app",
            "run",
            "--config-path",
            config_path.to_str().expect("utf-8 config path"),
            "--token-cache",
            "--app-id",
            "1",
            "--installation-id",
            "42",
            "--api-url",
            &api_url,
            "--",
            "sh",
            "-c",
            "test \"$GH_TOKEN\" = cached-token",
        ])
        .env("XDG_CACHE_HOME", &cache_dir)
        .assert()
        .success();

    let mut second = Command::cargo_bin("ops-session").expect("binary exists");
    second
        .args([
            "github-app",
            "run",
            "--config-path",
            config_path.to_str().expect("utf-8 config path"),
            "--token-cache",
            "--app-id",
            "1",
            "--installation-id",
            "42",
            "--api-url",
            &api_url,
            "--",
            "sh",
            "-c",
            "test \"$GH_TOKEN\" = cached-token",
        ])
        .env("XDG_CACHE_HOME", &cache_dir)
        .assert()
        .success();

    let requests = server.join().expect("server thread completed");
    assert_eq!(requests.len(), 2);
    assert!(requests[0].starts_with("post /app/installations/42/access_tokens "));
    assert!(requests[1].starts_with("get /installation/repositories "));

    fs::remove_dir_all(cache_dir).expect("temporary cache directory removed");
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_does_not_write_token_cache_by_default() {
    let cache_dir = unique_temp_dir("ops-session-token-cache-test");
    let (api_url, server) = one_token_response_server();
    let (config_dir, config_path) = write_github_config("ops-session-config-test");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token",
    ])
    .env("XDG_CACHE_HOME", &cache_dir)
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    assert!(!cache_dir.exists());

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_mints_token_when_cache_directory_is_unavailable() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--",
        "sh",
        "-c",
        "test \"$GH_TOKEN\" = test-token",
    ])
    .env_remove("XDG_CACHE_HOME")
    .env_remove("HOME")
    .assert()
    .success();

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[cfg(unix)]
#[test]
fn ops_session_exits_with_child_signal_status() {
    let (api_url, server) = one_token_response_server();
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--app-id",
        "1",
        "--installation-id",
        "42",
        "--api-url",
        &api_url,
        "--",
        "sh",
        "-c",
        "kill -TERM $$",
    ])
    .assert()
    .code(143);

    let request = server.join().expect("server thread completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn ops_session_requires_command_after_separator() {
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let (config_dir, config_path) = write_github_config("ops-session-config-test");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--app-id",
        "1",
        "--repo",
        "OWNER/REPO",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("<COMMAND>"));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn ops_session_accepts_command_options_after_separator() {
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");
    let config_dir = unique_temp_dir("ops-session-config-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        "[github_app]\nprivate_key_path = \"/not/a/key.pem\"\n",
    )
    .expect("config written");

    cmd.args([
        "github-app",
        "run",
        "--config-path",
        config_path.to_str().expect("utf-8 config path"),
        "--app-id",
        "1",
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
    .failure()
    .stderr(predicate::str::contains("failed to read private key"));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn creates_github_app_agent_workflow_skill() {
    let skills_dir = unique_temp_dir("ops-session-skill-test");
    let mut cmd = Command::cargo_bin("ops-session").expect("binary exists");

    cmd.args([
        "agent-skill",
        "--install-path",
        skills_dir.to_str().expect("utf-8 path"),
    ])
    .assert()
    .success()
    .stdout(predicate::str::contains("ops-session-workflow"));

    let skill_file = skills_dir.join("ops-session-workflow").join("SKILL.md");
    let skill = fs::read_to_string(&skill_file).expect("skill file exists");
    assert!(skill.contains("name: ops-session-workflow"));
    assert!(skill.contains("ops-session github-app run"));
    assert!(skill.contains("ops-session github-app config check"));
    assert!(skill.contains("provider-scoped session runner"));
    assert!(skill.contains("[github_app.profiles.codex-review]"));
    assert!(skill.contains("OPS_SESSION_GITHUB_PROFILE"));

    fs::remove_dir_all(skills_dir).expect("temporary skill directory removed");
}

const TEST_RSA_PRIVATE_KEY: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC56nCrsvN8UqK+\n\
yKdhY9ecfqRnqzphLzOKtyxhWz28W0xLE2HbzRkSz6IoQbO71QVLrs2IpsHxtJnW\n\
05xBQiR8YAUI0w3K0lVIAbM/OewkqgiLAvZvX/iocT1URy2ixJs6f1eodkJ0Os0n\n\
CU0DZRX5vUe6ic90OIGyftgcJpYutpq1oSEdQLCHwjGIIitpRd6ztAdMXtUuZ2wZ\n\
R0o4UaCF/Ptyt+CpZW/jRQoBoDiXetnPc7GGu9JoAZU0tpt3sdqvnrNdafyd7FMO\n\
bKyxfb7uNHGqqy36dWGQcpBqsw6WZZJVTZPKPBwngXGg8hM6wGoHIfDCePvkhsCi\n\
5WqOP/qvAgMBAAECggEAFHjIxFdVsWRmEE0HBVXZqZ1WXCYCLS5l5gnqhKPn5eRF\n\
v+SX+3yXnLcpW3Z0pKO9zAopDrmSFJv27q1pgNQYMWvfUgvvclx70IyDYNxvcNAa\n\
VbhTS4tNVbr2bl/SGiC9GRFppR60jZjl+zzucofAhjn9+n/vTJRmT7Hg+SSUl/sK\n\
AshWb6F6TyHnw/gdqysq9qS+kSvmRywxEv21Vu8EgZGm/bys/Zu53XKrwOpw974p\n\
rsn/4b+vk6oiZJOB9nPbrBcch3duCdtkbVij9dYz1MVbPpke8uLb9iUF9d+q5wJu\n\
BcToL/8ErYmkPctlt19vl896S6oe4z9a5xIx7nwo4QKBgQD7OPHbKoJNyhq/Gml6\n\
sn+WIpEG7AKHRsJK3K0XyEs2l78ekCsYJlbWU/ymP8dZfr2DoQKeUk43/yoiLWAO\n\
c6eW9Pq2oJNRzhQdls45hdgjm6iKQpFRTdVoK7mK1ZFjnl8dr0e6NyEfdSV7IIP4\n\
oxRmhe2BVduBLIejxp2erXX4IQKBgQC9c44vjB6lsdDZXJLnwopBnuSzgbUiNzuM\n\
t532mu+tSw3vWm3lE43CHkvFM7HjrxxAK90EYq8l5k57LiaEbrMnMR1SnjnIOkUR\n\
vHjaocnES9FF9wOMCoOrz6SIHR4Xx6Tvj03YsPMgJrihJyLu/CpEjkZK86IW0/EP\n\
QjriOHFYzwKBgQCkE44KmVnfWndbhvGLHFeuA8d6oNwJ5BHzeOtoE/3jmvpNCNXM\n\
gQXIF7R0FEWr0tYNyTP/mTvS4Mlw5vfMmIbFVh0E+B0fmZuTs7He6ea/YuOR4WYt\n\
lsshrSUSYugBCyeOKLONEIKGnCktoI/w7Pne9+ulxCCH3kB8m7TINPxOYQKBgGeQ\n\
bd/MJ0zI4bSRCLWtAUtSAw+mDlDABMut7KpMlE0VRG7d7klV4R6G1UDeO5aNuVHT\n\
KKUnFTwQpEJuPhwTL9hy3ua1HD06rVs+voo1+0hVcfdfSw8ZCFW50uWdlT/GoYFb\n\
w2B7isy+nhtqe4xNSQXlCMQcXzU/cv22ZN4ZoMy9AoGBAJA6iJSdEz3wQkqPVLvQ\n\
YasrScJznIf2ZoWawMx2GDY1jrCEFLa2j+2jAFmXHJhCXnk63XUATNVDtHHB/T9A\n\
VnX0GJ36qCIZLrq+r2IYUHJNpRCbMDxBpPHeGybT/7c648FzahrdHfF2ygyKO4PW\n\
MzBXuiFERcpCt4YM/pVtnc99\n\
-----END PRIVATE KEY-----";

#[test]
fn refuses_to_overwrite_existing_skill_without_force() {
    let skills_dir = unique_temp_dir("ops-session-skill-test");

    let mut create = Command::cargo_bin("ops-session").expect("binary exists");
    create
        .args([
            "agent-skill",
            "-i",
            skills_dir.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();

    assert!(skills_dir
        .join("ops-session-workflow")
        .join("SKILL.md")
        .exists());

    let mut overwrite = Command::cargo_bin("ops-session").expect("binary exists");
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

fn write_github_config(prefix: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let config_dir = unique_temp_dir(prefix);
    fs::create_dir(&config_dir).expect("config dir created");
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\nprivate_key_path = \"{}\"\ndefault_profile = \"default\"\n\n[github_app.profiles.default]\nrepos = [\"OWNER/REPO\"]\n\n[github_app.profiles.default.permissions]\ncontents = \"read\"\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");
    (config_dir, config_path)
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}-{}-{unique}-{counter}",
        std::process::id()
    ))
}

#[cfg(unix)]
fn one_token_response_server() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server binds");
    let address = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test server accepts");
        let mut buffer = [0; 8192];
        let bytes = stream.read(&mut buffer).expect("test server reads request");
        let request = String::from_utf8_lossy(&buffer[..bytes]).to_ascii_lowercase();
        let body = r#"{"token":"test-token","expires_at":"2026-06-15T00:00:00Z","repository_selection":"selected","repositories":[],"permissions":{}}"#;
        let response = format!(
            "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("test server writes response");
        request
    });

    (format!("http://{address}"), handle)
}

#[cfg(unix)]
fn token_cache_response_server() -> (String, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server binds");
    let address = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        for index in 0..2 {
            let (mut stream, _) = listener.accept().expect("test server accepts");
            let mut buffer = [0; 8192];
            let bytes = stream.read(&mut buffer).expect("test server reads request");
            requests.push(String::from_utf8_lossy(&buffer[..bytes]).to_ascii_lowercase());

            let body = if index == 0 {
                r#"{"token":"cached-token","expires_at":"2099-06-15T00:00:00Z","repository_selection":"selected","repositories":[],"permissions":{}}"#
            } else {
                r#"{"total_count":1,"repositories":[],"repository_selection":"selected"}"#
            };
            let status = if index == 0 { "201 Created" } else { "200 OK" };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("test server writes response");
        }
        requests
    });

    (format!("http://{address}"), handle)
}
