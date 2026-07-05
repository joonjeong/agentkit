# ops-runbook Installation and Usage

`ops-runbook` is a config-driven restricted executor for homelab operations.
It lets automation agents run a narrow set of root operations through sudo,
without granting raw shell, `systemctl`, `apt`, Docker socket, or Ansible access.

Supported service-manager backends:

- `systemd`: service control and logs through `systemctl` and `journalctl`
- `openrc`: service control through `rc-service`; `logs` is not supported

## Security Model

The trusted administrator runs:

```sh
sudo ./ops-runbook bootstrap --user hermes
```

`bootstrap` configures the local host:

- installs the current binary to `/usr/local/sbin/ops-runbook`
- creates the `ops-agent` system group when missing
- adds existing `--user` accounts to `ops-agent`
- creates `/etc/ops-runbook/config.toml` when missing
- writes `/etc/sudoers.d/ops-agent`
- creates log paths under `/var/log/ops-runbook`
- writes `/etc/logrotate.d/ops-runbook`

The generated sudoers rule allows `ops-agent` members to run only the operational
subcommands. It does not allow agents to run `bootstrap`. The rule is rendered
from `crates/ops-runbook/resources/templates/sudoers.ops-agent.template`.

## Build

From the repository root:

```sh
cargo build --release --bin ops-runbook
```

The binary is written to:

```text
target/release/ops-runbook
```

## Bootstrap

Copy or download the binary to the target host, then run:

```sh
sudo ./ops-runbook bootstrap --user hermes
```

Useful options:

```sh
sudo ./ops-runbook bootstrap \
  --source-binary /path/to/ops-runbook \
  --binary-path /usr/local/sbin/ops-runbook \
  --backend systemd \
  --group ops-agent \
  --sudoers-path /etc/sudoers.d/ops-agent \
  --config-path /etc/ops-runbook/config.toml \
  --audit-log-path /var/log/ops-runbook/audit.log \
  --sudo-log-path /var/log/ops-runbook/sudo.log \
  --logrotate-path /etc/logrotate.d/ops-runbook
```

Use `--force-config` to replace an existing config file with the sample config.
Without `--force-config`, an existing config file is preserved.

## Config

The default config format is TOML:

```toml
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[channels.telegram_myriad]
type = "telegram"
chat_id = "123456789"
bot_token_file = "/etc/ops-runbook/secrets/telegram-bot-token"

[channels.discord_myriad]
type = "discord"
webhook_url_file = "/etc/ops-runbook/secrets/discord-webhook-url"

[callers.hermes]
# Allows service start, stop, restart, and reload.
service_control = ["hermes", "cloudflared", "tailscale"]

# Allows service status and logs.
service_read = ["hermes", "cloudflared", "tailscale"]

# Allows sending notifications without exposing provider credentials to the caller.
notify = ["telegram_myriad", "discord_myriad"]
```

The caller is read from `SUDO_USER`. For example, when `hermes` runs:

```sh
sudo /usr/local/sbin/ops-runbook service restart hermes
```

`ops-runbook` checks `callers.hermes.service_control` for `hermes`.

For Alpine/OpenRC hosts, set:

```toml
[defaults]
backend = "openrc"
max_log_lines = 1000
```

With `backend = "openrc"`, `service start`, `service stop`, `service restart`,
`service reload`, and `service status` call `rc-service`. `logs` returns an
explicit unsupported backend error because OpenRC does not define a standard
per-service journald equivalent.

## Usage

Allowed operational commands:

```sh
sudo /usr/local/sbin/ops-runbook service restart hermes
sudo /usr/local/sbin/ops-runbook service start cloudflared
sudo /usr/local/sbin/ops-runbook service stop tailscale
sudo /usr/local/sbin/ops-runbook service reload cloudflared
sudo /usr/local/sbin/ops-runbook service status cloudflared
sudo /usr/local/sbin/ops-runbook logs hermes --lines 200 # systemd only
sudo /usr/local/sbin/ops-runbook notify telegram_myriad --severity critical --message "disk full"
sudo /usr/local/sbin/ops-runbook notify discord_myriad --title "Hermes" --message "service degraded"
sudo /usr/local/sbin/ops-runbook config check
sudo /usr/local/sbin/ops-runbook config explain
sudo /usr/local/sbin/ops-runbook config explain --config-path ./config.toml
ops-runbook config template --backend openrc --output ./config.toml
sudo /usr/local/sbin/ops-runbook version
```

Rejected command families are intentionally absent:

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

Notification channels are configured under `[channels.<name>]` with
`type = "telegram"` or `type = "discord"` and allowlisted per caller with
`notify`. Telegram channels require `chat_id` plus either `bot_token_file` or
`bot_token_env`. Discord channels require either `webhook_url_file` or
`webhook_url_env`.

## Verification

Validate the config:

```sh
sudo /usr/local/sbin/ops-runbook config check
sudo /usr/local/sbin/ops-runbook config check --config-path ./config.toml
```

Dump the validated config and per-caller derived commands:

```sh
sudo /usr/local/sbin/ops-runbook config explain
OPS_RUNBOOK_CONFIG_PATH=./config.toml ops-runbook config explain
```

The default config path is `/etc/ops-runbook/config.toml`. Use
`--config-path` with `config check` or `config explain`, or set
`OPS_RUNBOOK_CONFIG_PATH`, to inspect another config file.

Generate a config template with:

```sh
ops-runbook config template --backend systemd
ops-runbook config template --backend openrc --output ./config.toml
ops-runbook config template --backend openrc --output ./config.toml --force
```

`config template` is an administrator convenience command and is not included in
the generated sudoers rule.

Audit events are written to:

```text
/var/log/ops-runbook/audit.log
```
