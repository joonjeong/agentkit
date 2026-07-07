# agentctl Installation and Usage

`agentctl` is a restricted client for homelab operations. It sends service,
notification, and GitHub App session requests to `agentd` over a Unix domain
socket instead of granting agents raw shell, `systemctl`, `apt`, Docker socket,
or Ansible access.

Supported service-manager backends:

- `systemd`: service control and logs through `systemctl` and `journalctl`
- `openrc`: service control through `rc-service`; `logs` is not supported

## Security Model

The trusted administrator runs:

```sh
sudo ./agentctl bootstrap --user hermes
```

`agentctl` is only the client. `agentd` owns the system-wide provider config,
long-lived secrets, and service-manager execution. For service requests,
`agentd` authorizes the connecting process by Unix-domain-socket peer
credentials (`uid`/`gid`) and then checks the requested service against
`/etc/agentkit/agentd.toml`.

`agentctl bootstrap` configures the client-side pieces:

- installs the current binary to `/usr/local/sbin/agentctl`
- creates the `agent` system group when missing
- adds existing `--user` accounts to `agent`
- creates `/etc/agentkit/agentctl.toml` when missing
- creates log paths under `/var/log/agentctl`
- writes `/etc/logrotate.d/agentctl`

It does not install sudoers rules. Make sure the `agentd` socket permissions and
`[service.callers.<name>]` entries in `/etc/agentkit/agentd.toml` allow the
intended agent users or groups to connect and perform only the required actions.

## Build

From the repository root:

```sh
cargo build --release --bin agentctl
```

The binary is written to:

```text
target/release/agentctl
```

## Bootstrap

Copy or download the binary to the target host, then run:

```sh
sudo ./agentctl bootstrap --user hermes
```

Useful options:

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

Use `--force-config` to replace an existing config file with the sample config.
Without `--force-config`, an existing config file is preserved.

## Config

`agentctl` still has a small local TOML config for `config check`,
`config explain`, `config template`, bootstrap defaults, and audit-log
placement. Service execution policy lives in the `agentd` config, not in this
client config.

The client config format is TOML:

```toml
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[callers.hermes]
# Allows service start, stop, restart, and reload.
service_control = ["hermes", "cloudflared", "tailscale"]

# Allows service status and logs.
service_read = ["hermes", "cloudflared", "tailscale"]
```

For example, this legacy/client-side config says that `hermes` may request
service control and read commands:

```sh
AGENTCTL_CONFIG_PATH=./config.toml agentctl config explain
```

Actual `service` command authorization is performed by `agentd` using peer
credentials and `[service.callers.<name>]` in `/etc/agentkit/agentd.toml`.

For Alpine/OpenRC hosts, set:

```toml
[defaults]
backend = "openrc"
max_log_lines = 1000
```

With `backend = "openrc"` in the `agentd` service config, `service start`,
`service stop`, `service restart`, `service reload`, and `service status` call
`rc-service`. `logs` returns an explicit unsupported backend error because
OpenRC does not define a standard per-service journald equivalent.

## Usage

Allowed operational commands:

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

Rejected command families are intentionally absent:

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

All operational commands use the `agentd` socket at
`/run/agentd/agentd.sock` by default. Override it with `--agentd-socket` or
`AGENTCTL_AGENTD_SOCKET`.

`agentctl telegram notify` and `agentctl discord notify` delegate notification
delivery to `agentd` over its Unix domain socket. Configure Telegram and Discord
profile credentials in `/etc/agentkit/agentd.toml`, not in the agentctl config.
Telegram destinations such as `chat_id` are passed per notify invocation.

## Verification

Validate the config:

```sh
agentctl config check
agentctl config check --config-path ./config.toml
```

Dump the validated config and per-caller derived commands:

```sh
agentctl config explain
AGENTCTL_CONFIG_PATH=./config.toml agentctl config explain
```

The default config path is `/etc/agentkit/agentctl.toml`. Use
`--config-path` with `config check` or `config explain`, or set
`AGENTCTL_CONFIG_PATH`, to inspect another config file.

Generate a config template with:

```sh
agentctl config template --backend systemd
agentctl config template --backend openrc --output ./config.toml
agentctl config template --backend openrc --output ./config.toml --force
```

The generated config is rendered from
`crates/agentctl/resources/templates/config.agentctl.toml.template`.

`config template` is an administrator convenience command.

Audit events are written to:

```text
/var/log/agentctl/audit.log
```
