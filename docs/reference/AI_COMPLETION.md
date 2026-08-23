# AI Inline Completion

## Current product state

The repository contains an OpenAI-compatible backend, streaming parser, rate
limiter, and deterministic fallback. **Production remote backend activation is
unavailable until a trusted user/operator adapter can give the server an
unforgeable activation subject.**

This is deliberate security containment for #4997. Generic LSP transports do not
preserve whether a value came from user, machine, workspace, folder, project, or
a mixed client merge. Therefore none of these generic fields can arm or select a
remote backend:

```text
aiCompletion.enabled
aiCompletion.provider
aiCompletion.model
aiCompletion.endpoint
aiCompletion.apiKeyEnv
aiCompletion.apiKeyHeader
aiCompletion.apiKeyPrefix
```

The generic settings schema exposes only non-activating preferences and resource
requests. Project `.perl-lsp.toml` may opt out with:

```toml
[ai_completion]
enabled = false
```

Project configuration cannot enable the backend or choose provider, model,
destination, or credentials.

## VS Code setting

`perl-lsp.aiCompletion.enabled` remains a machine-scoped client preference and a
reserved migration surface. It prevents committed workspace settings from
changing the client-side gate, but it is **not** server-side proof of trusted
activation and currently does not make remote completion available.

`perl-lsp.aiCompletion.streaming.enabled` is likewise a machine-scoped
presentation preference. Streaming preference never authorizes network access.

## What still works

`textDocument/inlineCompletion` continues to provide deterministic, local
suggestions. Automatic requests remain local-first. The test-only
`expose_lsp_test_api` adapter can admit a private trusted authority so provider,
fallback, cancellation, and streaming behavior remain directly tested; that seam
is not production configuration.

## Future activation contract

#10817/#10387 will carry typed source observations and accepted configuration
subjects. #10909 will bind backend construction, session/cache identity, retries,
streams, and the last pre-network check to that accepted generation. A future
trusted adapter must prove all of the following before remote activation becomes
available:

```text
server-owned adapter identity
accepted user/operator source
exact configuration generation
project/root eligibility
trusted provider/model/destination/credential source
hard network and resource envelope
final currentness immediately before first egress
```

Client name, configuration-array position, manifest scope text, URI, or a
payload-supplied `trusted` flag cannot satisfy this contract.

## Security boundaries

Destination validation, redirect credential binding, and response/concurrency
limits remain with #5004/#4955 after legitimate activation. Consent UX remains
#5049. This document does not claim those programmes complete.
