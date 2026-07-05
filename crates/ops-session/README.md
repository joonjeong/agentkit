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
`$XDG_CONFIG_HOME/ops-session/github.toml`, or
`~/.config/ops-session/github.toml` when `XDG_CONFIG_HOME` is unset. Override it
with `--config-path` or `OPS_SESSION_GITHUB_CONFIG_PATH`.

```toml
app_id = 123456
private_key_path = "/home/me/.config/ops-session/github-app.private-key.pem"
api_url = "https://api.github.com"
default_profile = "repo-read"

[[profiles]]
name = "repo-read"
repos = ["OWNER/REPO"]

[profiles.permissions]
contents = "read"
```

`app_id`, `installation_id`, `api_url`, `default_profile`, and `profiles` can be set
in the config file. Each profile owns its `repos` and `permissions`, so different
repositories can use different installation token permissions. `--profile`,
`OPS_SESSION_GITHUB_PROFILE`, `--app-id`, `GITHUB_APP_ID`, `--installation-id`,
`GITHUB_APP_INSTALLATION_ID`, `--api-url`, `GITHUB_API_URL`, `--repo`, and
`--permission` override non-secret config values. The private key path is only
read from `private_key_path` in the config file; use an absolute path.

The child command inherits stdin, stdout, stderr, the current working directory,
`PATH`, and ordinary environment variables. The scoped installation token is
injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Useful options:

- `--repo OWNER/REPO` scopes the token to a repository. Repeat `--repo` for
  multiple repositories.
- `--profile NAME` selects a named config profile.
- `--permission key=value` limits token permissions, for example
  `--permission contents=read`.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.
- `--token-cache` opts into reusing a valid cached installation token.

Token caching is disabled by default. When enabled, cache keys include the config
path, selected profile, app, installation, API URL, repository list, and
permissions. Use separate config profiles when two agent profiles share a Unix
account but must not share cached tokens.

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

Generate a starting config with `ops-session github-app config template`.
