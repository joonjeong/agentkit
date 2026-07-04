---
name: github-app-agent-workflow
description: Perform GitHub agent work through ops-session github without exposing temporary installation tokens.
---

# GitHub App Agent Workflow

Use `ops-session github` to run GitHub commands inside a short-lived GitHub App
installation token context without printing the token or exporting it into the
parent shell.

## Command Forms

```sh
ops-session github [OPTIONS] -- COMMAND [ARG]...
ops-session github app-auth [OPTIONS]
ops-session agent-skill --install-path /path/to/skills
```

Use `ops-session github ... -- COMMAND` for ordinary agent work. Use
`ops-session github app-auth` only for diagnostics or integrations that
explicitly need token stdout.

## Configuration

GitHub App credential material comes from a config file. The default path is
`/etc/ops-session/github.toml`; override it with `--config-path` or
`OPS_SESSION_GITHUB_CONFIG_PATH`.

```toml
app_id = 123456
private_key_path = "/etc/ops-session/github-app.private-key.pem"
api_url = "https://api.github.com"
repos = ["OWNER/REPO"]

[permissions]
contents = "read"
pull_requests = "read"
```

The private key path is read only from `private_key_path` in this config file.
Do not pass private key paths or private key contents through shell arguments or
environment variables.

Non-secret values can be overridden per invocation:

- `--app-id` or `GITHUB_APP_ID`
- `--installation-id` or `GITHUB_APP_INSTALLATION_ID`
- `--api-url` or `GITHUB_API_URL`
- `--repo OWNER/REPO`
- `--permission key=value`

## Examples

```sh
ops-session github \
  --repo OWNER/REPO \
  --permission contents=read \
  -- gh pr view 123 --repo OWNER/REPO
```

```sh
ops-session github \
  --repo OWNER/REPO \
  --git-credentials \
  -- git remote update
```

`ops-session github` executes the command after `--` directly. For pipes,
redirects, shell functions, aliases, variables, or grouped commands, invoke a
shell explicitly:

```sh
ops-session github \
  --repo OWNER/REPO \
  -- sh -c 'gh pr view "$1" --repo "$2" --json title,url | jq .url' sh 123 OWNER/REPO
```

## Environment Boundary

The child command receives:

- `GH_TOKEN` set to the temporary installation token
- `GITHUB_TOKEN` set to the same temporary installation token
- when `--git-credentials` is passed, child-only `GIT_CONFIG_*` values pointing
  at a temporary credential helper and `GIT_TERMINAL_PROMPT=0`

`ops-session github` may reuse a matching locally cached installation token
until it is near expiration. The cache is keyed by app, installation selector,
API URL, repository scope, and requested permissions.

## Operational Notes

- Scope tokens with `--repo OWNER/REPO` whenever possible.
- Request only the permissions needed by the child command.
- Do not log stdout from `ops-session github app-auth`; it may be the token.
- Git over HTTPS needs `--git-credentials`; `GH_TOKEN` and `GITHUB_TOKEN` are
  not Git credential helpers by themselves.
- The HTTP client uses a finite timeout, so automation should fail instead of
  hanging indefinitely.
- The JWT is intentionally short-lived and remains below GitHub's 10-minute
  maximum lifetime.
