# agent-session

`agent-session` runs a command inside an authenticated provider session obtained
from `agentd`. The current provider is GitHub App authentication:

```sh
AGENT_SESSION_GITHUB_PROFILE=codex-review \
agent-session github-app run \
  -- git remote update
```

GitHub credentials and provider profiles are read by `agentd`, not by
`agent-session`. Configure profile names, repository scope, permissions, and
private key sources in `/etc/agentkit/agentd.toml`; see
[agentd](../agentd/README.md) for the config schema and wire protocol.

`AGENT_SESSION_GITHUB_PROFILE` is the default way to select the agentd profile for
agent services. `--profile` is available for one-off overrides.
`--repo OWNER/REPO` and `--permission key=value` request a subset of that
profile's configured scope. `agentd` rejects requests outside the profile
instead of letting `agent-session` override system configuration.

The child command inherits stdin, stdout, stderr, the current working directory,
`PATH`, and ordinary environment variables. The scoped installation token is
injected as both `GH_TOKEN` and `GITHUB_TOKEN`.

Useful options:

- `AGENT_SESSION_GITHUB_PROFILE` selects the default agentd profile for the
  process. Use `--profile NAME` only for one-off overrides.
- `--git-credentials` configures a child-only Git credential helper for HTTPS
  GitHub remotes.
- `--repo OWNER/REPO` and `--permission key=value` can override profile
  repository and permission scope only when agentd allows that subset.

Shell syntax such as pipes, redirects, aliases, and shell functions requires an
explicit shell command:

```sh
agent-session github-app run \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

or with a service-level profile:

```sh
AGENT_SESSION_GITHUB_PROFILE=codex-review \
agent-session github-app run \
  --repo OWNER/REPO \
  -- sh -c 'gh issue view 123 | jq .url'
```

`agent-session` exits with the child process exit code, so it can be used directly
in automation.

Validate the broker config when needed:

```sh
agentd config check --config-path /etc/agentkit/agentd.toml
```
