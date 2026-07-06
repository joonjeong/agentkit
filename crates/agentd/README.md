# agentd

`agentd` is a local credential broker for agent tools. It owns system-wide
provider profiles and long-lived secret access, then serves short-lived
credentials to local clients over a Unix domain socket.

For now, the supported provider is GitHub App authentication.

## Configuration

`agentd` reads system-wide config from `/etc/agentd/config.toml` by default:

```toml
[github_app]
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
app_id = 123456
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/agentd/secrets/codex-review-github-app.private-key.pem"

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"
```

Validate and run the broker with:

```sh
agentd config check --config-path /etc/agentd/config.toml
agentd serve --config-path /etc/agentd/config.toml --socket-path /run/agentd/agentd.sock
```

Print the full example config with:

```sh
agentd config example
```

## Wire Protocol

The agentd UDS protocol is newline-delimited JSON. Each request and response is
one UTF-8 JSON object followed by `\n`. The current wire protocol version is
`1`; clients must send `version = 1`, and `agentd` includes `version = 1` in
every response.

GitHub App token request:

```json
{"version":1,"type":"github_app_token","profile":"codex-review","repos":["OWNER/REPO"],"permissions":{"contents":"read"}}
```

Successful response:

```json
{"status":"ok","version":1,"token":"...","expires_at":"2026-06-15T00:00:00Z","api_url":"https://api.github.com"}
```

Error response:

```json
{"status":"error","version":1,"error":"repo \"OWNER/OTHER\" is not allowed by GitHub App profile \"codex-review\""}
```

Requests for `repos` and `permissions` are treated as subsets of the selected
system-wide profile. agentd rejects requests outside the configured profile
instead of letting clients widen their own scope.
