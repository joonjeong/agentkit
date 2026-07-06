# ops-session

`ops-session` runs a command inside an authenticated provider session obtained
from `agentd`. The current provider is GitHub App authentication:

```sh
ops-session github-app run \
  --profile codex-review \
  -- git remote update
```

GitHub credentials are read by `agentd`, not by `ops-session`. Put provider
profiles in the system-wide broker config at `/etc/agentd/config.toml`:

```toml
[github_app]
app_id = 123456
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/agentd/secrets/codex-review-github-app.private-key.pem"

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
sudo chown root:agentd /etc/agentd/secrets/codex-review-github-app.private-key.pem
sudo chmod 0640 /etc/agentd/secrets/codex-review-github-app.private-key.pem
```

`--profile` or `OPS_SESSION_GITHUB_PROFILE` selects the agentd profile.
`--repo OWNER/REPO` and `--permission key=value` request a subset of that
profile's configured scope. `agentd` rejects requests outside the profile
instead of letting `ops-session` override system configuration.

The child command inherits stdin, stdout, stderr, the current working directory,
`PATH`, and ordinary environment variables. The scoped installation token is
injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Useful options:

- `--profile NAME` selects a named config profile. Prefer
  `OPS_SESSION_GITHUB_PROFILE` for long-running agent services.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.
- `--repo OWNER/REPO` and `--permission key=value` can override profile
  repository and permission scope only when agentd allows that subset.

Shell syntax such as pipes, redirects, aliases, and shell functions requires an
explicit shell command:

```sh
ops-session github-app run \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

`ops-session` exits with the child process exit code, so it can be used directly
in automation.

Validate the broker config when needed:

```sh
agentd config check --config-path /etc/agentd/config.toml
```
