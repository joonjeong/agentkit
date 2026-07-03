# ops-runbook

`ops-runbook` is a policy-driven restricted executor for homelab operations.
Agents in the `ops-agent` Unix group can run only the operations listed in
`/etc/ops-runbook/policy.toml` through sudo:

```sh
sudo /usr/local/sbin/ops-runbook service restart nginx
sudo /usr/local/sbin/ops-runbook service reload coredns
sudo /usr/local/sbin/ops-runbook service status cloudflared
sudo /usr/local/sbin/ops-runbook logs nginx --lines 200
sudo /usr/local/sbin/ops-runbook policy check
sudo /usr/local/sbin/ops-runbook policy explain service_restart nginx
```

The binary never exposes `exec`, `shell`, raw `systemctl`, raw `apt`, or
`ansible-playbook` commands. It reads the original caller from `SUDO_USER`,
rejects direct root execution, validates the target name, checks the caller's
allowlist, writes `/var/log/ops-runbook/audit.log`, and then executes a fixed
command path without going through a shell.

Build:

```sh
cargo build --release --bin ops-runbook
```

Bootstrap a host directly from a downloaded or locally copied binary:

```sh
sudo ./ops-runbook bootstrap --user hermes --user openclaw
```

`bootstrap` installs the current executable to `/usr/local/sbin/ops-runbook`,
creates the `ops-agent` group, adds existing `--user` accounts to the group,
writes the default policy, writes sudoers, and writes logrotate config.

The generated sudoers rule grants only:

```sudoers
%ops-agent ALL=(root) NOPASSWD: /usr/local/sbin/ops-runbook service restart *, /usr/local/sbin/ops-runbook service reload *, /usr/local/sbin/ops-runbook service status *, /usr/local/sbin/ops-runbook logs *, /usr/local/sbin/ops-runbook policy check, /usr/local/sbin/ops-runbook policy explain *, /usr/local/sbin/ops-runbook version
```

Useful bootstrap options:

- `--binary-path /usr/local/sbin/ops-runbook`
- `--source-binary /path/to/ops-runbook`
- `--sudoers-path /etc/sudoers.d/ops-agent`
- `--policy-path /etc/ops-runbook/policy.toml`
- `--audit-log-path /var/log/ops-runbook/audit.log`
- `--sudo-log-path /var/log/ops-runbook/sudo.log`
- `--group ops-agent`
