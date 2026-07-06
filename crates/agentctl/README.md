# agentctl

`agentctl` is a restricted client for homelab operations. Service control is
delegated to `agentd` over a Unix domain socket, where peer credentials and the
system-wide agentd config decide what may run.

- [Installation and usage (English)](../../docs/agentctl-install-usage.en.md)
- [설치 및 사용법 (한국어)](../../docs/agentctl-install-usage.ko.md)

```sh
agentctl service restart hermes
agentctl service start cloudflared
agentctl service stop tailscale
agentctl service reload cloudflared
agentctl service status cloudflared
agentctl service logs hermes --lines 200
sudo /usr/local/sbin/agentctl telegram notify myriad --chat-id 123456789 --severity critical --message "disk full"
sudo /usr/local/sbin/agentctl config check
sudo /usr/local/sbin/agentctl config explain
sudo /usr/local/sbin/agentctl config explain --config-path ./config.toml
agentctl config template --backend openrc --output ./config.toml
```

The binary never exposes `exec`, `shell`, raw `systemctl`, raw `apt`, or
`ansible-playbook` commands. Service requests are sent to `agentd`, which
validates the peer uid/gid, checks its service allowlist, and executes a fixed
service-manager command path without going through a shell. Notifications are
also delegated to `agentd` without exposing provider credentials to the caller.

Build:

```sh
cargo build --release --bin agentctl
```

Bootstrap a host directly from a downloaded or locally copied binary:

```sh
sudo ./agentctl bootstrap --user hermes
```

`config template` prints an example config by default and can write one with
`--output`; it is intended for administrators.
The config text is rendered from
`crates/agentctl/resources/templates/config.agentctl.toml.template`.

`bootstrap` installs the current executable to `/usr/local/sbin/agentctl`,
creates the `agent` group, adds existing `--user` accounts to the group,
writes the default config, and writes logrotate config. Service control does
not require sudo for `agentctl`; access is gated by the agentd socket and
agentd peer-credential policy.

Useful bootstrap options:

- `--binary-path /usr/local/sbin/agentctl`
- `--source-binary /path/to/agentctl`
- `--backend systemd`
- `--backend openrc`
- `--config-path /etc/agentkit/agentctl.toml`
- `--audit-log-path /var/log/agentctl/audit.log`
- `--group agent`
