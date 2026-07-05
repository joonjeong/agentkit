use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn policy_check_accepts_sample_policy() {
    let temp = temp_dir("ops-runbook-policy-check");
    let policy = write_policy(&temp, SAMPLE_POLICY);

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["policy", "check"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("policy OK:").and(predicate::str::contains("callers: hermes")),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn policy_check_accepts_explicit_policy_path() {
    let temp = temp_dir("ops-runbook-policy-check-path");
    let policy = write_policy(&temp, SAMPLE_POLICY);
    let invalid_policy = temp.join("invalid-policy.toml");
    fs::write(&invalid_policy, "not toml").expect("invalid policy written");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args([
            "policy",
            "check",
            "--policy-path",
            policy.to_str().expect("utf-8 path"),
        ])
        .env("OPS_RUNBOOK_POLICY_PATH", &invalid_policy)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("policy OK:").and(predicate::str::contains("callers: hermes")),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn policy_explain_dumps_validated_policy() {
    let temp = temp_dir("ops-runbook-explain");
    let policy = write_policy(&temp, SAMPLE_POLICY);

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["policy", "explain"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
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
                .and(predicate::str::contains("logs tailscale")),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn policy_template_prints_backend_template() {
    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["policy", "template", "--backend", "openrc"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("backend = \"openrc\"")
                .and(predicate::str::contains("[callers.hermes]"))
                .and(predicate::str::contains("service_control")),
        );
}

#[test]
fn policy_template_writes_output_without_overwriting_by_default() {
    let temp = temp_dir("ops-runbook-policy-template");
    let output = temp.join("policy.toml");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args([
            "policy",
            "template",
            "--output",
            output.to_str().expect("utf-8 path"),
        ])
        .assert()
        .success();

    let contents = fs::read_to_string(&output).expect("template written");
    assert!(contents.contains("backend = \"systemd\""));

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args([
            "policy",
            "template",
            "--backend",
            "openrc",
            "--output",
            output.to_str().expect("utf-8 path"),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("output already exists"));

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args([
            "policy",
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
    let temp = temp_dir("ops-runbook-status");
    let policy = write_policy(&temp, SAMPLE_POLICY);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "status", "hermes"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
        .env("OPS_RUNBOOK_SYSTEMCTL_PATH", &recorder)
        .env("OPS_RUNBOOK_RECORD_PATH", &record)
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
    let temp = temp_dir("ops-runbook-start");
    let policy = write_policy(&temp, SAMPLE_POLICY);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "start", "hermes"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
        .env("OPS_RUNBOOK_SYSTEMCTL_PATH", &recorder)
        .env("OPS_RUNBOOK_RECORD_PATH", &record)
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
    let temp = temp_dir("ops-runbook-stop");
    let policy = write_policy(&temp, SAMPLE_POLICY);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "stop", "hermes"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
        .env("OPS_RUNBOOK_SYSTEMCTL_PATH", &recorder)
        .env("OPS_RUNBOOK_RECORD_PATH", &record)
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
fn openrc_service_status_runs_rc_service_without_shell() {
    let temp = temp_dir("ops-runbook-openrc-status");
    let policy = write_policy(&temp, OPENRC_POLICY);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "rc-service-recorder");
    let record = temp.join("rc-service.args");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "status", "hermes"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
        .env("OPS_RUNBOOK_RC_SERVICE_PATH", &recorder)
        .env("OPS_RUNBOOK_RECORD_PATH", &record)
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
    let temp = temp_dir("ops-runbook-openrc-start");
    let policy = write_policy(&temp, OPENRC_POLICY);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "rc-service-recorder");
    let record = temp.join("rc-service.args");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "start", "hermes"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
        .env("OPS_RUNBOOK_RC_SERVICE_PATH", &recorder)
        .env("OPS_RUNBOOK_RECORD_PATH", &record)
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
    let temp = temp_dir("ops-runbook-openrc-logs");
    let policy = write_policy(&temp, OPENRC_POLICY);
    let audit = temp.join("audit.log");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["logs", "hermes", "--lines", "200"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
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
fn policy_check_rejects_unknown_policy_fields() {
    let temp = temp_dir("ops-runbook-unknown-policy-field");
    let policy = write_policy(
        &temp,
        r#"
version = 1

[defaults]
max_log_lines = 1000

[callers.hermes]
service_restarts = ["nginx"]
"#,
    );

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["policy", "check"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown field"));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn denied_target_is_audited_and_not_executed() {
    let temp = temp_dir("ops-runbook-denied");
    let policy = write_policy(&temp, SAMPLE_POLICY);
    let audit = temp.join("audit.log");
    let recorder = write_recorder(&temp, "systemctl-recorder");
    let record = temp.join("systemctl.args");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "restart", "nginx"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
        .env("OPS_RUNBOOK_SYSTEMCTL_PATH", &recorder)
        .env("OPS_RUNBOOK_RECORD_PATH", &record)
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
fn logs_rejects_line_count_above_policy_maximum() {
    let temp = temp_dir("ops-runbook-lines");
    let policy = write_policy(&temp, SAMPLE_POLICY);
    let audit = temp.join("audit.log");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["logs", "hermes", "--lines", "999999"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("OPS_RUNBOOK_AUDIT_LOG", &audit)
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
    let temp = temp_dir("ops-runbook-root");
    let policy = write_policy(&temp, SAMPLE_POLICY);

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["service", "status", "hermes"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
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
    let temp = temp_dir("ops-runbook-bootstrap");
    let source_binary = temp.join("source/ops-runbook");
    let binary = temp.join("bin/ops-runbook");
    let sudoers = temp.join("sudoers/custom-ops-agent");
    let policy = temp.join("etc/policy.toml");
    let audit_log = temp.join("logs/audit.log");
    let sudo_log = temp.join("logs/sudo.log");
    let logrotate = temp.join("logrotate/ops-runbook");
    fs::create_dir(source_binary.parent().expect("source parent")).expect("source parent created");
    fs::write(&source_binary, b"fake ops-runbook binary").expect("source binary written");

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--skip-visudo",
            "--source-binary",
            source_binary.to_str().expect("utf-8 path"),
            "--group",
            "custom-ops",
            "--binary-path",
            binary.to_str().expect("utf-8 path"),
            "--sudoers-path",
            sudoers.to_str().expect("utf-8 path"),
            "--policy-path",
            policy.to_str().expect("utf-8 path"),
            "--audit-log-path",
            audit_log.to_str().expect("utf-8 path"),
            "--sudo-log-path",
            sudo_log.to_str().expect("utf-8 path"),
            "--logrotate-path",
            logrotate.to_str().expect("utf-8 path"),
        ])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("sudoers ready:"));

    assert_eq!(
        fs::read(&binary).expect("installed binary"),
        b"fake ops-runbook binary"
    );
    let sudoers_contents = fs::read_to_string(&sudoers).expect("sudoers written");
    assert!(sudoers_contents.contains("Defaults:%custom-ops"));
    assert!(sudoers_contents.contains(&format!("logfile=\"{}\"", sudo_log.display())));
    assert!(sudoers_contents.contains(&format!("{} service restart *", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} policy check *", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} policy explain", binary.display())));
    assert!(sudoers_contents.contains(&format!("{} policy explain *", binary.display())));
    assert!(!sudoers_contents.contains(" bootstrap"));

    let policy_contents = fs::read_to_string(policy).expect("policy written");
    assert!(policy_contents.contains("backend = \"systemd\""));
    assert!(policy_contents.contains("[callers.hermes]"));

    let logrotate_contents = fs::read_to_string(logrotate).expect("logrotate written");
    assert!(logrotate_contents.contains(&audit_log.display().to_string()));
    assert!(logrotate_contents.contains(&sudo_log.display().to_string()));

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn bootstrap_rejects_relative_sudoers_paths() {
    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args([
            "bootstrap",
            "--skip-system-accounts",
            "--skip-visudo",
            "--sudoers-path",
            "relative/sudoers",
        ])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--sudoers-path must be absolute"));
}

#[test]
fn bootstrap_can_write_openrc_default_policy() {
    let temp = temp_dir("ops-runbook-bootstrap-openrc");
    let source_binary = temp.join("source/ops-runbook");
    let binary = temp.join("bin/ops-runbook");
    let sudoers = temp.join("sudoers/ops-agent");
    let policy = temp.join("etc/policy.toml");
    let audit_log = temp.join("logs/audit.log");
    let sudo_log = temp.join("logs/sudo.log");
    let logrotate = temp.join("logrotate/ops-runbook");
    fs::create_dir(source_binary.parent().expect("source parent")).expect("source parent created");
    fs::write(&source_binary, b"fake ops-runbook binary").expect("source binary written");

    Command::cargo_bin("ops-runbook")
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
            "--policy-path",
            policy.to_str().expect("utf-8 path"),
            "--audit-log-path",
            audit_log.to_str().expect("utf-8 path"),
            "--sudo-log-path",
            sudo_log.to_str().expect("utf-8 path"),
            "--logrotate-path",
            logrotate.to_str().expect("utf-8 path"),
        ])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .assert()
        .success();

    let policy_contents = fs::read_to_string(policy).expect("policy written");
    assert!(policy_contents.contains("backend = \"openrc\""));

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

fn write_policy(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("policy.toml");
    fs::write(&path, contents).expect("policy written");
    path
}

#[cfg(unix)]
fn write_recorder(dir: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    fs::write(
        &path,
        "#!/bin/sh\n: > \"$OPS_RUNBOOK_RECORD_PATH\"\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> \"$OPS_RUNBOOK_RECORD_PATH\"; done\n",
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
        "@echo off\r\nbreak > %OPS_RUNBOOK_RECORD_PATH%\r\n:loop\r\nif \"%1\"==\"\" exit /b 0\r\necho %1>> %OPS_RUNBOOK_RECORD_PATH%\r\nshift\r\ngoto loop\r\n",
    )
    .expect("recorder written");
    path
}

const SAMPLE_POLICY: &str = include_str!("../resources/examples/policy/systemd.example.toml");
const OPENRC_POLICY: &str = include_str!("../resources/examples/policy/openrc.example.toml");
