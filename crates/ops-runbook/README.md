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

Deployment scaffolding lives under `ansible/`. The sudoers template grants only:

```sudoers
%ops-agent ALL=(root) NOPASSWD: /usr/local/sbin/ops-runbook
```
