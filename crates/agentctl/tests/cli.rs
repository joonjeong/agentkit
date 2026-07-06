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
    let config = write_config(&temp, SAMPLE_CONFIG);

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
    let config = write_config(&temp, SAMPLE_CONFIG);
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
    let config = write_config(&temp, SAMPLE_CONFIG);

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
                .and(predicate::str::contains("logs tailscale"))
                .and(predicate::str::contains("notify telegram myriad").not()),
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
fn service_status_runs_fixed_systemctl_without_shell() {
    let temp = temp_dir("agentctl-status");
    let config = write_config(&temp, SAMPLE_CONFIG);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "status", "hermes"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("AGENTCTL_SYSTEMCTL_PATH", &recorder)
        .env("AGENTCTL_RECORD_PATH", &record)
        .env("SUDO_USER", "hermes")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(record).expect("recorded args"),
        "--no-pager\nstatus\nhermes.service\n"
    );
    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains("caller=hermes action=service_status target=hermes result=allow"));
    assert!(audit_log.contains(
        "caller=hermes action=service_status target=hermes result=executed reason=exit_code=0"
    ));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn service_start_runs_fixed_systemctl_without_shell() {
    let temp = temp_dir("agentctl-start");
    let config = write_config(&temp, SAMPLE_CONFIG);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "start", "hermes"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("AGENTCTL_SYSTEMCTL_PATH", &recorder)
        .env("AGENTCTL_RECORD_PATH", &record)
        .env("SUDO_USER", "hermes")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(record).expect("recorded args"),
        "--no-pager\nstart\nhermes.service\n"
    );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn service_stop_runs_fixed_systemctl_without_shell() {
    let temp = temp_dir("agentctl-stop");
    let config = write_config(&temp, SAMPLE_CONFIG);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "stop", "hermes"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("AGENTCTL_SYSTEMCTL_PATH", &recorder)
        .env("AGENTCTL_RECORD_PATH", &record)
        .env("SUDO_USER", "hermes")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(record).expect("recorded args"),
        "--no-pager\nstop\nhermes.service\n"
    );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn notify_posts_to_allowlisted_telegram_channel() {
    let temp = temp_dir("agentctl-notify-telegram");
    let config = write_config(&temp, TELEGRAM_NOTIFY_CONFIG);
    let audit = temp.join("audit.log");
    let socket_path = short_socket_path("agentctl-telegram");
    let agentd = agentd_notify_response_server(&socket_path, r#"{"status":"ok","version":1}"#);

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "notify",
            "telegram",
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
        .env("SUDO_USER", "hermes")
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
    let agentd = agentd_notify_response_server(&socket_path, r#"{"status":"ok","version":1}"#);

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "notify",
            "discord",
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
        .env("SUDO_USER", "hermes")
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
fn notify_reports_agentd_rejection() {
    let temp = temp_dir("agentctl-notify-denied");
    let config = write_config(&temp, TELEGRAM_NOTIFY_CONFIG);
    let audit = temp.join("audit.log");
    let socket_path = short_socket_path("agentctl-denied");
    let agentd = agentd_notify_response_server(
        &socket_path,
        r#"{"status":"error","version":1,"error":"telegram profile not found: other"}"#,
    );

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "notify",
            "telegram",
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
        .env("SUDO_USER", "hermes")
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
fn openrc_service_status_runs_rc_service_without_shell() {
    let temp = temp_dir("agentctl-openrc-status");
    let config = write_config(&temp, OPENRC_CONFIG);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "rc-service-recorder");
    let record = temp.join("rc-service.args");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "status", "hermes"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("AGENTCTL_RC_SERVICE_PATH", &recorder)
        .env("AGENTCTL_RECORD_PATH", &record)
        .env("SUDO_USER", "hermes")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(record).expect("recorded args"),
        "hermes\nstatus\n"
    );
    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains("caller=hermes action=service_status target=hermes result=allow"));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn openrc_service_start_runs_rc_service_without_shell() {
    let temp = temp_dir("agentctl-openrc-start");
    let config = write_config(&temp, OPENRC_CONFIG);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "rc-service-recorder");
    let record = temp.join("rc-service.args");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "start", "hermes"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("AGENTCTL_RC_SERVICE_PATH", &recorder)
        .env("AGENTCTL_RECORD_PATH", &record)
        .env("SUDO_USER", "hermes")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(record).expect("recorded args"),
        "hermes\nstart\n"
    );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn openrc_logs_are_explicitly_unsupported() {
    let temp = temp_dir("agentctl-openrc-logs");
    let config = write_config(&temp, OPENRC_CONFIG);
    let audit = temp.join("audit.log");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["logs", "hermes", "--lines", "200"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("SUDO_USER", "hermes")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "backend openrc does not support action logs",
        ));

    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains("result=deny reason=unsupported_backend_action"));

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
fn denied_target_is_audited_and_not_executed() {
    let temp = temp_dir("agentctl-denied");
    let config = write_config(&temp, SAMPLE_CONFIG);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "restart", "nginx"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("AGENTCTL_SYSTEMCTL_PATH", &recorder)
        .env("AGENTCTL_RECORD_PATH", &record)
        .env("SUDO_USER", "hermes")
        .assert()
        .failure()
        .stderr(predicate::str::contains("target not allowed: nginx"));

    assert!(!record.exists());
    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains(
        "caller=hermes action=service_restart target=nginx result=deny reason=target_not_allowed"
    ));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn logs_rejects_line_count_above_config_maximum() {
    let temp = temp_dir("agentctl-lines");
    let config = write_config(&temp, SAMPLE_CONFIG);
    let audit = temp.join("audit.log");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["logs", "hermes", "--lines", "999999"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("AGENTCTL_AUDIT_LOG", &audit)
        .env("SUDO_USER", "hermes")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid log line count: 999999"));

    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains("result=deny reason=invalid_line_count"));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn direct_root_execution_is_rejected() {
    let temp = temp_dir("agentctl-root");
    let config = write_config(&temp, SAMPLE_CONFIG);

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args(["service", "status", "hermes"])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .env("AGENTCTL_CONFIG_PATH", &config)
        .env("SUDO_USER", "root")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "direct root execution is not allowed",
        ));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn bootstrap_installs_binary_and_writes_configurable_files() {
    let temp = temp_dir("agentctl-bootstrap");
    let source_binary = temp.join("source/agentctl");
    let binary = temp.join("bin/agentctl");
    let sudoers = temp.join("sudoers/custom-agent");
    let config = temp.join("etc/config.toml");
    let audit_log = temp.join("logs/audit.log");
    let sudo_log = temp.join("logs/sudo.log");
    let logrotate = temp.join("logrotate/agentctl");
    fs::create_dir(source_binary.parent().expect("source parent")).expect("source parent created");
    fs::write(&source_binary, b"fake agentctl binary").expect("source binary written");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--skip-visudo",
            "--source-binary",
            source_binary.to_str().expect("utf-8 path"),
            "--group",
            "custom-agent",
            "--binary-path",
            binary.to_str().expect("utf-8 path"),
            "--sudoers-path",
            sudoers.to_str().expect("utf-8 path"),
            "--config-path",
            config.to_str().expect("utf-8 path"),
            "--audit-log-path",
            audit_log.to_str().expect("utf-8 path"),
            "--sudo-log-path",
            sudo_log.to_str().expect("utf-8 path"),
            "--logrotate-path",
            logrotate.to_str().expect("utf-8 path"),
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("sudoers ready:"));

    assert_eq!(
        fs::read(&binary).expect("installed binary"),
        b"fake agentctl binary"
    );
    let sudoers_contents = fs::read_to_string(&sudoers).expect("sudoers written");
    assert!(sudoers_contents.contains("Defaults:%custom-agent"));
    assert!(sudoers_contents.contains(&format!("logfile=\"{}\"", sudo_log.display())));
    assert!(sudoers_contents.contains(&format!("{} service restart *", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} notify *", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} config check *", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} config explain", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} config explain *", binary.display())));
    assert!(!sudoers_contents.contains(" bootstrap"));

    let config_contents = fs::read_to_string(config).expect("config written");
    assert!(config_contents.contains("backend = \"systemd\""));
    assert!(config_contents.contains("[callers.hermes]"));

    let logrotate_contents = fs::read_to_string(logrotate).expect("logrotate written");
    assert!(logrotate_contents.contains(&audit_log.display().to_string()));
    assert!(logrotate_contents.contains(&sudo_log.display().to_string()));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn bootstrap_rejects_relative_sudoers_paths() {
    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--skip-visudo",
            "--sudoers-path",
            "relative/sudoers",
        ])
        .env("AGENTCTL_TEST_OVERRIDES", "1")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--sudoers-path must be absolute"));
}

#[test]
fn bootstrap_can_write_openrc_default_config() {
    let temp = temp_dir("agentctl-bootstrap-openrc");
    let source_binary = temp.join("source/agentctl");
    let binary = temp.join("bin/agentctl");
    let sudoers = temp.join("sudoers/agent");
    let config = temp.join("etc/config.toml");
    let audit_log = temp.join("logs/audit.log");
    let sudo_log = temp.join("logs/sudo.log");
    let logrotate = temp.join("logrotate/agentctl");
    fs::create_dir(source_binary.parent().expect("source parent")).expect("source parent created");
    fs::write(&source_binary, b"fake agentctl binary").expect("source binary written");

    Command::cargo_bin("agentctl")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--skip-visudo",
            "--backend",
            "openrc",
            "--source-binary",
            source_binary.to_str().expect("utf-8 path"),
            "--binary-path",
            binary.to_str().expect("utf-8 path"),
            "--sudoers-path",
            sudoers.to_str().expect("utf-8 path"),
            "--config-path",
            config.to_str().expect("utf-8 path"),
            "--audit-log-path",
            audit_log.to_str().expect("utf-8 path"),
            "--sudo-log-path",
            sudo_log.to_str().expect("utf-8 path"),
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
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::path::PathBuf::from("/tmp").join(format!(
        "{prefix}-{}-{nonce}-{count}.sock",
        std::process::id()
    ))
}

fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("config.toml");
    fs::write(&path, contents).expect("config written");
    path
}

fn agentd_notify_response_server(
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

#[cfg(unix)]
fn write_recorder(dir: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    fs::write(
        &path,
        "#!/bin/sh\n: > \"$AGENTCTL_RECORD_PATH\"\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> \"$AGENTCTL_RECORD_PATH\"; done\n",
    )
    .expect("recorder written");
    let mut permissions = fs::metadata(&path)
        .expect("recorder metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("recorder executable");
    path
}

#[cfg(not(unix))]
fn write_recorder(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(format!("{name}.bat"));
    fs::write(
        &path,
        "@echo off\r\nbreak > %AGENTCTL_RECORD_PATH%\r\n:loop\r\nif \"%1\"==\"\" exit /b 0\r\necho %1>> %AGENTCTL_RECORD_PATH%\r\nshift\r\ngoto loop\r\n",
    )
    .expect("recorder written");
    path
}

const SAMPLE_CONFIG: &str =
    include_str!("../../../resources/agentctl/examples/config/systemd.example.toml");
const OPENRC_CONFIG: &str =
    include_str!("../../../resources/agentctl/examples/config/openrc.example.toml");
const TELEGRAM_NOTIFY_CONFIG: &str = r#"
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[callers.hermes]
service_read = ["hermes"]
"#;
