# ops-session

`ops-session` runs a command inside an authenticated provider session. The
current provider is GitHub App authentication:

```sh
ops-session github-app run \
  --profile codex-review \
  -- git remote update
```

GitHub credentials are read from a config file. Runs read
`$XDG_CONFIG_HOME/ops-session/config.toml`, or
`~/.config/ops-session/config.toml` when `XDG_CONFIG_HOME` is unset, before
using the system config at `/etc/ops-session/config.toml`. Override it with
`--config-path` or `OPS_SESSION_GITHUB_CONFIG_PATH` for local checks and
development runs.
The top level of this file may contain multiple provider sections; GitHub App
settings live under `[github_app]`.

```toml
[github_app]
app_id = 123456
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/ops-session/secrets/codex-review-github-app.private-key.pem"

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"

[github_app.profiles.codex-maintainer]
repos = ["OWNER/REPO"]

[github_app.profiles.codex-maintainer.private_key]
type = "command"
command = "/usr/bin/op"
args = ["read", "op://ops/github-apps/codex-maintainer/private-key"]

[github_app.profiles.codex-maintainer.permissions]
contents = "write"
pull_requests = "write"
```

`app_id`, `installation_id`, `api_url`, `default_profile`, and auth profiles are
set under `[github_app]`. Profiles are map entries keyed by profile name, so
`[github_app.profiles.codex-review]` defines the `codex-review` auth profile.
On a node that runs multiple agents, give each agent a distinct auth profile and
set `OPS_SESSION_GITHUB_PROFILE` in that agent's service environment. Each auth
profile owns its private key source, `repos`, optional `installation_id`,
and `permissions`.

For a simple local secret store, keep private keys in files readable by the
agent users' group, for example:

```sh
sudo chown root:ops-agent /etc/ops-session/secrets/codex-review-github-app.private-key.pem
sudo chmod 0640 /etc/ops-session/secrets/codex-review-github-app.private-key.pem
```

`--profile`, `OPS_SESSION_GITHUB_PROFILE`, `--app-id`, `GITHUB_APP_ID`, `--installation-id`,
`GITHUB_APP_INSTALLATION_ID`, `--api-url`, `GITHUB_API_URL`, `--repo`, and
`--permission` can override non-secret config values. Private keys are read
from `[github_app.profiles.<profile>.private_key]`. `type = "file"` paths and
`type = "command"` commands must be absolute. Command sources are executed
without a shell and use stdout as the private key.
Unknown top-level provider sections are ignored by the GitHub App commands, but
unknown fields inside `[github_app]` are rejected so GitHub configuration typos
fail fast.

The child command inherits stdin, stdout, stderr, the current working directory,
`PATH`, and ordinary environment variables. The scoped installation token is
injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Useful options:

- `--profile NAME` selects a named config profile. Prefer
  `OPS_SESSION_GITHUB_PROFILE` for long-running agent services.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.
- `--token-cache` opts into reusing a valid cached installation token.
- `--repo OWNER/REPO` and `--permission key=value` can override profile
  repository and permission scope.

Token caching is disabled by default. When enabled, cache keys include the config
path, selected profile, app, installation, API URL, repository list, and
permissions. This keeps cached tokens isolated between agent profiles on the
same Unix account.

Shell syntax such as pipes, redirects, aliases, and shell functions requires an
explicit shell command:

```sh
ops-session github-app run \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

`ops-session` exits with the child process exit code, so it can be used directly
in automation.

Validate the GitHub App auth config when needed:

```sh
ops-session github-app config check
```

Print an example config with `ops-session github-app config example`.
