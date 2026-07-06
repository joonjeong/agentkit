# agentctl

`agentctl` is a config-driven restricted executor for homelab operations.
Agents in the `ops-agent` Unix group can run only the operations listed in
`/etc/agentctl/config.toml` through sudo:

- [Installation and usage (English)](../../docs/agentctl-install-usage.en.md)
- [설치 및 사용법 (한국어)](../../docs/agentctl-install-usage.ko.md)

```sh
sudo /usr/local/sbin/agentctl service restart hermes
sudo /usr/local/sbin/agentctl service start cloudflared
sudo /usr/local/sbin/agentctl service stop tailscale
sudo /usr/local/sbin/agentctl service reload cloudflared
sudo /usr/local/sbin/agentctl service status cloudflared
sudo /usr/local/sbin/agentctl logs hermes --lines 200
sudo /usr/local/sbin/agentctl notify telegram_myriad --severity critical --message "disk full"
sudo /usr/local/sbin/agentctl config check
sudo /usr/local/sbin/agentctl config explain
sudo /usr/local/sbin/agentctl config explain --config-path ./config.toml
agentctl config template --backend openrc --output ./config.toml
```

The binary never exposes `exec`, `shell`, raw `systemctl`, raw `apt`, or
`ansible-playbook` commands. It reads the original caller from `SUDO_USER`,
rejects direct root execution, validates the target name, checks the caller's
allowlist, writes `/var/log/agentctl/audit.log`, and then executes a fixed
service-manager command path without going through a shell. It can also send
allowlisted notifications by delegating to `agentd` over a Unix domain socket,
without exposing provider credentials to the caller. The default backend is
`systemd`; Alpine/OpenRC service control can be enabled with `backend = "openrc"`
in the config defaults.

Build:

```sh
cargo build --release --bin agentctl
```

Bootstrap a host directly from a downloaded or locally copied binary:

```sh
sudo ./agentctl bootstrap --user hermes
```

`config template` prints an example config by default and can write one with
`--output`; it is intended for administrators and is not included in the
generated sudoers rule.

`bootstrap` installs the current executable to `/usr/local/sbin/agentctl`,
creates the `ops-agent` group, adds existing `--user` accounts to the group,
writes the default config, writes sudoers, and writes logrotate config. The
sudoers rule is rendered from
`crates/agentctl/resources/templates/sudoers.ops-agent.template`.

The generated sudoers rule grants only:

```sudoers
%ops-agent ALL=(root) NOPASSWD: /usr/local/sbin/agentctl service start *, /usr/local/sbin/agentctl service stop *, /usr/local/sbin/agentctl service restart *, /usr/local/sbin/agentctl service reload *, /usr/local/sbin/agentctl service status *, /usr/local/sbin/agentctl logs *, /usr/local/sbin/agentctl notify *, /usr/local/sbin/agentctl config check, /usr/local/sbin/agentctl config check *, /usr/local/sbin/agentctl config explain, /usr/local/sbin/agentctl config explain *, /usr/local/sbin/agentctl version
```

Useful bootstrap options:

- `--binary-path /usr/local/sbin/agentctl`
- `--source-binary /path/to/agentctl`
- `--backend systemd`
- `--backend openrc`
- `--sudoers-path /etc/sudoers.d/ops-agent`
- `--config-path /etc/agentctl/config.toml`
- `--audit-log-path /var/log/agentctl/audit.log`
- `--sudo-log-path /var/log/agentctl/sudo.log`
- `--group ops-agent`
