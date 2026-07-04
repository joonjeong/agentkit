# ops-runbook Installation and Usage

`ops-runbook` is a policy-driven restricted executor for homelab operations.
It lets automation agents run a narrow set of root operations through sudo,
without granting raw shell, `systemctl`, `apt`, Docker socket, or Ansible access.

Supported service-manager backends:

- `systemd`: service control and logs through `systemctl` and `journalctl`
- `openrc`: service control through `rc-service`; `logs` is not supported

## Security Model

The trusted administrator runs:

```sh
sudo ./ops-runbook bootstrap --user hermes --user openclaw
```

`bootstrap` configures the local host:

- installs the current binary to `/usr/local/sbin/ops-runbook`
- creates the `ops-agent` system group when missing
- adds existing `--user` accounts to `ops-agent`
- creates `/etc/ops-runbook/policy.toml` when missing
- writes `/etc/sudoers.d/ops-agent`
- creates log paths under `/var/log/ops-runbook`
- writes `/etc/logrotate.d/ops-runbook`

The generated sudoers rule allows `ops-agent` members to run only the operational
subcommands. It does not allow agents to run `bootstrap`.

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
sudo ./ops-runbook bootstrap --user hermes --user openclaw
```

Useful options:

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

Use `--force-policy` to replace an existing policy file with the sample policy.
Without `--force-policy`, an existing policy file is preserved.

## Policy

The default policy format is TOML:

```toml
version = 1

[defaults]
backend = "systemd"
max_log_lines = 1000

[callers.hermes]
service_start = ["nginx", "coredns", "cloudflared"]
service_stop = ["nginx", "coredns", "cloudflared"]
service_restart = ["nginx", "coredns", "cloudflared"]
service_reload = ["nginx", "coredns"]
service_status = ["nginx", "coredns", "cloudflared"]
logs = ["nginx", "coredns", "cloudflared"]
```

The caller is read from `SUDO_USER`. For example, when `hermes` runs:

```sh
sudo /usr/local/sbin/ops-runbook service restart nginx
```

`ops-runbook` checks `callers.hermes.service_restart` for `nginx`.

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
sudo /usr/local/sbin/ops-runbook service restart nginx
sudo /usr/local/sbin/ops-runbook service start nginx
sudo /usr/local/sbin/ops-runbook service stop nginx
sudo /usr/local/sbin/ops-runbook service reload coredns
sudo /usr/local/sbin/ops-runbook service status cloudflared
sudo /usr/local/sbin/ops-runbook logs nginx --lines 200 # systemd only
sudo /usr/local/sbin/ops-runbook policy check
sudo /usr/local/sbin/ops-runbook policy explain service_restart nginx
sudo /usr/local/sbin/ops-runbook version
```

Rejected command families are intentionally absent:

- `exec`
- `shell`
- raw `systemctl`
- raw `apt`
- `ansible-playbook`

## Verification

Validate the policy:

```sh
sudo /usr/local/sbin/ops-runbook policy check
```

Explain a decision for the current sudo caller:

```sh
sudo /usr/local/sbin/ops-runbook policy explain service_restart nginx
```

Audit events are written to:

```text
/var/log/ops-runbook/audit.log
```
