# ops-runbook

`ops-runbook` is a config-driven restricted executor for homelab operations.
Agents in the `ops-agent` Unix group can run only the operations listed in
`/etc/ops-runbook/config.toml` through sudo:

- [Installation and usage (English)](../../docs/ops-runbook-install-usage.en.md)
- [설치 및 사용법 (한국어)](../../docs/ops-runbook-install-usage.ko.md)

```sh
sudo /usr/local/sbin/ops-runbook service restart hermes
sudo /usr/local/sbin/ops-runbook service start cloudflared
sudo /usr/local/sbin/ops-runbook service stop tailscale
sudo /usr/local/sbin/ops-runbook service reload cloudflared
sudo /usr/local/sbin/ops-runbook service status cloudflared
sudo /usr/local/sbin/ops-runbook logs hermes --lines 200
sudo /usr/local/sbin/ops-runbook notify telegram_myriad --severity critical --message "disk full"
sudo /usr/local/sbin/ops-runbook config check
sudo /usr/local/sbin/ops-runbook config explain
sudo /usr/local/sbin/ops-runbook config explain --config-path ./config.toml
ops-runbook config template --backend openrc --output ./config.toml
```

The binary never exposes `exec`, `shell`, raw `systemctl`, raw `apt`, or
`ansible-playbook` commands. It reads the original caller from `SUDO_USER`,
rejects direct root execution, validates the target name, checks the caller's
allowlist, writes `/var/log/ops-runbook/audit.log`, and then executes a fixed
service-manager command path without going through a shell. It can also send
allowlisted notifications to Telegram or Discord without exposing provider
credentials to the caller. The default backend is `systemd`; Alpine/OpenRC
service control can be enabled with `backend = "openrc"` in the config defaults.

Build:

```sh
cargo build --release --bin ops-runbook
```

Bootstrap a host directly from a downloaded or locally copied binary:

```sh
sudo ./ops-runbook bootstrap --user hermes
```

`config template` prints an example config by default and can write one with
`--output`; it is intended for administrators and is not included in the
generated sudoers rule.

`bootstrap` installs the current executable to `/usr/local/sbin/ops-runbook`,
creates the `ops-agent` group, adds existing `--user` accounts to the group,
writes the default config, writes sudoers, and writes logrotate config. The
sudoers rule is rendered from
`crates/ops-runbook/resources/templates/sudoers.ops-agent.template`.

The generated sudoers rule grants only:

```sudoers
%ops-agent ALL=(root) NOPASSWD: /usr/local/sbin/ops-runbook service start *, /usr/local/sbin/ops-runbook service stop *, /usr/local/sbin/ops-runbook service restart *, /usr/local/sbin/ops-runbook service reload *, /usr/local/sbin/ops-runbook service status *, /usr/local/sbin/ops-runbook logs *, /usr/local/sbin/ops-runbook notify *, /usr/local/sbin/ops-runbook config check, /usr/local/sbin/ops-runbook config check *, /usr/local/sbin/ops-runbook config explain, /usr/local/sbin/ops-runbook config explain *, /usr/local/sbin/ops-runbook version
```

Useful bootstrap options:

- `--binary-path /usr/local/sbin/ops-runbook`
- `--source-binary /path/to/ops-runbook`
- `--backend systemd`
- `--backend openrc`
- `--sudoers-path /etc/sudoers.d/ops-agent`
- `--config-path /etc/ops-runbook/config.toml`
- `--audit-log-path /var/log/ops-runbook/audit.log`
- `--sudo-log-path /var/log/ops-runbook/sudo.log`
- `--group ops-agent`
