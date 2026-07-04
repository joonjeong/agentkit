# ops-session

`ops-session` runs a command with a temporary GitHub App installation token.
For now, GitHub App authentication is the only supported session provider.

```sh
ops-session \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem \
  -- git remote update
```

The command after `--` inherits stdin, stdout, stderr, the current working
directory, `PATH`, and ordinary environment variables. GitHub App credential
environment variables are removed from the child environment, and the scoped
installation token is injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Supported environment variables:

- `GITHUB_APP_ID`
- `GITHUB_APP_INSTALLATION_ID`
- `GITHUB_APP_PRIVATE_KEY_FILE`
- `GITHUB_APP_PRIVATE_KEY_PATH`
- `GITHUB_APP_PRIVATE_KEY`
- `GITHUB_API_URL`

Useful options:

- `--repo OWNER/REPO` scopes the token to a repository. Repeat `--repo` for
  multiple repositories.
- `--installation-id ID` skips repository installation discovery when the
  installation ID is already known.
- `--permission key=value` limits token permissions, for example
  `--permission contents=read`.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.

Shell syntax such as pipes, redirects, aliases, and shell functions requires an
explicit shell command:

```sh
ops-session \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem \
  -- sh -c 'gh issue view 123 | jq .url'
```

`ops-session` exits with the child process exit code, so it can be used directly
in automation.

Diagnostic token output is available when needed:

```sh
ops-session app-auth \
  --app-id "$GITHUB_APP_ID" \
  --repo OWNER/REPO \
  --private-key-file /path/to/private-key.pem \
  --format json
```

`app-auth` prints the installation token by default. Use it for diagnostics or
integrations that explicitly need token stdout, not ordinary agent workflows.
