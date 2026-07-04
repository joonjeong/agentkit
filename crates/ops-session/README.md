# ops-session

`ops-session` runs a command inside an authenticated provider session. The
current provider is GitHub App authentication:

```sh
ops-session github \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  -- git remote update
```

GitHub credentials are read from a config file. The default path is
`/etc/ops-session/github.toml`; override it with `--config-path` or
`OPS_SESSION_GITHUB_CONFIG_PATH`.

```toml
app_id = 123456
private_key_path = "/etc/ops-session/github-app.private-key.pem"
api_url = "https://api.github.com"
repos = ["OWNER/REPO"]

[permissions]
contents = "read"
```

`app_id`, `installation_id`, `api_url`, `repos`, and `permissions` can be set in
the config file. `--app-id`, `GITHUB_APP_ID`, `--installation-id`,
`GITHUB_APP_INSTALLATION_ID`, `--api-url`, `GITHUB_API_URL`, `--repo`, and
`--permission` override non-secret config values. The private key path is only
read from `private_key_path` in the config file.

The child command inherits stdin, stdout, stderr, the current working directory,
`PATH`, and ordinary environment variables. The scoped installation token is
injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Useful options:

- `--repo OWNER/REPO` scopes the token to a repository. Repeat `--repo` for
  multiple repositories.
- `--permission key=value` limits token permissions, for example
  `--permission contents=read`.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.

Shell syntax such as pipes, redirects, aliases, and shell functions requires an
explicit shell command:

```sh
ops-session github \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

`ops-session` exits with the child process exit code, so it can be used directly
in automation.

Diagnostic token output is available when needed:

```sh
ops-session github app-auth \
  --repo OWNER/REPO \
  --format json
```

`app-auth` prints the installation token by default. Use it for diagnostics or
integrations that explicitly need token stdout, not ordinary agent workflows.
