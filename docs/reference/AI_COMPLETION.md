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
| `perl-lsp.aiCompletion.enabled` | boolean | `false` | Enable AI-powered inline completions. |
| `perl-lsp.aiCompletion.streaming.enabled` | boolean | `true` | Enable progressive streaming (ghost text updates as tokens arrive). Requires `aiCompletion.enabled`. |

Model, timeout, and rate limits are configured via the LSP server config or the
project config file, not VS Code settings.

`endpoint` and the API-key fields are configured **only** via the LSP server
config — they are not accepted from `.perl-lsp.toml` (see "Destination and
credentials" below, and issue #4955). The API key itself is always read from an
environment variable; it is never stored in settings.

## Server Configuration Fields

The server reads AI completion settings from the `aiCompletion` section of LSP
`initializationOptions` or `didChangeConfiguration`. These are the full fields
accepted:

| JSON Key | Type | Default | Description |
|----------|------|---------|-------------|
| `enabled` | boolean | `false` | Master toggle for AI completions. |
| `provider` | string | `"openai_compat"` | Provider type. Currently only `openai_compat` is supported. |
| `endpoint` | string | `""` (empty) | API endpoint URL (e.g. `https://api.openai.com/v1/chat/completions`). |
| `model` | string | `"gpt-4o-mini"` | Model identifier sent in the request body. |
| `apiKeyEnv` | string | `"OPENAI_API_KEY"` | Name of the environment variable containing the API key. |
| `timeoutMs` | integer | `1800` | Per-request timeout in milliseconds. |
| `maxOutputTokens` | integer | `64` | Maximum tokens the model may generate per request. |
| `rateLimitRps` | float | `1.0` | Maximum requests per second (token-bucket rate). |
| `maxInflight` | integer | `1` | Maximum concurrent in-flight requests (burst size). |
| `fallback` | boolean | `true` | Fall back to deterministic completions on AI failure. |
| `streaming.enabled` | boolean | `true` | Enable streaming mode (progressive ghost text). |
| `streaming.updateDebounceMs` | integer | `60` | Minimum milliseconds between streamed ghost text updates. |

## Project Config File (`.perl-lsp.toml`)

For editor-agnostic, team-wide defaults, add an `[ai_completion]` section to
`.perl-lsp.toml` at the workspace root. These values serve as the base layer;
LSP client settings always override them.

```toml
[ai_completion]
enabled = true
provider = "openai_compat"
model = "gpt-4o-mini"
```

Only the fields you set will override defaults. Omitted fields retain their
built-in default values.

### Destination and credentials cannot come from `.perl-lsp.toml`

`endpoint`, `api_key_env`, `api_key_header`, and `api_key_prefix` **cannot be set
from `.perl-lsp.toml`**. They are read only from the LSP client/server
configuration channel — `initializationOptions` or `didChangeConfiguration`, as
documented under "Server Configuration Fields" above.

`.perl-lsp.toml` is checked into a repository, so honouring those fields let a
cloned project choose both which environment variable was read as a credential
and where it was sent — arbitrary named-secret exfiltration, with your source in
the request body. They were removed for the same reason `perlPath` and `perlArgs`
are not honoured from workspace settings (issue #3729). See issue #4955.

**These keys are silently ignored, not rejected**, because the TOML deserializer
drops unknown fields. If you set `endpoint` in `.perl-lsp.toml` and requests keep
going to the default destination, that is why — move it to your client settings.

> **The client-settings channel is not itself fully hardened yet.** Do not read
> the above as "these values are safe because they come from the user". In
> VS Code, `perl-lsp.aiCompletion.enabled` is declared `scope: resource`, which
> means a workspace or folder can set it, and the extension forwards workspace
> and folder overrides. So a repository still cannot choose the destination or
> the credential, but it **can still activate AI completion** against whatever
> endpoint you configured, sending source code without you opting in for that
> workspace. Tracked as issue #4997 and is a 0.18.0 blocker; issue #4998 covers
> the same provenance gap for include paths.

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
> API-key fields.**
>
> Earlier revisions of this document showed a `perl-lsp.serverConfig` block
> here. **That setting does not exist** — the extension has never contributed
> it, and its `initializationOptions` carry only `disabledFeatures`. The
> example was wrong when written.
>
> Until a configuration surface is added (issue #4997), the endpoint and
> credential fields can only be supplied by an LSP client that sends them in
> `initializationOptions` or `didChangeConfiguration` directly — which the
> VS Code extension does not currently do. They can no longer be set from
> `.perl-lsp.toml`, because a checked-in file choosing the credential name and
> destination is the exfiltration chain closed in issue #4955.
>
> If you previously configured `endpoint` via `.perl-lsp.toml`, it will stop
> taking effect. That is intentional; the replacement surface is tracked in
> #4997.

Set the API key in your shell profile or VS Code terminal environment:

```bash
export OPENAI_API_KEY="sk-..."
```

## Streaming vs Buffered Behavior

The server supports two completion delivery modes:

**Streaming (default, `streaming.enabled: true`)**:
The server opens an SSE connection to the provider and delivers partial
completions as ghost text via `$/progress` notifications. Each update
contains the cumulative text so far (not a delta). Updates are debounced
by `updateDebounceMs` (default 60ms) to avoid flooding the client. A
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

1. Verify `aiCompletion.enabled` is `true` in your settings.
2. Check that `endpoint` is set to a valid URL (it defaults to empty).
3. Confirm the API key environment variable is set and non-empty in the
   LSP server's process environment. Check the server stderr log for
   `AI completion enabled but <VAR> is empty or unset`.

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
