# agentd

`agentd` is a local service broker for agent tools. It owns system-wide provider
profiles and long-lived secret access, then serves short-lived credentials and
notification delivery to local clients over a Unix domain socket.

For now, the supported providers are GitHub App authentication, Telegram or
Discord notification delivery, and allowlisted service control.

## Configuration

`agentd` reads system-wide config from `/etc/agentkit/agentd.toml` by default:

```toml
[github_app]
api_url = "https://api.github.com"
default_profile = "codex-review"

[github_app.profiles.codex-review]
app_id = 123456
repos = ["OWNER/REPO"]

[github_app.profiles.codex-review.private_key]
type = "file"
path = "/etc/agentkit/secrets/codex-review-github-app.private-key.pem"

[github_app.profiles.codex-review.permissions]
contents = "read"
pull_requests = "read"

[telegram.profiles.myriad.token]
type = "file"
path = "/etc/agentkit/secrets/telegram-bot-token"

[discord.profiles.myriad.webhook]
type = "file"
path = "/etc/agentkit/secrets/discord-webhook-url"

[service]
backend = "systemd"
max_log_lines = 1000

[service.hermes]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]

[service.cloudflared]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]

[service.tailscale]
viewer = ["u:hermes", "g:agentkit"]
operator = ["u:hermes", "g:agentkit"]
```

Validate and run the broker with:

```sh
agentd config check --config-path /etc/agentkit/agentd.toml
agentd serve --config-path /etc/agentkit/agentd.toml --socket-path /run/agentd/agentd.sock
```

Bootstrap a host from a downloaded or locally copied binary:

```sh
sudo ./agentd bootstrap
sudo ./agentd bootstrap --backend openrc
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

Notification request:

```json
{"version":1,"type":"notify","caller":"hermes","provider":"telegram","profile":"myriad","chat_id":"123456789","severity":"critical","title":"disk full","message":"/var is 95%"}
```

Successful notification response:

```json
{"status":"ok","version":1}
```

Notification profiles live under `[telegram.profiles.<name>]` and
`[discord.profiles.<name>]` in the agentd config. Telegram profiles require
`[telegram.profiles.<name>.token]`; `chat_id` is supplied per notify request.
Discord profiles require `[discord.profiles.<name>.webhook]`. Notification
secrets use the same typed source shape as GitHub App private keys:
`type = "file"` with `path`, or `type = "command"` with `command` and optional
`args`.

Service request:

```json
{"version":1,"type":"service","action":"restart","service":"hermes","lines":null}
```

Successful service response:

```json
{"status":"ok","version":1,"exit_code":0,"stdout":"","stderr":""}
```

Service authorization is based on Unix-domain-socket peer credentials. agentd
checks the connecting process uid/gid against `[service.<target>]` entries and
rejects actions outside `viewer` or `operator` identity lists. Use `u:<name>`
for users and `g:<name>` for groups.
