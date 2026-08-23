# AI Inline Completion Reference

AI inline completions provide context-aware code suggestions as ghost text
while you type. The feature is **opt-in** and **disabled by default**. When
enabled, the server sends the surrounding code context to an
OpenAI-compatible API endpoint and streams back completion candidates. If the
AI backend is unavailable, slow, or disabled, the server falls back to
deterministic pattern-based completions automatically.

## VS Code Settings

These settings appear under the "Perl LSP: AI Completion" section in the VS
Code settings UI.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `perl-lsp.aiCompletion.enabled` | boolean | `false` | Enable AI-powered inline completions. **Machine scope** — cannot be set per-workspace in `.vscode/settings.json`. |
| `perl-lsp.aiCompletion.streaming.enabled` | boolean | `true` | Enable progressive streaming (ghost text updates as tokens arrive). Requires `aiCompletion.enabled`. **Machine scope.** |

Where each field can be set:

| Channel | Fields it accepts |
|---------|-------------------|
| `.perl-lsp.toml` | `enabled` only (`false` opt-out; `true` is ignored) |
| LSP client/server configuration | envelope/presentation fields only (`timeoutMs`, limits, `fallback`, `localModelMode`, `streaming.updateDebounceMs`) — never activation, provider/model, endpoint, or credentials (#4997, #5684) |
| Primary VS Code extension | client-side activation/streaming toggles only (machine scope) — the extension does not forward an AI transport configuration to the server |

Project configuration is opt-out only: `enabled = false` can disable AI for a
repository, while `enabled = true`, `provider`, and `model` are ignored. No
current channel can enable remote AI or choose its provider/model: activation
authority is reserved for a future server-owned trusted user/operator adapter
(issue #4997, tracked by #10817). The API key itself is always read from an
environment variable; it is never stored in settings.

## Server Configuration Fields

The server parses the `aiCompletion` section of LSP `initializationOptions`,
`didChangeConfiguration`, and `workspace/configuration` results through one
generic settings parser. Because none of these channels can prove user/machine
provenance, **activation and selection fields are rejected on arrival**
(#4997); previously accepted state is preserved and a warning names the
ignored key.

| JSON Key | Type | Default | Accepted generically | Description |
|----------|------|---------|----------------------|-------------|
| `enabled` | boolean | `false` | no — rejected (#4997) | Master toggle for AI completions; writable only by the future trusted operator adapter. |
| `provider` | string | `"openai_compat"` | no — rejected (#4997) | Provider type. Currently only `openai_compat` is supported. |
| `endpoint` | string | `""` (empty) | no — rejected (#5684) | API endpoint URL (e.g. `https://api.openai.com/v1/chat/completions`). |
| `model` | string | `"gpt-4o-mini"` | no — rejected (#4997) | Model identifier sent in the request body. |
| `apiKeyEnv` | string | `"OPENAI_API_KEY"` | no — rejected (#5684) | Name of the environment variable containing the API key. |
| `apiKeyHeader` | string | `"Authorization"` | no — rejected (#5684) | HTTP header the credential is sent in. Must be a valid header name; an empty or invalid value is ignored and the default is kept. Use e.g. `"x-api-key"` for providers that do not use `Authorization`. |
| `apiKeyPrefix` | string | `"Bearer"` | no — rejected (#5684) | Scheme prepended to the key, sent as `<prefix> <key>`. Set to `""` to send the raw key with no prefix — required by providers that expect a bare token. Values containing control characters are ignored. |
| `timeoutMs` | integer | `1800` | yes | Per-request timeout in milliseconds. |
| `maxOutputTokens` | integer | `64` | yes | Maximum tokens the model may generate per request. |
| `rateLimitRps` | float | `1.0` | yes | Maximum requests per second (token-bucket rate). |
| `maxInflight` | integer | `1` | yes | Maximum concurrent in-flight requests (burst size). |
| `fallback` | boolean | `true` | yes | Fall back to deterministic completions on AI failure. |
| `streaming.enabled` | boolean | `true` | no — rejected (#4997) | Enable streaming mode (progressive ghost text). Streaming preferences never imply backend authorization. |
| `streaming.updateDebounceMs` | integer | `60` | yes | Minimum milliseconds between streamed ghost text updates. The first and final cumulative updates are always emitted. |

Arming the remote backend additionally requires an accepted trusted
activation; until the server-owned adapter lands, remote construction fails
closed and inline completions resolve deterministically regardless of any
client payload.

## Project Config File (`.perl-lsp.toml`)

For editor-agnostic, team-wide policy, add an `[ai_completion]` section to
`.perl-lsp.toml` at the workspace root. The only honoured project field is
`enabled = false` (opt-out). Enabling AI or choosing provider/model requires
user/machine client settings.

```toml
[ai_completion]
# Opt-out only: set enabled = false to disable AI for this repo.
# enabled = true is ignored — AI must be turned on in user/machine settings.
enabled = false
```

Only the fields you set will override defaults. Omitted fields retain their
built-in default values. `enabled = true`, `provider`, and `model` in
`.perl-lsp.toml` are **not honoured** (issue #4997). Enabling remote AI and
choosing provider/model currently has no accepted channel at all: that
authority is reserved for the future server-owned trusted user/operator
adapter (#10817).

### Destination, credentials, activation, and selection cannot come from any project or generic channel

`endpoint`, `api_key_env`, `api_key_header`, `api_key_prefix`,
`enabled`, `provider`, and `model` **cannot be set from `.perl-lsp.toml` nor
from generic LSP settings** (`initializationOptions`, `didChangeConfiguration`,
`workspace/configuration` results). A checked-in file or a workspace-derived
client payload must never be able to choose which environment variable is read
as a credential, where requests are sent (#4955, #5684), or whether source code
leaves the machine at all (#4997).

Generic-channel arrivals of these keys are logged with a warning naming the
ignored setting; previously accepted state is preserved.

> **Activation is user/machine-scoped — and server-enforced.**
> `perl-lsp.aiCompletion.enabled` and
> `perl-lsp.aiCompletion.streaming.enabled` are declared `scope: machine` in
> the VS Code extension, the server ignores project attempts to enable AI,
> and no generic LSP channel can arm the backend either (issue #4997).
> A repository can still opt out with `[ai_completion] enabled =
> false` in `.perl-lsp.toml`. Issue #4998 covers the same provenance gap for
> include paths.

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | Default API key variable. The server reads the key from whatever variable name is configured in `apiKeyEnv`. |
| `GEMINI_API_KEY` | Fallback key variable used automatically when `apiKeyEnv` is unset/empty in the environment. Useful for Gemini CLI/OpenAI-compatible setups. |
| `GOOGLE_API_KEY` | Secondary fallback key variable for Gemini/OpenAI-compatible setups. |

The API key is resolved at runtime via `std::env::var`. The variable must be
set in the environment where the LSP server process runs. If the variable is
empty or unset, the server logs a warning and disables AI completions (falling
back to deterministic rules if `fallback` is true).

## Supported Providers

The only supported provider type is `openai_compat`, which supports both
OpenAI-compatible wire formats:

- **Responses API** (recommended when available)
- **Chat Completions API** (legacy compatibility)

The server auto-selects the request/stream format from your configured endpoint:

- Endpoints containing `/responses` use Responses payload and event parsing
- Other endpoints use Chat Completions payload and event parsing

- **OpenAI** -- `https://api.openai.com/v1/chat/completions`
- **OpenAI (Responses)** -- `https://api.openai.com/v1/responses`
- **Azure OpenAI** -- `https://<resource>.openai.azure.com/openai/deployments/<deployment>/chat/completions?api-version=<version>`
- **Local servers** -- Any OpenAI-compatible local inference server (e.g. llama.cpp, vLLM, Ollama with OpenAI compatibility layer)
- **Gemini (OpenAI-compatible)** -- `https://generativelanguage.googleapis.com/v1beta/openai/chat/completions` (typically with `model` like `gemini-2.5-pro` or `gemini-2.5-flash`)
- **Other providers** -- Any service that accepts the same request format and returns SSE `data:` lines with `choices[].delta.content`

For Chat Completions endpoints, the request format uses `"stream": true`,
`messages` (system + user with fill-in-the-middle context), and the configured
`model` and `max_tokens`.

For Responses endpoints, the request format uses `"stream": true`,
`instructions`, `input`, `model`, and `max_output_tokens`.

## Example Configuration (VS Code `settings.json`)

```jsonc
{
  // Enable the feature
  "perl-lsp.aiCompletion.enabled": true
}
```

> **Known gap: there is currently no VS Code setting for `endpoint` or the
> API-key fields, and no accepted channel supplies them to the server.**
>
> Earlier revisions of this document showed a `perl-lsp.serverConfig` block
> here. **That setting does not exist** — the extension has never contributed
> it, and its `initializationOptions` carry only `disabledFeatures`. The
> example was wrong when written.
>
> The endpoint and credential fields cannot be supplied from `.perl-lsp.toml`
> (a checked-in file choosing the credential name and destination is the
> exfiltration chain closed in issue #4955), and generic LSP settings channels
> are rejected for the same provenance reasons (#5684, #4997). Until a
> server-owned trusted operator adapter exists (issue #4997, #10817), remote
> AI backend construction fails closed; inline completions resolve through the
> deterministic path.
>
> If you previously configured `endpoint` or enabled AI via `.perl-lsp.toml`
> or client settings, those values no longer take effect. That is intentional;
> the replacement surface is tracked in #4997.

Set the API key in your shell profile or VS Code terminal environment:

```bash
export OPENAI_API_KEY="sk-..."
```

## Streaming vs Buffered Behavior

The server supports two completion delivery modes:

**Streaming (default, `streaming.enabled: true`)**:
The server opens an SSE connection to the provider and delivers partial
completions as ghost text via `$/progress` notifications. Each update
contains the cumulative text so far (not a delta). The first update is
immediate, intermediate updates respect `updateDebounceMs`, and the final
cumulative result is always emitted. A
session manager tracks active streams with cancel-previous semantics --
when the user types or moves the cursor, the stale stream is cancelled
and a new one starts.

**Buffered (`streaming.enabled: false`)**:
The server waits for the full response before returning the completion
item. Ghost text appears all at once after the request completes (or
times out). This mode is simpler but has higher perceived latency.

In both modes, if the AI backend fails (timeout, auth error, rate limit),
the server falls back to deterministic pattern-based completions when
`fallback` is `true`.

## Troubleshooting

**No AI completions appearing**

1. Remote AI activation currently has no accepted configuration channel: the
   server-owned trusted operator adapter is pending (issue #4997, #10817), so
   completions resolve deterministically. A warning naming
   `aiCompletion.enabled` means a client payload tried to arm egress and was
   rejected.
2. When the trusted adapter lands, check that `endpoint` is set to a valid URL
   (it defaults to empty) and that the API key environment variable is set and
   non-empty in the LSP server's process environment. Check the server stderr
   log for `AI completion enabled but <VAR> is empty or unset`.

**Authentication errors (401/403)**

- The API key is read from the environment variable named in `apiKeyEnv`.
  Verify the variable name matches and the key is valid.
- For Azure OpenAI, ensure the endpoint URL includes the required
  `api-version` query parameter.

**Completions are slow or missing**

- Increase `timeoutMs` if the provider is slow (default is 1800ms).
- Check `rateLimitRps` and `maxInflight`. With defaults of 1.0 rps and 1
  concurrent request, rapid typing will hit the rate limiter. The server
  returns `RateLimited` errors silently and falls back to deterministic
  completions.
- Enable streaming (`streaming.enabled: true`) for lower perceived latency.

**Rate limiting from the provider**

- Reduce `rateLimitRps` to stay within your API plan's limits.
- Reduce `maxOutputTokens` to lower per-request cost and latency.

**Wrong endpoint format**

- The endpoint must be a full API resource URL, not just the base URL.
- Use `.../v1/chat/completions` for Chat Completions format.
- Use `.../v1/responses` for Responses format (recommended for Codex Desktop
  and modern OpenAI-compatible stacks).

**Fallback completions appearing instead of AI**

- This is expected when AI is rate-limited, timed out, or returns an
  error. Set `fallback` to `false` to suppress deterministic completions
  entirely (not recommended -- you will get no completions on AI failure).
