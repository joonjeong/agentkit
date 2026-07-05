---
name: github-app-agent-workflow
description: Run GitHub agent workflows through ops-session github-app with user-level GitHub App credentials and short-lived installation tokens.
---

# GitHub App Agent Workflow

Use this skill when an agent needs to inspect or change GitHub state through the
local `ops-session` binary. Prefer `ops-session github-app run ... -- COMMAND` so the
installation token stays inside the child process instead of being printed,
copied, or exported into the parent shell.

## Command Forms

```sh
ops-session github-app run [OPTIONS] -- COMMAND [ARG]...
ops-session github-app config check
ops-session github-app config template
ops-session agent-skill --install-path /path/to/skills
```

Use `ops-session github-app run ... -- COMMAND` for ordinary agent work. Use
`ops-session github-app config check` and `ops-session github-app config template`
to validate or create the user-level auth config.

## Configuration

GitHub App credential material comes from a user-level config file. The default
path is `$XDG_CONFIG_HOME/ops-session/github.toml`, or
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
pull_requests = "read"
```

The private key path is read only from `private_key_path` in this config file.
Use an absolute path. Do not pass private key paths or private key contents
through shell arguments or environment variables.

Each profile owns its repository list and token permissions. Use separate profiles
when different repositories need different permission sets.

Non-secret values can be overridden per invocation:

- `--profile NAME` or `OPS_SESSION_GITHUB_PROFILE`
- `--app-id` or `GITHUB_APP_ID`
- `--installation-id` or `GITHUB_APP_INSTALLATION_ID`
- `--api-url` or `GITHUB_API_URL`
- `--repo OWNER/REPO`
- `--permission key=value`
- `--token-cache`

## Examples

```sh
ops-session github-app run \
  --repo OWNER/REPO \
  --permission contents=read \
  -- gh pr view 123 --repo OWNER/REPO
```

```sh
ops-session github-app run \
  --repo OWNER/REPO \
  --git-credentials \
  -- git remote update
```

`ops-session github-app run` executes the command after `--` directly. For pipes,
redirects, shell functions, aliases, variables, or grouped commands, invoke a
shell explicitly:

```sh
ops-session github-app run \
  --repo OWNER/REPO \
  -- sh -c 'gh pr view "$1" --repo "$2" --json title,url | jq .url' sh 123 OWNER/REPO
```

## Agent Workflow

1. Check that the repository list and requested permissions are as narrow as
   the task allows.
2. Run `gh` or `git` through `ops-session github-app run ... -- COMMAND`.
3. Add `--git-credentials` for HTTPS Git operations such as `git remote update`,
   `git fetch`, or `git push`.
4. Use `ops-session github-app config check` after changing the auth config.

## Environment Boundary

The child command receives:

- `GH_TOKEN` set to the temporary installation token
- `GITHUB_TOKEN` set to the same temporary installation token
- when `--git-credentials` is passed, child-only `GIT_CONFIG_*` values pointing
  at a temporary credential helper and `GIT_TERMINAL_PROMPT=0`

`ops-session github-app run` may reuse a matching locally cached installation token
until it is near expiration. The cache is keyed by app, installation selector,
config path, selected profile, API URL, repository list, and requested
permissions.

## Operational Notes

- Scope tokens with `--repo OWNER/REPO` whenever possible.
- Request only the permissions needed by the child command.
- Git over HTTPS needs `--git-credentials`; `GH_TOKEN` and `GITHUB_TOKEN` are
  not Git credential helpers by themselves.
- Token caching is disabled by default. If `--token-cache` is used, select a
  distinct config profile for agent profiles that share a Unix account but must
  not share cached tokens.
- The HTTP client uses a finite timeout, so automation should fail instead of
  hanging indefinitely.
- The JWT is intentionally short-lived and remains below GitHub's 10-minute
  maximum lifetime.
