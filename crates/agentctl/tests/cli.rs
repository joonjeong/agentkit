use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn config_check_accepts_sample_config() {
    let temp = temp_dir("agentctl-config-check");
    let config = write_config(&temp, &sample_config("systemd"));

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["config", "check"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("config OK:").and(predicate::str::contains("callers: hermes")),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn config_check_accepts_explicit_config_path() {
    let temp = temp_dir("agentctl-config-check-path");
    let config = write_config(&temp, &sample_config("systemd"));
    let invalid_config = temp.join("invalid-config.toml");
    fs::write(&invalid_config, "not toml").expect("invalid config written");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "config",
            "check",
            "--config-path",
            config.to_str().expect("utf-8 path"),
        ])
        .env("AGENTCTL_CONFIG_PATH", &invalid_config)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("config OK:").and(predicate::str::contains("callers: hermes")),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn config_explain_dumps_validated_config() {
    let temp = temp_dir("agentctl-explain");
    let config = write_config(&temp, &sample_config("systemd"));

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["config", "explain"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("version: 1")
                .and(predicate::str::contains("backend: systemd"))
                .and(predicate::str::contains("callers:"))
                .and(predicate::str::contains("  hermes:"))
                .and(predicate::str::contains(
                    "    service_control: [\"hermes\", \"cloudflared\", \"tailscale\"]",
                ))
                .and(predicate::str::contains("service start hermes"))
                .and(predicate::str::contains("service stop hermes"))
                .and(predicate::str::contains("service restart hermes"))
                .and(predicate::str::contains("service status cloudflared"))
                .and(predicate::str::contains("service logs tailscale"))
                .and(predicate::str::contains("telegram notify myriad").not()),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn config_template_prints_backend_template() {
    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["config", "template", "--backend", "openrc"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("backend = \"openrc\"")
                .and(predicate::str::contains("[callers.hermes]"))
                .and(predicate::str::contains("service_control")),
        );
}

#[test]
fn config_template_writes_output_without_overwriting_by_default() {
    let temp = temp_dir("agentctl-config-template");
    let output = temp.join("config.toml");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "config",
            "template",
            "--output",
            output.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();

    let contents = fs::read_to_string(&output).expect("template written");
    assert!(contents.contains("backend = \"systemd\""));

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "config",
            "template",
            "--backend",
            "openrc",
            "--output",
            output.to_str().expect("utf-8 path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("output already exists"));

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "config",
            "template",
            "--backend",
            "openrc",
            "--output",
            output.to_str().expect("utf-8 path"),
            "--force",
        ])
        .assert()
        .success();

    let contents = fs::read_to_string(output).expect("template overwritten");
    assert!(contents.contains("backend = \"openrc\""));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn service_status_runs_fixed_systemctl_without_shell() {
    let temp = temp_dir("agentctl-status");
    let socket_path = short_socket_path("agentctl-service-status");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"ok","version":1,"exit_code":0,"stdout":"active\n","stderr":""}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "status",
            "hermes",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("active"));

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""type":"service""#));
    assert!(request.contains(r#""action":"status""#));
    assert!(request.contains(r#""service":"hermes""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn service_start_runs_fixed_systemctl_without_shell() {
    let temp = temp_dir("agentctl-start");
    let socket_path = short_socket_path("agentctl-service-start");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"ok","version":1,"exit_code":0,"stdout":"","stderr":""}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "start",
            "hermes",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .success();

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""action":"start""#));
    assert!(request.contains(r#""service":"hermes""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn service_stop_runs_fixed_systemctl_without_shell() {
    let temp = temp_dir("agentctl-stop");
    let socket_path = short_socket_path("agentctl-service-stop");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"ok","version":1,"exit_code":0,"stdout":"","stderr":""}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "stop",
            "hermes",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .success();

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""action":"stop""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn notify_posts_to_allowlisted_telegram_channel() {
    let temp = temp_dir("agentctl-notify-telegram");
    let config = write_config(&temp, TELEGRAM_NOTIFY_CONFIG);
    let audit = temp.join("audit.log");
    let socket_path = short_socket_path("agentctl-telegram");
    let agentd = agentd_response_server(&socket_path, r#"{"status":"ok","version":1}"#);

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "telegram",
            "notify",
            "myriad",
            "--chat-id",
            "123456789",
            "--severity",
            "critical",
            "--title",
            "disk full",
            "--message",
            "/var is 95%",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("USER", "hermes")
        .assert()
        .success();

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""type":"notify""#));
    assert!(request.contains(r#""version":1"#));
    assert!(request.contains(r#""caller":"hermes""#));
    assert!(request.contains(r#""provider":"telegram""#));
    assert!(request.contains(r#""profile":"myriad""#));
    assert!(request.contains(r#""chat_id":"123456789""#));
    assert!(request.contains(r#""severity":"critical""#));
    assert!(request.contains(r#""title":"disk full""#));
    assert!(request.contains(r#""message":"/var is 95%""#));

    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains(
        "caller=hermes action=notify target=telegram:myriad result=delegate reason=agentd"
    ));
    assert!(audit_log.contains(
        "caller=hermes action=notify target=telegram:myriad result=executed reason=sent"
    ));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn notify_posts_to_allowlisted_discord_channel() {
    let temp = temp_dir("agentctl-notify-discord");
    let config = write_config(
        &temp,
        r#"
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[callers.hermes]
service_read = ["hermes"]
"#,
    );
    let audit = temp.join("audit.log");
    let socket_path = short_socket_path("agentctl-discord");
    let agentd = agentd_response_server(&socket_path, r#"{"status":"ok","version":1}"#);

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "discord",
            "notify",
            "myriad",
            "--severity",
            "warning",
            "--message",
            "service degraded",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("USER", "hermes")
        .assert()
        .success();

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""type":"notify""#));
    assert!(request.contains(r#""caller":"hermes""#));
    assert!(request.contains(r#""provider":"discord""#));
    assert!(request.contains(r#""profile":"myriad""#));
    assert!(request.contains(r#""severity":"warning""#));
    assert!(request.contains(r#""message":"service degraded""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn notify_reports_agentd_rejection() {
    let temp = temp_dir("agentctl-notify-denied");
    let config = write_config(&temp, TELEGRAM_NOTIFY_CONFIG);
    let audit = temp.join("audit.log");
    let socket_path = short_socket_path("agentctl-denied");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"error","version":1,"error":"telegram profile not found: other"}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "telegram",
            "notify",
            "other",
            "--chat-id",
            "123456789",
            "--message",
            "should not send",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("USER", "hermes")
        .assert()
        .failure()
        .stderr(predicate::str::contains("agentd rejected request"));

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""provider":"telegram""#));
    assert!(request.contains(r#""profile":"other""#));

    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains(
        "caller=hermes action=notify target=telegram:other result=delegate reason=agentd"
    ));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn openrc_service_status_runs_rc_service_without_shell() {
    let temp = temp_dir("agentctl-openrc-status");
    let socket_path = short_socket_path("agentctl-service-openrc-status");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"ok","version":1,"exit_code":0,"stdout":"status\n","stderr":""}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "status",
            "hermes",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("status"));

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""action":"status""#));
    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn openrc_service_start_runs_rc_service_without_shell() {
    let temp = temp_dir("agentctl-openrc-start");
    let socket_path = short_socket_path("agentctl-service-openrc-start");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"ok","version":1,"exit_code":0,"stdout":"","stderr":""}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "start",
            "hermes",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .success();

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""action":"start""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn openrc_logs_are_explicitly_unsupported() {
    let temp = temp_dir("agentctl-openrc-logs");
    let socket_path = short_socket_path("agentctl-service-openrc-logs");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"error","version":1,"error":"backend openrc does not support action logs"}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "logs",
            "hermes",
            "--lines",
            "200",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("agentd rejected request"));

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""action":"logs""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn config_check_rejects_unknown_config_fields() {
    let temp = temp_dir("agentctl-unknown-config-field");
    let config = write_config(
        &temp,
        r#"
version = 1

[defaults]
max_log_lines = 1000

[callers.hermes]
service_restarts = ["nginx"]
"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["config", "check"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown field"));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn denied_target_is_audited_and_not_executed() {
    let temp = temp_dir("agentctl-denied");
    let socket_path = short_socket_path("agentctl-service-denied");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"error","version":1,"error":"service \"nginx\" is not allowed"}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "restart",
            "nginx",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("agentd rejected request"));

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""service":"nginx""#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "macOS sandbox blocks test UDS listener bind"
)]
fn logs_rejects_line_count_above_config_maximum() {
    let temp = temp_dir("agentctl-lines");
    let socket_path = short_socket_path("agentctl-service-lines");
    let agentd = agentd_response_server(
        &socket_path,
        r#"{"status":"error","version":1,"error":"invalid log line count: 999999"}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "service",
            "logs",
            "hermes",
            "--lines",
            "999999",
            "--agentd-socket",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("agentd rejected request"));

    let request = agentd.join().expect("agentd request captured");
    assert!(request.contains(r#""lines":999999"#));

    fs::remove_file(socket_path).expect("socket removed");
    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn service_rejects_invalid_target_before_agentd() {
    let temp = temp_dir("agentctl-invalid-target");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "status", "../hermes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid target"));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn bootstrap_installs_binary_and_writes_configurable_files() {
    let temp = temp_dir("agentctl-bootstrap");
    let source_binary = temp.join("source/agentctl");
    let binary = temp.join("bin/agentctl");
    let config = temp.join("etc/config.toml");
    let audit_log = temp.join("logs/audit.log");
    let logrotate = temp.join("logrotate/agentctl");
    fs::create_dir(source_binary.parent().expect("source parent")).expect("source parent created");
    fs::write(&source_binary, b"fake agentctl binary").expect("source binary written");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--source-binary",
            source_binary.to_str().expect("utf-8 path"),
            "--group",
            "custom-agent",
            "--binary-path",
            binary.to_str().expect("utf-8 path"),
            "--config-path",
            config.to_str().expect("utf-8 path"),
            "--audit-log-path",
            audit_log.to_str().expect("utf-8 path"),
            "--logrotate-path",
            logrotate.to_str().expect("utf-8 path"),
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("logrotate ready:"));

    assert_eq!(
        fs::read(&binary).expect("installed binary"),
        b"fake agentctl binary"
    );
    let config_contents = fs::read_to_string(config).expect("config written");
    assert!(config_contents.contains("backend = \"systemd\""));
    assert!(config_contents.contains("[callers.hermes]"));

    let logrotate_contents = fs::read_to_string(logrotate).expect("logrotate written");
    assert!(logrotate_contents.contains(&audit_log.display().to_string()));
    assert!(!logrotate_contents.contains("sudo.log"));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn bootstrap_rejects_relative_logrotate_paths() {
    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--logrotate-path",
            "relative/logrotate",
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--logrotate-path must be absolute",
        ));
}

#[test]
fn bootstrap_can_write_openrc_default_config() {
    let temp = temp_dir("agentctl-bootstrap-openrc");
    let source_binary = temp.join("source/agentctl");
    let binary = temp.join("bin/agentctl");
    let config = temp.join("etc/config.toml");
    let audit_log = temp.join("logs/audit.log");
    let logrotate = temp.join("logrotate/agentctl");
    fs::create_dir(source_binary.parent().expect("source parent")).expect("source parent created");
    fs::write(&source_binary, b"fake agentctl binary").expect("source binary written");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--backend",
            "openrc",
            "--source-binary",
            source_binary.to_str().expect("utf-8 path"),
            "--binary-path",
            binary.to_str().expect("utf-8 path"),
            "--config-path",
            config.to_str().expect("utf-8 path"),
            "--audit-log-path",
            audit_log.to_str().expect("utf-8 path"),
            "--logrotate-path",
            logrotate.to_str().expect("utf-8 path"),
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .assert()
        .success();

    let config_contents = fs::read_to_string(config).expect("config written");
    assert!(config_contents.contains("backend = \"openrc\""));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

fn temp_dir(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{prefix}-{nonce}-{count}"));
    fs::create_dir(&path).expect("temporary directory created");
    path
}

fn short_socket_path(prefix: &str) -> PathBuf {
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let short_prefix = prefix
        .bytes()
        .filter(u8::is_ascii_alphanumeric)
        .take(8)
        .map(char::from)
        .collect::<String>();
    let dir = PathBuf::from("test-sockets");
    fs::create_dir_all(&dir).expect("socket test directory created");
    dir.join(format!(
        "{}-{}-{count}.sock",
        short_prefix,
        std::process::id()
    ))
}

fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("config.toml");
    fs::write(&path, contents).expect("config written");
    path
}

fn agentd_response_server(
    socket_path: &Path,
    response: &'static str,
) -> thread::JoinHandle<String> {
    let socket_path = socket_path.to_owned();
    let wait_path = socket_path.clone();
    let response = response.to_owned();
    let handle = thread::spawn(move || {
        let listener = UnixListener::bind(&socket_path).expect("agentd socket binds");
        let (mut stream, _) = listener.accept().expect("agentctl connects");
        let mut request = String::new();
        BufReader::new(stream.try_clone().expect("stream clones"))
            .read_line(&mut request)
            .expect("request reads");
        stream
            .write_all(response.as_bytes())
            .expect("response writes");
        stream.write_all(b"\n").expect("response newline writes");
        request
    });
    for _ in 0..100 {
        if wait_path.exists() {
            return handle;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("agentd socket was not created");
}

const CONFIG_TEMPLATE: &str = include_str!("../resources/templates/config.agentctl.toml.template");

fn sample_config(backend: &str) -> String {
    let (backend_name, backend_note, service_read_note) = match backend {
        "systemd" => ("systemd", "", ""),
        "openrc" => (
            "OpenRC",
            " The OpenRC backend supports service control and status;\n# logs return an explicit unsupported-backend error.",
            " Logs are currently unsupported on OpenRC.",
        ),
        other => panic!("unknown backend: {other}"),
    };

    CONFIG_TEMPLATE
        .replace("{backend_name}", backend_name)
        .replace("{backend}", backend)
        .replace("{backend_note}", backend_note)
        .replace("{service_read_note}", service_read_note)
}
const TELEGRAM_NOTIFY_CONFIG: &str = r#"
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[callers.hermes]
service_read = ["hermes"]
"#;
