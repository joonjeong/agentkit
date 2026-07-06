use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn config_check_accepts_valid_github_app_profile() {
    let config_dir = unique_temp_dir("agentd-config-check-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let config_path = write_config(&config_dir, "https://api.github.com");
    let mut cmd = Command::cargo_bin("agentd").expect("binary exists");

    cmd.args(["config", "check", "--config-path"])
        .arg(&config_path)
        .assert()
        .success()
        .stdout(
            predicate::str::contains("config OK")
                .and(predicate::str::contains("github_app profiles: 1")),
        );

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn config_check_rejects_profile_without_allowed_repos() {
    let config_dir = unique_temp_dir("agentd-config-check-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\napp_id = 1\n\n[github_app.profiles.default]\nrepos = []\n\n[github_app.profiles.default.private_key]\ntype = \"file\"\npath = \"{}\"\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");

    let mut cmd = Command::cargo_bin("agentd").expect("binary exists");
    cmd.args(["config", "check", "--config-path"])
        .arg(&config_path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("must include at least one repo"));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn serve_once_mints_github_app_token_over_uds() {
    let (api_url, github_server) = github_token_response_server();
    let config_dir = unique_temp_dir("agentd-serve-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let config_path = write_config(&config_dir, &api_url);
    let socket_path = config_dir.join("agentd.sock");

    let mut agentd = std::process::Command::new(assert_cmd::cargo::cargo_bin("agentd"))
        .args([
            "serve",
            "--once",
            "--config-path",
            config_path.to_str().expect("utf-8 config path"),
            "--socket-path",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .spawn()
        .expect("agentd starts");

    wait_for_socket(&socket_path);
    let mut stream = UnixStream::connect(&socket_path).expect("client connects");
    stream
        .write_all(
            br#"{"type":"github_app_token","profile":"default","repos":["OWNER/REPO"],"permissions":{"contents":"read"}}"#,
        )
        .expect("request writes");
    stream.write_all(b"\n").expect("request newline writes");

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .expect("response reads");

    let status = agentd.wait().expect("agentd exits");
    assert!(status.success());
    assert!(response.contains(r#""status":"ok""#));
    assert!(response.contains(r#""token":"agentd-test-token""#));
    assert!(response.contains(&format!(r#""api_url":"{api_url}""#)));

    let request = github_server.join().expect("github server completed");
    assert!(request.starts_with("post /app/installations/42/access_tokens "));
    assert!(request.contains(r#""repositories":["repo"]"#));
    assert!(request.contains(r#""permissions":{"contents":"read"}"#));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

#[test]
fn serve_once_rejects_repo_outside_profile_scope() {
    let config_dir = unique_temp_dir("agentd-serve-test");
    fs::create_dir(&config_dir).expect("config dir created");
    let config_path = write_config(&config_dir, "https://api.github.com");
    let socket_path = config_dir.join("agentd.sock");

    let mut agentd = std::process::Command::new(assert_cmd::cargo::cargo_bin("agentd"))
        .args([
            "serve",
            "--once",
            "--config-path",
            config_path.to_str().expect("utf-8 config path"),
            "--socket-path",
            socket_path.to_str().expect("utf-8 socket path"),
        ])
        .spawn()
        .expect("agentd starts");

    wait_for_socket(&socket_path);
    let mut stream = UnixStream::connect(&socket_path).expect("client connects");
    stream
        .write_all(
            br#"{"type":"github_app_token","profile":"default","repos":["OWNER/OTHER"],"permissions":{"contents":"read"}}"#,
        )
        .expect("request writes");
    stream.write_all(b"\n").expect("request newline writes");

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .expect("response reads");

    let status = agentd.wait().expect("agentd exits");
    assert!(status.success());
    assert!(response.contains(r#""status":"error""#));
    assert!(response.contains("is not allowed"));

    fs::remove_dir_all(config_dir).expect("config dir removed");
}

fn github_token_response_server() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test server binds");
    let address = listener.local_addr().expect("test server address");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("test server accepts");
        let mut buffer = [0; 8192];
        let bytes = stream.read(&mut buffer).expect("test server reads request");
        let request = String::from_utf8_lossy(&buffer[..bytes]).to_ascii_lowercase();
        let body = r#"{"token":"agentd-test-token","expires_at":"2026-06-15T00:00:00Z","repository_selection":"selected","repositories":[],"permissions":{}}"#;
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

fn write_config(config_dir: &std::path::Path, api_url: &str) -> std::path::PathBuf {
    let private_key_path = config_dir.join("private-key.pem");
    fs::write(&private_key_path, TEST_RSA_PRIVATE_KEY).expect("private key written");
    let config_path = config_dir.join("config.toml");
    fs::write(
        &config_path,
        format!(
            "[github_app]\napp_id = 1\ninstallation_id = 42\napi_url = \"{api_url}\"\ndefault_profile = \"default\"\n\n[github_app.profiles.default]\nrepos = [\"OWNER/REPO\"]\n\n[github_app.profiles.default.private_key]\ntype = \"file\"\npath = \"{}\"\n\n[github_app.profiles.default.permissions]\ncontents = \"read\"\n",
            private_key_path.to_string_lossy()
        ),
    )
    .expect("config written");
    config_path
}

fn wait_for_socket(socket_path: &std::path::Path) {
    for _ in 0..100 {
        if socket_path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("agentd socket was not created");
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
