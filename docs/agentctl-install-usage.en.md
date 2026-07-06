# agentctl Installation and Usage

`agentctl` is a config-driven restricted executor for homelab operations.
It lets automation agents run a narrow set of root operations through sudo,
without granting raw shell, `systemctl`, `apt`, Docker socket, or Ansible access.

Supported service-manager backends:

- `systemd`: service control and logs through `systemctl` and `journalctl`
- `openrc`: service control through `rc-service`; `logs` is not supported

## Security Model

The trusted administrator runs:

```sh
sudo ./agentctl bootstrap --user hermes
```

`bootstrap` configures the local host:

- installs the current binary to `/usr/local/sbin/agentctl`
- creates the `ops-agent` system group when missing
- adds existing `--user` accounts to `ops-agent`
- creates `/etc/agentctl/config.toml` when missing
- writes `/etc/sudoers.d/ops-agent`
- creates log paths under `/var/log/agentctl`
- writes `/etc/logrotate.d/agentctl`

The generated sudoers rule allows `ops-agent` members to run only the operational
subcommands. It does not allow agents to run `bootstrap`. The rule is rendered
from `crates/agentctl/resources/templates/sudoers.ops-agent.template`.

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
  --group ops-agent \
  --sudoers-path /etc/sudoers.d/ops-agent \
  --config-path /etc/agentctl/config.toml \
  --audit-log-path /var/log/agentctl/audit.log \
  --sudo-log-path /var/log/agentctl/sudo.log \
  --logrotate-path /etc/logrotate.d/agentctl
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

[callers.hermes]
# Allows service start, stop, restart, and reload.
service_control = ["hermes", "cloudflared", "tailscale"]

# Allows service status and logs.
service_read = ["hermes", "cloudflared", "tailscale"]
```

The caller is read from `SUDO_USER`. For example, when `hermes` runs:

```sh
sudo /usr/local/sbin/agentctl service restart hermes
```

`agentctl` checks `callers.hermes.service_control` for `hermes`.

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
sudo /usr/local/sbin/agentctl service restart hermes
sudo /usr/local/sbin/agentctl service start cloudflared
sudo /usr/local/sbin/agentctl service stop tailscale
sudo /usr/local/sbin/agentctl service reload cloudflared
sudo /usr/local/sbin/agentctl service status cloudflared
sudo /usr/local/sbin/agentctl logs hermes --lines 200 # systemd only
sudo /usr/local/sbin/agentctl notify telegram_myriad --severity critical --message "disk full"
sudo /usr/local/sbin/agentctl notify discord_myriad --title "Hermes" --message "service degraded"
sudo /usr/local/sbin/agentctl config check
sudo /usr/local/sbin/agentctl config explain
sudo /usr/local/sbin/agentctl config explain --config-path ./config.toml
agentctl config template --backend openrc --output ./config.toml
sudo /usr/local/sbin/agentctl version
```

Rejected command families are intentionally absent:

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

`agentctl notify` delegates notification delivery to `agentd` over its Unix
domain socket. Configure Telegram and Discord channel credentials and allowed
callers in `/etc/agentd/config.toml`, not in the agentctl config.

## Verification

Validate the config:

```sh
sudo /usr/local/sbin/agentctl config check
sudo /usr/local/sbin/agentctl config check --config-path ./config.toml
```

Dump the validated config and per-caller derived commands:

```sh
sudo /usr/local/sbin/agentctl config explain
AGENTCTL_CONFIG_PATH=./config.toml agentctl config explain
```

The default config path is `/etc/agentctl/config.toml`. Use
`--config-path` with `config check` or `config explain`, or set
`AGENTCTL_CONFIG_PATH`, to inspect another config file.

Generate a config template with:

```sh
agentctl config template --backend systemd
agentctl config template --backend openrc --output ./config.toml
agentctl config template --backend openrc --output ./config.toml --force
```

`config template` is an administrator convenience command and is not included in
the generated sudoers rule.

Audit events are written to:

```text
/var/log/agentctl/audit.log
```
