# agentctl 설치 및 사용법

`agentctl`은 홈랩 운영 작업을 위한 config 기반 제한 실행기입니다.
자동화 에이전트가 sudo를 통해 제한된 root 작업만 실행하게 하며, raw shell,
`systemctl`, `apt`, Docker socket, Ansible 접근 권한은 주지 않습니다.

지원하는 서비스 관리자 backend:

- `systemd`: `systemctl`과 `journalctl`로 서비스 제어 및 로그 조회
- `openrc`: `rc-service`로 서비스 제어. `logs`는 지원하지 않음

## 보안 모델

신뢰할 수 있는 관리자가 다음 명령을 실행합니다.

```sh
sudo ./agentctl bootstrap --user hermes
```

`bootstrap`은 대상 호스트를 구성합니다.

- 현재 바이너리를 `/usr/local/sbin/agentctl`에 설치
- `agent` 시스템 그룹이 없으면 생성
- `--user`로 지정한 기존 계정을 `agent` 그룹에 추가
- `/etc/agentkit/agentctl.toml`이 없으면 기본 config 생성
- `/etc/sudoers.d/agent` 생성
- `/var/log/agentctl` 아래 로그 경로 생성
- `/etc/logrotate.d/agentctl` 생성

생성되는 sudoers 규칙은 `agent` 멤버에게 운영 서브커맨드만 허용합니다.
에이전트가 `bootstrap`을 실행할 수는 없습니다. 이 규칙은
`crates/agentctl/resources/templates/sudoers.agent.template`에서 렌더링됩니다.

## 빌드

저장소 루트에서 실행합니다.

```sh
cargo build --release --bin agentctl
```

바이너리는 다음 위치에 생성됩니다.

```text
target/release/agentctl
```

## Bootstrap

바이너리를 대상 호스트에 복사하거나 다운로드한 뒤 실행합니다.

```sh
sudo ./agentctl bootstrap --user hermes
```

주요 옵션:

```sh
sudo ./agentctl bootstrap \
  --source-binary /path/to/agentctl \
  --binary-path /usr/local/sbin/agentctl \
  --backend systemd \
  --group agent \
  --sudoers-path /etc/sudoers.d/agent \
  --config-path /etc/agentkit/agentctl.toml \
  --audit-log-path /var/log/agentctl/audit.log \
  --sudo-log-path /var/log/agentctl/sudo.log \
  --logrotate-path /etc/logrotate.d/agentctl
```

기존 config 파일을 샘플 config로 교체하려면 `--force-config`를 사용합니다.
`--force-config`가 없으면 기존 config 파일은 보존됩니다.

## Config

기본 config 형식은 TOML입니다.

```toml
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[callers.hermes]
# service start, stop, restart, reload 허용
service_control = ["hermes", "cloudflared", "tailscale"]

# service status와 logs 허용
service_read = ["hermes", "cloudflared", "tailscale"]
```

호출자는 `SUDO_USER`에서 읽습니다. 예를 들어 `hermes`가 다음을 실행하면:

```sh
sudo /usr/local/sbin/agentctl service restart hermes
```

`agentctl`은 `callers.hermes.service_control`에 `hermes`가 있는지 확인합니다.

Alpine/OpenRC 호스트에서는 다음처럼 설정합니다.

```toml
[defaults]
backend = "openrc"
max_log_lines = 1000
```

`backend = "openrc"`에서는 `service start`, `service stop`,
`service restart`, `service reload`, `service status`가 `rc-service`를
호출합니다. OpenRC에는 journald에 대응하는 표준 서비스별 로그 조회 방식이
없으므로 `logs`는 명시적인 unsupported backend 오류를 반환합니다.

## 사용법

허용되는 운영 명령:

```sh
sudo /usr/local/sbin/agentctl service restart hermes
sudo /usr/local/sbin/agentctl service start cloudflared
sudo /usr/local/sbin/agentctl service stop tailscale
sudo /usr/local/sbin/agentctl service reload cloudflared
sudo /usr/local/sbin/agentctl service status cloudflared
sudo /usr/local/sbin/agentctl service logs hermes --lines 200 # systemd only
sudo /usr/local/sbin/agentctl telegram notify myriad --chat-id 123456789 --severity critical --message "disk full"
sudo /usr/local/sbin/agentctl discord notify myriad --title "Hermes" --message "service degraded"
sudo /usr/local/sbin/agentctl config check
sudo /usr/local/sbin/agentctl config explain
sudo /usr/local/sbin/agentctl config explain --config-path ./config.toml
agentctl config template --backend openrc --output ./config.toml
sudo /usr/local/sbin/agentctl version
```

다음 계열의 명령은 의도적으로 제공하지 않습니다.

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

`agentctl telegram notify`와 `agentctl discord notify`는 Unix domain socket을 통해
알림 전송을 `agentd`에 위임합니다. Telegram과 Discord 프로파일 credential은 agentctl config가
아니라 `/etc/agentkit/agentd.toml`에 설정합니다. Telegram의 `chat_id` 같은
목적지는 notify 실행 시 파라미터로 전달합니다.

## 확인

config를 검증합니다.

```sh
sudo /usr/local/sbin/agentctl config check
sudo /usr/local/sbin/agentctl config check --config-path ./config.toml
```

검증된 config와 caller별 파생 명령을 출력합니다.

```sh
sudo /usr/local/sbin/agentctl config explain
AGENTCTL_CONFIG_PATH=./config.toml agentctl config explain
```

기본 config 경로는 `/etc/agentkit/agentctl.toml`입니다. 다른 파일을
확인하려면 `config check`와 `config explain`에서 `--config-path`를
지정하거나 `AGENTCTL_CONFIG_PATH` 환경변수를 사용할 수 있습니다.

설정 파일 템플릿은 다음처럼 생성합니다.

```sh
agentctl config template --backend systemd
agentctl config template --backend openrc --output ./config.toml
agentctl config template --backend openrc --output ./config.toml --force
```

생성되는 config는
`crates/agentctl/resources/templates/config.agentctl.toml.template`에서
렌더링됩니다.

`config template`은 관리자 편의 명령이며, 생성되는 sudoers 규칙에는
포함되지 않습니다.

감사 로그는 다음 파일에 기록됩니다.

```text
/var/log/agentctl/audit.log
```
