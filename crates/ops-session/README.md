# ops-session

`ops-session` runs a command inside an authenticated provider session. The
current provider is GitHub App authentication:

```sh
ops-session github-app run \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  -- git remote update
```

GitHub credentials are read from a config file. The default path is
`$XDG_CONFIG_HOME/ops-session/config.toml`, or
`~/.config/ops-session/config.toml` when `XDG_CONFIG_HOME` is unset. Override it
with `--config-path` or `OPS_SESSION_GITHUB_CONFIG_PATH`.
The top level of this file may contain multiple provider sections; GitHub App
settings live under `[github_app]`.

```toml
[github_app]
app_id = 123456
private_key_path = "/home/me/.config/ops-session/github-app.private-key.pem"
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"

[github_app.profiles.codex-maintainer]
repos = ["OWNER/REPO"]

[github_app.profiles.codex-maintainer.permissions]
contents = "write"
pull_requests = "write"
```

`app_id`, `installation_id`, `api_url`, `default_profile`, and `profiles` are
set under `[github_app]`. Profiles are map entries keyed by profile name, so
`[github_app.profiles.codex-review]` defines the `codex-review` profile. On a
node that runs multiple agents, give each agent a distinct profile and set
`OPS_SESSION_GITHUB_PROFILE` in that agent's service environment. Each profile
owns its `repos`, optional `installation_id`, and `permissions`, so agents can
share the same GitHub App credentials without sharing repository scope or token
permissions. `--profile`, `OPS_SESSION_GITHUB_PROFILE`, `--app-id`,
`GITHUB_APP_ID`, `--installation-id`, `GITHUB_APP_INSTALLATION_ID`, `--api-url`,
`GITHUB_API_URL`, `--repo`, and `--permission` override non-secret config
values. The private key path is only read from `github_app.private_key_path` in
the config file; use an absolute path.
Unknown top-level provider sections are ignored by the GitHub App commands, but
unknown fields inside `[github_app]` are rejected so GitHub configuration typos
fail fast.

The child command inherits stdin, stdout, stderr, the current working directory,
`PATH`, and ordinary environment variables. The scoped installation token is
injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Useful options:

- `--repo OWNER/REPO` scopes the token to a repository. Repeat `--repo` for
  multiple repositories.
- `--profile NAME` selects a named config profile. Prefer
  `OPS_SESSION_GITHUB_PROFILE` for long-running agent services.
- `--permission key=value` limits token permissions, for example
  `--permission contents=read`.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.
- `--token-cache` opts into reusing a valid cached installation token.

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
