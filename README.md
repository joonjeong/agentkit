# ops tools

Operational tools packaged as Rust binaries.

## Shape

This repository is a Cargo workspace for small, focused tools:

```text
crates/
  ops-session/  GitHub App-backed command session runner
  ops-runbook/  policy-driven restricted executor for homelab operations
```

Build individual tools with Cargo:

```sh
cargo build --release --bin ops-session
cargo build --release --bin ops-runbook
```

## ops-session

`ops-session` runs a child command inside a short-lived GitHub App installation
token context. For now, GitHub App authentication is the only supported session
provider.

```sh
ops-session github \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  -- git remote update
```

GitHub App credential material is read from `/etc/ops-session/github.toml` by
default. Set `private_key_path` there; do not pass private key paths or private
key contents through CLI arguments or environment variables.

The child command receives `GH_TOKEN` and `GITHUB_TOKEN`. GitHub App credential
environment variables are removed from the child environment. Shell syntax such
as pipes or redirects requires an explicit shell:

```sh
ops-session github \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

Git HTTPS remotes need `--git-credentials` so the child process gets a
temporary Git credential helper:

```sh
ops-session github \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --permission contents=read \
  --git-credentials \
  -- git ls-remote --heads https://github.com/OWNER/REPO.git
```

Diagnostic token output is available through `ops-session github app-auth`, but
normal agent workflows should use `ops-session github ... -- COMMAND`.

See [crates/ops-session/README.md](crates/ops-session/README.md).

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
sudo /usr/local/sbin/ops-runbook policy check
sudo /usr/local/sbin/ops-runbook policy explain
sudo /usr/local/sbin/ops-runbook policy explain --policy-path ./policy.toml
ops-runbook policy template --backend openrc --output ./policy.toml
```

See [crates/ops-runbook/README.md](crates/ops-runbook/README.md).

## Releases

The release workflow creates HeadVer-tagged GitHub releases and uploads
`ops-session` binaries for `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, and `aarch64-apple-darwin`. Linux assets are
statically linked musl binaries so they do not depend on the host system's
glibc version.

The weekly release workflow runs every Sunday at 10:00 KST and creates a
[HeadVer](https://github.com/line/headver) release from the default branch. Until
the project is ready for a stable head value, automated releases use head `0` in
the form `v0.<yearweek>.<build>`. The weekly workflow only calculates the
HeadVer tag and delegates release creation, builds, and asset uploads to the
release workflow. If there are no commits after the latest merged `v*` release
tag, the weekly workflow skips the release.

HeadVer values are calculated by `scripts/headver`:

```sh
scripts/headver --head 0 --build 123 --timezone Asia/Seoul
```

Release metadata is calculated by `scripts/release-metadata`, and weekly release
change detection is calculated by `scripts/weekly-release-changes`.
