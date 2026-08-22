# Acceptance: #4997 — generic configuration cannot arm remote AI

## Required outcome

No production generic LSP configuration payload can activate a remote AI
backend, select provider/model request identity, or cause the first outbound
AI request. Project configuration remains opt-out only. Test-only trusted setup
continues to exercise existing AI completion semantics.

## Stable acceptance rows

### AI-AUTH-001 — generic enabled true is inert

`ServerConfig::update_from_value({"aiCompletion":{"enabled":true}})` does not
change `user_enabled` or effective `enabled`.

### AI-AUTH-002 — generic enabled false is not a hidden authority

The same generic channel cannot rewrite trusted activation state in either
direction. Project opt-out remains the supported repository reduction path.

### AI-AUTH-003 — generic provider is inert

A generic payload cannot change the accepted provider from its prior value.

### AI-AUTH-004 — generic model is inert

A generic payload cannot change the accepted model from its prior value.

### AI-AUTH-005 — mixed hostile payload cannot compose activation

A payload combining enabled/provider/model with endpoint, credential locator,
local-model mode, scheduling, and streaming fields cannot create effective
activation or a backend. Rejected arm/select fields stay rejected even when
non-arm/select siblings are valid.

### AI-AUTH-006 — project opt-out still reduces

A trusted test setup may enable AI; applying `[ai_completion] enabled = false`
from project configuration disables the effective feature for that project.
`enabled = true` remains ignored.

### AI-AUTH-007 — initialization options cannot arm

An initialize payload carrying generic AI activation/selection fields leaves the
server without an active backend.

### AI-AUTH-008 — didChangeConfiguration cannot arm

A `workspace/didChangeConfiguration` payload carrying valid-looking remote AI
activation/selection fields leaves the server without an active backend.

### AI-AUTH-009 — configuration response cannot arm

Unscoped and per-root configuration responses cannot establish trusted user
AI authority by position, root, or client label.

### AI-AUTH-010 — first invoked request has zero backend calls

After hostile generic configuration and an explicitly invoked
`textDocument/inlineCompletion`, a counting backend is called zero times and the
request returns only the existing deterministic behavior allowed by fallback.
This is the discriminating first-egress control.

### AI-AUTH-011 — trusted test setup still exercises AI

`test_configure_ai_completion` plus an injected backend still reaches the backend
for an explicitly invoked request. The security fix does not delete AI provider
or completion semantics.

### AI-AUTH-012 — catalog sources are exact

`ai.user_enabled`, `ai.provider`, and `ai.model` list only compiled default and
reserved trusted-user settings. `ai.effective_enabled` additionally lists the
project-file reducer. Generic client sources are absent.

### AI-AUTH-013 — generic schema is truthful

The generic settings schema omits `enabled`, `provider`, and `model` from
`aiCompletion`, while retaining only fields the generic parser still admits.
Endpoint and credential-routing fields remain absent.

### AI-AUTH-014 — warnings are bounded and redacted

Rejected generic fields produce bounded diagnostics that name field identity and
required authority. Provider/model values, endpoint material, credential values,
and source text are not copied into durable output.

### AI-AUTH-015 — first-party documentation is truthful

Current AI configuration documentation states that no production first-party or
generic client can arm/select remote AI until a trusted user/operator adapter
exists. Project opt-out and deterministic fallback remain documented.

### AI-AUTH-016 — recurrence is mechanical

A source/architecture test fails if generic parser assignments to user enablement,
provider, or model return; if the catalog restores generic arm/select sources; or
if the generic schema/documentation advertises those fields as effective.

### AI-AUTH-017 — lifecycle claim stays bounded

The completion receipt states that stale generation, held backend/session/cache,
retry/stream and final pre-network supersession behavior remain #10909. This PR
proves generic inputs never establish the initial activation subject; it does not
claim complete generation-bound egress lifecycle.

## Required scenario matrix

| Scenario | Expected result |
| --- | --- |
| default config | AI disabled; no backend |
| generic enabled=true | ignored; AI disabled |
| generic enabled=false | ignored as activation authority |
| generic provider/model only | previous provider/model retained |
| generic arm/select plus safe preference | arm/select ignored; admitted preference may update |
| project enabled=true | ignored; no activation |
| project enabled=false after trusted test enable | effective AI disabled |
| initializationOptions hostile payload | zero backend construction/calls |
| didChangeConfiguration hostile payload | zero backend construction/calls |
| unscoped/per-root result hostile payload | zero backend construction/calls |
| explicit invoked request after hostile payload | counting backend calls = 0 |
| explicit trusted test enable + mock backend | counting backend calls = 1 |

## Negative controls

The focused proof must become red when a mutation:

1. assigns generic `enabled` to `user_enabled`;
2. assigns generic provider or model;
3. restores `InitializationOptions` or `GlobalClientSettings` to catalog
   arm/select rows;
4. treats machine-scoped VS Code storage as server-side provenance;
5. allows an unscoped configuration result to become trusted by array position;
6. proves only config values while the invoked-request backend counter can fire;
7. disables the whole AI block to avoid discriminating safe siblings;
8. removes project opt-out;
9. removes the test-only trusted seam or existing AI behavior;
10. restores generic endpoint/credential acceptance;
11. logs hostile provider/model/credential values; or
12. claims #10909 generation/session/stream lifecycle complete.

## Completion condition

#4997 closes only when current parser, schema, catalog, runtime first-effect,
project reduction, documentation, and recurrence rows all agree. A machine-scope
manifest change alone is insufficient.
