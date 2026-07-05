---
name: ops-session-workflow
description: Run agent commands through ops-session provider sessions so app-auth credentials stay scoped to the child process.
---

# Ops Session Workflow

Use this skill when an agent needs temporary provider credentials for a command
without exporting long-lived secrets into the parent shell. `ops-session` is a
provider-scoped session runner: it resolves an app-auth context, mints a
short-lived credential, injects that credential into one child process, and then
lets the process exit normally.

The current provider is GitHub App authentication:

```sh
ops-session github-app run [OPTIONS] -- COMMAND [ARG]...
ops-session github-app config check
ops-session github-app config example
ops-session agent-skill --install-path /path/to/skills
```

Future providers should follow the same boundary: provider credentials come from
the ops-session config, the minted session credential is scoped to the child
command, and provider-specific secret material is not passed through argv or the
ambient environment.

## Configuration

User-level configuration lives at `$XDG_CONFIG_HOME/ops-session/config.toml`, or
`~/.config/ops-session/config.toml` when `XDG_CONFIG_HOME` is unset. Provider
settings are namespaced so one file can hold multiple app-auth providers.

For GitHub App sessions:

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

On a node that runs multiple agents, give each agent a distinct provider profile
and set the profile selector in that agent's service environment. For GitHub App
sessions, use `OPS_SESSION_GITHUB_PROFILE`. Each profile owns its repository
scope, optional installation ID, and requested token permissions.

GitHub App private key paths are read only from
`github_app.private_key_path` in the config file. Use an absolute path. Do not
pass private key paths or private key contents through shell arguments or
environment variables.

## GitHub App Sessions

Use `ops-session github-app run ... -- COMMAND` for ordinary GitHub work:

```sh
OPS_SESSION_GITHUB_PROFILE=codex-review \
ops-session github-app run \
  -- gh pr view 123 --repo OWNER/REPO
```

Use `--git-credentials` for HTTPS Git operations:

```sh
OPS_SESSION_GITHUB_PROFILE=codex-maintainer \
ops-session github-app run \
  --git-credentials \
  -- git remote update
```

Non-secret values can be overridden per invocation:

- `--profile NAME` or `OPS_SESSION_GITHUB_PROFILE`
- `--app-id` or `GITHUB_APP_ID`
- `--installation-id` or `GITHUB_APP_INSTALLATION_ID`
- `--api-url` or `GITHUB_API_URL`
- `--repo OWNER/REPO`
- `--permission key=value`
- `--token-cache`

## Agent Workflow

1. Select the narrowest provider profile for the current agent and task.
2. Run provider CLIs or Git commands through `ops-session ... run -- COMMAND`.
3. Keep shell syntax explicit. Pipes, redirects, aliases, variables, and grouped
   commands require an explicit shell command.
4. Add provider-specific helpers only when needed, such as `--git-credentials`
   for HTTPS Git operations.
5. Run the provider config check after changing the ops-session config.

For shell syntax:

```sh
ops-session github-app run \
  --repo OWNER/REPO \
  -- sh -c 'gh pr view "$1" --repo "$2" --json title,url | jq .url' sh 123 OWNER/REPO
```

## Environment Boundary

For GitHub App sessions, the child command receives:

- `GH_TOKEN` set to the temporary installation token
- `GITHUB_TOKEN` set to the same temporary installation token
- when `--git-credentials` is passed, child-only `GIT_CONFIG_*` values pointing
  at a temporary credential helper and `GIT_TERMINAL_PROMPT=0`

GitHub App credential environment variables are removed from the child
environment. The child still inherits stdin, stdout, stderr, working directory,
PATH, and ordinary environment variables.

## Operational Notes

- Treat `ops-session` as a boundary tool: secrets in config, temporary
  credentials in one child process.
- Scope tokens as narrowly as the provider allows.
- Request only the permissions needed by the child command.
- Token caching is disabled by default. If `--token-cache` is used, the selected
  profile is part of the cache key, so separate agent profiles keep cached
  credentials isolated on a shared Unix account.
- Provider HTTP clients should use finite timeouts, and provider JWTs or similar
  assertions should remain below upstream maximum lifetimes with clock skew
  accounted for.
