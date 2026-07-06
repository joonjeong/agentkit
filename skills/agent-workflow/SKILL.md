---
name: agent-workflow
description: Run agent commands through agent-session provider sessions so app-auth credentials stay scoped to the child process.
---

# Agent Workflow

Use this skill when an agent needs temporary provider credentials for a command
without exporting long-lived secrets into the parent shell. `agent-session` is a
provider-scoped session runner: it asks `agentd` for an app-auth context,
injects the short-lived credential into one child process, and then lets the
process exit normally.

The current provider is GitHub App authentication:

```sh
agent-session github-app run [OPTIONS] -- COMMAND [ARG]...
agentd config check --config-path /etc/agentkit/agentd.toml
```

Future providers should follow the same boundary: provider credentials come from
agentd's system-wide config, the minted session credential is scoped to the
child command, and provider-specific secret material is not passed through argv
or the ambient environment.

## Configuration

`agentd` reads system-wide provider settings from `/etc/agentkit/agentd.toml`.
Use `agentd config check --config-path PATH` for local checks and development
runs.

On a node that runs multiple agents, give each agent a distinct provider profile
and set the profile selector in that agent's service environment. For GitHub App
sessions, use `AGENT_SESSION_GITHUB_PROFILE`. `agent-session` does not read private
keys or provider config; it only talks to the local broker socket.

## GitHub App Sessions

Use `agent-session github-app run ... -- COMMAND` for ordinary GitHub work:

```sh
AGENT_SESSION_GITHUB_PROFILE=codex-review \
/usr/local/bin/agent-session github-app run \
  -- gh pr view 123 --repo OWNER/REPO
```

Use `--git-credentials` for HTTPS Git operations:

```sh
AGENT_SESSION_GITHUB_PROFILE=codex-maintainer \
/usr/local/bin/agent-session github-app run \
  --git-credentials \
  -- git remote update
```

Profile selection should normally come from the process environment:

- `AGENT_SESSION_GITHUB_PROFILE`

Use `--profile NAME` only for one-off overrides.

These non-secret values can be requested per invocation. agentd rejects requests
outside the selected profile's configured scope:

- `--repo OWNER/REPO`
- `--permission key=value`

## Agent Workflow

1. Select the narrowest provider profile for the current agent and task.
2. Run provider CLIs or Git commands through `agent-session ... run -- COMMAND`.
3. Keep shell syntax explicit. Pipes, redirects, aliases, variables, and grouped
   commands require an explicit shell command.
4. Add provider-specific helpers only when needed, such as `--git-credentials`
   for HTTPS Git operations.
5. Run `agentd config check` after changing the broker config.

For shell syntax:

```sh
agent-session github-app run \
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

- Treat `agent-session` as a boundary tool: secrets in agentd config, temporary
  credentials in one child process.
- Scope tokens as narrowly as the provider allows.
- Request only the permissions needed by the child command.
- Provider HTTP clients should use finite timeouts, and provider JWTs or similar
  assertions should remain below upstream maximum lifetimes with clock skew
  accounted for.
