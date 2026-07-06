# ops tools

Operational tools packaged as Rust binaries.

## Shape

This repository is a Cargo workspace for small, focused tools:

```text
crates/
  agentd/       local credential broker for agent tools
  ops-session/  GitHub App-backed command session runner
  ops-runbook/  config-driven restricted executor for homelab operations
```

Build individual tools with Cargo:

```sh
cargo build --release --bin agentd
cargo build --release --bin ops-session
cargo build --release --bin ops-runbook
```

## agentd

`agentd` is a local credential broker. It owns system-wide provider profiles and
long-lived secret access, then serves short-lived credentials to local clients
over a Unix domain socket.

For now, `agentd` mints GitHub App installation tokens:

```toml
[github_app]
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
app_id = 123456
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/agentd/secrets/codex-review-github-app.private-key.pem"

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"
```

Validate and run the broker with:

```sh
agentd config check --config-path /etc/agentd/config.toml
agentd serve --config-path /etc/agentd/config.toml --socket-path /run/agentd/agentd.sock
```

## ops-session

`ops-session` runs a child command inside a short-lived GitHub App installation
token context obtained from `agentd`. For now, GitHub App authentication is the
only supported session provider.

```sh
ops-session github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  -- git remote update
```

GitHub App credential material is not read by `ops-session`. Keep private keys
in the system-wide `agentd` config and set `OPS_SESSION_GITHUB_PROFILE` in each
agent's service environment. `ops-session` requests a profile-scoped token from
`agentd`; `agentd` validates requested repositories and permissions against its
configured profile before minting the token.

The child command receives `GH_TOKEN` and `GITHUB_TOKEN`. GitHub App credential
environment variables are removed from the child environment. Shell syntax such
as pipes or redirects requires an explicit shell:

```sh
ops-session github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

Git HTTPS remotes need `--git-credentials` so the child process gets a
temporary Git credential helper:

```sh
ops-session github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  --permission contents=read \
  --git-credentials \
  -- git ls-remote --heads https://github.com/OWNER/REPO.git
```

Validate broker configuration with `agentd config check`.

See [crates/ops-session/README.md](crates/ops-session/README.md).
See [crates/agentd/README.md](crates/agentd/README.md) for the broker config
schema and UDS wire protocol.

## ops-runbook

`ops-runbook` is a separate binary for allowing automation agents such as
Hermes or OpenClaw to perform a narrow set of root operations through sudo:

- [Installation and usage (English)](docs/ops-runbook-install-usage.en.md)
- [설치 및 사용법 (한국어)](docs/ops-runbook-install-usage.ko.md)

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

See [crates/ops-runbook/README.md](crates/ops-runbook/README.md).

## Releases

The release workflow creates HeadVer-tagged GitHub releases with
`agentd` and `ops-session` binaries for `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, and `aarch64-apple-darwin`. Linux assets are
statically linked musl binaries so they do not depend on the host system's
glibc version.

The weekly release workflow runs every Sunday at 10:00 KST and creates a
[HeadVer](https://github.com/line/headver) release from the default branch. Until
the project is ready for a stable head value, automated releases use head `0` in
the form `v0.<yearweek>.<build>`. The weekly workflow only calculates the
HeadVer tag and delegates builds and immutable-compatible release publishing to
the release workflow. If there are no commits after the latest merged `v*`
release tag, the weekly workflow skips the release.

HeadVer values are calculated by `scripts/headver`:

```sh
scripts/headver --head 0 --build 123 --timezone Asia/Seoul
```

Release metadata is calculated by `scripts/release-metadata`, and weekly release
change detection is calculated by `scripts/weekly-release-changes`.
