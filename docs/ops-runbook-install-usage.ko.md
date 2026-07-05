# ops-runbook 설치 및 사용법

`ops-runbook`은 홈랩 운영 작업을 위한 policy 기반 제한 실행기입니다.
자동화 에이전트가 sudo를 통해 제한된 root 작업만 실행하게 하며, raw shell,
`systemctl`, `apt`, Docker socket, Ansible 접근 권한은 주지 않습니다.

지원하는 서비스 관리자 backend:

- `systemd`: `systemctl`과 `journalctl`로 서비스 제어 및 로그 조회
- `openrc`: `rc-service`로 서비스 제어. `logs`는 지원하지 않음

## 보안 모델

신뢰할 수 있는 관리자가 다음 명령을 실행합니다.

```sh
sudo ./ops-runbook bootstrap --user hermes
```

`bootstrap`은 대상 호스트를 구성합니다.

- 현재 바이너리를 `/usr/local/sbin/ops-runbook`에 설치
- `ops-agent` 시스템 그룹이 없으면 생성
- `--user`로 지정한 기존 계정을 `ops-agent` 그룹에 추가
- `/etc/ops-runbook/policy.toml`이 없으면 기본 policy 생성
- `/etc/sudoers.d/ops-agent` 생성
- `/var/log/ops-runbook` 아래 로그 경로 생성
- `/etc/logrotate.d/ops-runbook` 생성

생성되는 sudoers 규칙은 `ops-agent` 멤버에게 운영 서브커맨드만 허용합니다.
에이전트가 `bootstrap`을 실행할 수는 없습니다. 이 규칙은
`crates/ops-runbook/resources/templates/sudoers.ops-agent.template`에서 렌더링됩니다.

## 빌드

저장소 루트에서 실행합니다.

```sh
cargo build --release --bin ops-runbook
```

바이너리는 다음 위치에 생성됩니다.

```text
target/release/ops-runbook
```

## Bootstrap

바이너리를 대상 호스트에 복사하거나 다운로드한 뒤 실행합니다.

```sh
sudo ./ops-runbook bootstrap --user hermes
```

주요 옵션:

```sh
sudo ./ops-runbook bootstrap \
  --source-binary /path/to/ops-runbook \
  --binary-path /usr/local/sbin/ops-runbook \
  --backend systemd \
  --group ops-agent \
  --sudoers-path /etc/sudoers.d/ops-agent \
  --policy-path /etc/ops-runbook/policy.toml \
  --audit-log-path /var/log/ops-runbook/audit.log \
  --sudo-log-path /var/log/ops-runbook/sudo.log \
  --logrotate-path /etc/logrotate.d/ops-runbook
```

기존 policy 파일을 샘플 policy로 교체하려면 `--force-policy`를 사용합니다.
`--force-policy`가 없으면 기존 policy 파일은 보존됩니다.

## Policy

기본 policy 형식은 TOML입니다.

```toml
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[notifications.telegram.ops]
chat_id = "123456789"
bot_token_file = "/etc/ops-runbook/secrets/telegram-bot-token"

[notifications.discord.ops]
webhook_url_file = "/etc/ops-runbook/secrets/discord-webhook-url"

[callers.hermes]
# service start, stop, restart, reload 허용
service_control = ["hermes", "cloudflared", "tailscale"]

# service status와 logs 허용
service_read = ["hermes", "cloudflared", "tailscale"]

# provider credential을 caller에게 노출하지 않고 알람 전송 허용
alarms = ["telegram.ops", "discord.ops"]
```

호출자는 `SUDO_USER`에서 읽습니다. 예를 들어 `hermes`가 다음을 실행하면:

```sh
sudo /usr/local/sbin/ops-runbook service restart hermes
```

`ops-runbook`은 `callers.hermes.service_control`에 `hermes`가 있는지 확인합니다.

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
sudo /usr/local/sbin/ops-runbook service restart hermes
sudo /usr/local/sbin/ops-runbook service start cloudflared
sudo /usr/local/sbin/ops-runbook service stop tailscale
sudo /usr/local/sbin/ops-runbook service reload cloudflared
sudo /usr/local/sbin/ops-runbook service status cloudflared
sudo /usr/local/sbin/ops-runbook logs hermes --lines 200 # systemd only
sudo /usr/local/sbin/ops-runbook alarm send telegram.ops --severity critical --message "disk full"
sudo /usr/local/sbin/ops-runbook alarm send discord.ops --title "Hermes" --message "service degraded"
sudo /usr/local/sbin/ops-runbook policy check
sudo /usr/local/sbin/ops-runbook policy explain
sudo /usr/local/sbin/ops-runbook policy explain --policy-path ./policy.toml
ops-runbook policy template --backend openrc --output ./policy.toml
sudo /usr/local/sbin/ops-runbook version
```

다음 계열의 명령은 의도적으로 제공하지 않습니다.

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

알람 목적지는 `[notifications.telegram.<name>]` 또는
`[notifications.discord.<name>]` 아래에 설정하고, caller별 `alarms`로
허용합니다. Telegram 목적지는 `chat_id`와 `bot_token_file` 또는
`bot_token_env`가 필요합니다. Discord 목적지는 `webhook_url_file` 또는
`webhook_url_env`가 필요합니다.

## 확인

policy를 검증합니다.

```sh
sudo /usr/local/sbin/ops-runbook policy check
sudo /usr/local/sbin/ops-runbook policy check --policy-path ./policy.toml
```

검증된 policy와 caller별 파생 명령을 출력합니다.

```sh
sudo /usr/local/sbin/ops-runbook policy explain
OPS_RUNBOOK_POLICY_PATH=./policy.toml ops-runbook policy explain
```

기본 policy 경로는 `/etc/ops-runbook/policy.toml`입니다. 다른 파일을
확인하려면 `policy check`와 `policy explain`에서 `--policy-path`를
지정하거나 `OPS_RUNBOOK_POLICY_PATH` 환경변수를 사용할 수 있습니다.

설정 파일 템플릿은 다음처럼 생성합니다.

```sh
ops-runbook policy template --backend systemd
ops-runbook policy template --backend openrc --output ./policy.toml
ops-runbook policy template --backend openrc --output ./policy.toml --force
```

`policy template`은 관리자 편의 명령이며, 생성되는 sudoers 규칙에는
포함되지 않습니다.

감사 로그는 다음 파일에 기록됩니다.

```text
/var/log/ops-runbook/audit.log
```
