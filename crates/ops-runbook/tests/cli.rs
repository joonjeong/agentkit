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
            predicate::str::contains("policy OK:")
                .and(predicate::str::contains("callers: hermes, openclaw")),
        );

    fs::remove_dir_all(temp).expect("temporary directory removed");
}

#[test]
fn explain_reports_allow_for_current_sudo_user() {
    let temp = temp_dir("ops-runbook-explain");
    let policy = write_policy(&temp, SAMPLE_POLICY);

    Command::cargo_bin("ops-runbook")
        .expect("binary exists")
        .args(["policy", "explain", "service_restart", "nginx"])
        .env("OPS_RUNBOOK_TEST_OVERRIDES", "1")
        .env("OPS_RUNBOOK_POLICY_PATH", &policy)
        .env("SUDO_USER", "hermes")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("caller: hermes")
                .and(predicate::str::contains("decision: allow"))
                .and(predicate::str::contains(
                    "source: callers.hermes.service_restart",
                )),
        );

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
        .args(["service", "status", "nginx"])
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
        "--no-pager\nstatus\nnginx.service\n"
    );
    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains("caller=hermes action=service_status target=nginx result=allow"));

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
        .env("SUDO_USER", "openclaw")
        .assert()
        .failure()
        .stderr(predicate::str::contains("target not allowed: nginx"));

    assert!(!record.exists());
    let audit_log = fs::read_to_string(audit).expect("audit log");
    assert!(audit_log.contains(
        "caller=openclaw action=service_restart target=nginx result=deny reason=target_not_allowed"
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
        .args(["logs", "nginx", "--lines", "999999"])
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
        .args(["service", "status", "nginx"])
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

const SAMPLE_POLICY: &str = r#"
version = 1

[defaults]
max_log_lines = 1000

[callers.hermes]
service_restart = ["nginx", "coredns", "cloudflared"]
service_reload = ["nginx", "coredns"]
service_status = ["nginx", "coredns", "cloudflared"]
logs = ["nginx", "coredns", "cloudflared"]

[callers.openclaw]
service_restart = ["openclaw", "myriad-bot"]
service_reload = []
service_status = ["openclaw", "myriad-bot"]
logs = ["openclaw", "myriad-bot"]
"#;
