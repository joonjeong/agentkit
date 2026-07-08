# agentkit

Operational agent tools packaged as Rust binaries.

## Shape

`agentkit` is a Cargo workspace for small, focused tools:

```text
crates/
  agentd/       local service broker for agent tools
  agentctl/     config-driven restricted executor for homelab operations
```

## Quickstart

For a host agent such as Hermes, install the released `agentd` broker and
`agentctl` client from GitHub. Pick the Linux asset for the host architecture:

```sh
repo="joonjeong/agentkit"
version="v0.YYWW.BUILD"
target="x86_64-unknown-linux-musl" # or aarch64-unknown-linux-musl

gh release download "${version}" --repo "${repo}" --pattern "agentd-${target}-*"
gh release download "${version}" --repo "${repo}" --pattern "agentctl-${target}-*"

install -m 0755 "agentd-${target}-${version#v}" ./agentd
install -m 0755 "agentctl-${target}-${version#v}" ./agentctl
```

Then bootstrap the system-wide broker and the client entrypoint for the agent
user:

```sh
sudo ./agentd bootstrap
sudo ./agentctl bootstrap --user hermes
```

In `/etc/agentkit/agentd.toml`, allow the agent user or group to operate only
the intended services. This is the core service gate: `viewer` identities may
read service state and logs; `operator` identities may also start, stop,
restart, and reload the service.

```toml
[service]
backend = "systemd"
max_log_lines = 1000

[service.hermes]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]

[service.cloudflared]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]
```

After validating and starting `agentd`, make the broker socket available to the
agent group. Persist that ownership policy in the service manager if the socket
is recreated on restart.

```sh
sudo /usr/local/sbin/agentd config check
sudo systemctl daemon-reload
sudo systemctl enable --now agentd
sudo chgrp agent /run/agentd/agentd.sock
```

The agent can then ask the gate to manipulate services without direct shell,
sudo, or service-manager access:

```sh
sudo -u hermes /usr/local/sbin/agentctl service status hermes
sudo -u hermes /usr/local/sbin/agentctl service restart cloudflared
```

The same broker can gate other host capabilities. Configure provider profiles
once in `/etc/agentkit/agentd.toml`, keep long-lived secrets on the host, and
let the agent request scoped operations through `agentctl`:

```sh
sudo -u hermes /usr/local/sbin/agentctl github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  -- gh pr view 123 --repo OWNER/REPO

sudo -u hermes /usr/local/sbin/agentctl telegram notify myriad \
  --chat-id 123456789 \
  --severity critical \
  --message "service restart failed"

sudo -u hermes /usr/local/sbin/agentctl discord notify myriad \
  --severity warning \
  --message "cloudflared is degraded"
```

If an agent cannot install system-wide files directly, have an administrator or
provisioning tool run the two bootstrap commands above, or place the downloaded
binaries under `/usr/local/sbin`, write `/etc/agentkit/agentd.toml`, and manage
the `agentd` service/socket with the host's service manager. The runtime agent
only needs permission to execute `agentctl` and connect to the broker socket.

For local development, build the same binaries with Cargo:

```sh
cargo build --release --bin agentd --bin agentctl
```

## agentd: Host Gate

`agentd` is a local service broker. It owns system-wide provider profiles and
long-lived secret access, then serves short-lived credentials and notification
delivery to local clients over a Unix domain socket.

`agentd` is the trusted side of agentkit. It checks Unix peer credentials for
local callers, enforces service `viewer` and `operator` policy, runs fixed
service-manager commands without a shell, mints scoped GitHub App installation
tokens, and sends Telegram or Discord notifications without exposing provider
secrets to the caller.

Example broker config:

```toml
[github_app]
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
app_id = 123456
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/agentkit/secrets/codex-review-github-app.private-key.pem"

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"

[telegram.profiles.myriad.token]
type = "file"
path = "/etc/agentkit/secrets/telegram-bot-token"

[discord.profiles.myriad.webhook]
type = "file"
path = "/etc/agentkit/secrets/discord-webhook-url"

[service]
backend = "systemd"
max_log_lines = 1000

[service.hermes]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]
```

Validate and run the broker with:

```sh
agentd config check --config-path /etc/agentkit/agentd.toml
agentd serve --config-path /etc/agentkit/agentd.toml --socket-path /run/agentd/agentd.sock
```

## agentctl GitHub App Sessions

`agentctl github-app run` runs a child command inside a short-lived GitHub App installation
token context obtained from `agentd`. For now, GitHub App authentication is the
only supported session provider.

```sh
agentctl github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  -- git remote update
```

GitHub App credential material is not read by `agentctl`. Keep private keys
in the system-wide `agentd` config and set `AGENTCTL_GITHUB_PROFILE` in each
agent's service environment. `agentctl` requests a profile-scoped token from
`agentd`; `agentd` validates requested repositories and permissions against its
configured profile before minting the token.

The child command receives `GH_TOKEN` and `GITHUB_TOKEN`. GitHub App credential
environment variables are removed from the child environment. Shell syntax such
as pipes or redirects requires an explicit shell:

```sh
agentctl github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

Git HTTPS remotes need `--git-credentials` so the child process gets a
temporary Git credential helper:

```sh
agentctl github-app run \
  --profile codex-review \
  --repo OWNER/REPO \
  --permission contents=read \
  --git-credentials \
  -- git ls-remote --heads https://github.com/OWNER/REPO.git
```

Validate broker configuration with `agentd config check`.

See [crates/agentd/README.md](crates/agentd/README.md) for the broker config
schema and UDS wire protocol.

## agentctl: Agent Client

`agentctl` is the untrusted-side client for automation agents such as Hermes or
OpenClaw. It does not read provider secrets and it does not expose raw shell,
`systemctl`, package-manager, or Ansible execution. It sends typed requests to
`agentd`, which decides whether the local caller is allowed to perform them.

- [Installation and usage (English)](docs/agentctl-install-usage.en.md)
- [설치 및 사용법 (한국어)](docs/agentctl-install-usage.ko.md)

```sh
agentctl service restart hermes
agentctl service start cloudflared
agentctl service stop tailscale
agentctl service reload cloudflared
agentctl service status cloudflared
agentctl service logs hermes --lines 200
agentctl telegram notify myriad --chat-id 123456789 --severity critical --message "disk full"
agentctl discord notify myriad --severity warning --message "deployment delayed"
agentctl config check
agentctl config explain
agentctl config explain --config-path ./config.toml
agentctl config template --backend openrc --output ./config.toml
```

See [crates/agentctl/README.md](crates/agentctl/README.md).

## Releases

The release workflow creates HeadVer-tagged GitHub releases with
`agentd` and `agentctl` binaries for `x86_64-unknown-linux-musl`,
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
