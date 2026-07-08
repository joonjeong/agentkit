# agentctl 설치 및 사용법

`agentctl`은 홈랩 운영 작업을 위한 제한된 클라이언트입니다. 자동화
에이전트에게 raw shell, `systemctl`, `apt`, Docker socket, Ansible 접근
권한을 주지 않고, 서비스/알림/GitHub App session 요청을 Unix domain socket을
통해 `agentd`로 보냅니다.

지원하는 서비스 관리자 backend:

- `systemd`: `systemctl`과 `journalctl`로 서비스 제어 및 로그 조회
- `openrc`: `rc-service`로 서비스 제어. `logs`는 지원하지 않음

## Quickstart

이 예시는 systemd 호스트에서 `hermes` agent 사용자가 `hermes`와
`cloudflared` 서비스만 재시작하고 조회할 수 있게 설정합니다.

broker와 client를 빌드합니다.

```sh
cargo build --release --bin agentd --bin agentctl
```

`agentd`, `agentctl`, 기본 service/client 파일을 설치합니다.

```sh
sudo target/release/agentd bootstrap
sudo target/release/agentctl bootstrap --user hermes
```

대상 호스트에 맞게 `/etc/agentkit/agentd.toml`의 service 섹션을 수정합니다.
agent 계정의 실제 사용자와 그룹 이름을 사용합니다.

```sh
sudo editor /etc/agentkit/agentd.toml
```

```toml
[service]
backend = "systemd"
max_log_lines = 1000

[service.hermes]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]

[service.cloudflared]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]
```

broker 설정을 검증하고 시작합니다.

```sh
sudo /usr/local/sbin/agentd config check
sudo systemctl daemon-reload
sudo systemctl enable --now agentd
```

`agentd`는 `/run/agentd/agentd.sock`을 mode `0660`으로 만듭니다. agent
사용자가 이 socket에 접속할 수 있어야 합니다. 빠른 수동 확인은 다음처럼
할 수 있습니다.

```sh
sudo chgrp agent /run/agentd/agentd.sock
```

socket이 restart 때 다시 만들어진다면 같은 ownership 정책을 service
manager에 영구 반영하세요.

agent 사용자로 서비스 작업을 실행합니다.

```sh
sudo -u hermes /usr/local/sbin/agentctl service status hermes
sudo -u hermes /usr/local/sbin/agentctl service restart cloudflared
sudo -u hermes /usr/local/sbin/agentctl service logs hermes --lines 100
```

알림을 쓰려면 `agentd.toml`에 Telegram 또는 Discord profile을 추가한 뒤
`agentctl`로 provider를 호출합니다.

```sh
sudo -u hermes /usr/local/sbin/agentctl telegram notify myriad \
  --chat-id 123456789 \
  --severity warning \
  --title "Hermes" \
  --message "deploy finished"
```

GitHub App 작업은 `agentd.toml`에 `[github_app]` profile을 추가한 뒤,
child command를 짧은 수명의 token context 안에서 실행합니다.

```sh
sudo -u hermes /usr/local/sbin/agentctl github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  -- gh pr view 123 --repo OWNER/REPO
```

## 보안 모델

신뢰할 수 있는 관리자가 다음 명령을 실행합니다.

```sh
sudo ./agentctl bootstrap --user hermes
```

`agentctl`은 클라이언트일 뿐입니다. system-wide provider config, 장기
secret, 서비스 관리자 실행은 `agentd`가 소유합니다. 서비스 요청에서
`agentd`는 Unix-domain-socket peer credential(`uid`/`gid`)로 접속한
프로세스를 식별하고, `/etc/agentkit/agentd.toml`의 service allowlist와
대조해 허용 여부를 결정합니다.

`agentctl bootstrap`은 클라이언트 측 구성을 준비합니다.

- 현재 바이너리를 `/usr/local/sbin/agentctl`에 설치
- `agent` 시스템 그룹이 없으면 생성
- `--user`로 지정한 기존 계정을 `agent` 그룹에 추가
- `/etc/agentkit/agentctl.toml`이 없으면 기본 config 생성
- `/var/log/agentctl` 아래 로그 경로 생성
- `/etc/logrotate.d/agentctl` 생성

sudoers 규칙은 설치하지 않습니다. 의도한 agent 사용자나 그룹이 필요한
작업만 수행할 수 있도록 `agentd` socket 권한과
`/etc/agentkit/agentd.toml`의 `[service.<target>]` 항목을 설정해야
합니다.

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
  --config-path /etc/agentkit/agentctl.toml \
  --audit-log-path /var/log/agentctl/audit.log \
  --logrotate-path /etc/logrotate.d/agentctl
```

기존 config 파일을 샘플 config로 교체하려면 `--force-config`를 사용합니다.
`--force-config`가 없으면 기존 config 파일은 보존됩니다.

## Config

`agentctl`에는 `config check`, `config explain`, `config template`,
bootstrap 기본값, audit log 위치를 위한 작은 local TOML config가 남아
있습니다. 서비스 실행 정책은 이 client config가 아니라 `agentd` config에
있습니다.

client config 형식은 TOML입니다.

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

예를 들어 이 legacy/client-side config는 `hermes`가 service control/read
명령을 요청할 수 있음을 보여줍니다.

```sh
AGENTCTL_CONFIG_PATH=./config.toml agentctl config explain
```

실제 `service` 명령의 권한 검사는 `agentd`가 peer credential과
`/etc/agentkit/agentd.toml`의 `[service.<target>]` 항목으로 수행합니다.

Alpine/OpenRC 호스트에서는 다음처럼 설정합니다.

```toml
[defaults]
backend = "openrc"
max_log_lines = 1000
```

`agentd` service config에서 `backend = "openrc"`를 설정하면
`service start`, `service stop`, `service restart`, `service reload`,
`service status`가 `rc-service`를 호출합니다. OpenRC에는 journald에
대응하는 표준 서비스별 로그 조회 방식이 없으므로 `logs`는 명시적인
unsupported backend 오류를 반환합니다.

## 사용법

허용되는 운영 명령:

```sh
agentctl service restart hermes
agentctl service start cloudflared
agentctl service stop tailscale
agentctl service reload cloudflared
agentctl service status cloudflared
agentctl service logs hermes --lines 200 # systemd only
agentctl telegram notify myriad --chat-id 123456789 --severity critical --message "disk full"
agentctl discord notify myriad --title "Hermes" --message "service degraded"
agentctl config check
agentctl config explain
agentctl config explain --config-path ./config.toml
agentctl config template --backend openrc --output ./config.toml
agentctl version
```

다음 계열의 명령은 의도적으로 제공하지 않습니다.

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

모든 운영 명령은 기본적으로 `/run/agentd/agentd.sock`의 `agentd` socket을
사용합니다. `--agentd-socket` 또는 `AGENTCTL_AGENTD_SOCKET`으로 바꿀 수
있습니다.

`agentctl telegram notify`와 `agentctl discord notify`는 Unix domain socket을
통해 알림 전송을 `agentd`에 위임합니다. Telegram과 Discord 프로파일
credential은 agentctl config가 아니라 `/etc/agentkit/agentd.toml`에
설정합니다. Telegram의 `chat_id` 같은 목적지는 notify 실행 시 파라미터로
전달합니다.

## 확인

config를 검증합니다.

```sh
agentctl config check
agentctl config check --config-path ./config.toml
```

검증된 config와 caller별 파생 명령을 출력합니다.

```sh
agentctl config explain
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

`config template`은 관리자 편의 명령입니다.

감사 로그는 다음 파일에 기록됩니다.

```text
/var/log/agentctl/audit.log
```
