# Context: #4997 — remote AI activation and request-selection authority

## Current proposition

The first-party clients have already removed project activation and machine-scoped
the visible VS Code toggles. The remaining defect is server-side: the generic LSP
configuration parser still treats an undifferentiated `aiCompletion` object as
trusted user authority.

On the implementation base `cf145b234a9bba19a165653acfeede71aea08bbe`,
`ServerConfig::update_from_value` still performs these assignments:

```text
aiCompletion.enabled  -> AiCompletionConfig.user_enabled
aiCompletion.provider -> AiCompletionConfig.provider
aiCompletion.model    -> AiCompletionConfig.model
```

It then recomputes:

```text
enabled = user_enabled && !project_opt_out
```

`LspServer::refresh_ai_backend` consumes that effective flag, resolves the
configured credential source, constructs an `OpenAiProvider`, and publishes a
bare backend `Arc` for inline-completion requests. Generic initialization
options, `workspace/didChangeConfiguration`, and configuration-result payloads
therefore still have enough authority to arm a preconfigured remote transport
and choose its provider/model request identity.

## Security boundary

No generic, resource, project, folder, mixed, or unknown-provenance observation
may:

```text
arm a remote AI backend
select provider
select model
cause the first source-bearing outbound request
```

Project configuration may only reduce eligibility with an opt-out. Until a
production server-owned trusted-user/operator adapter exists, production remote
backend construction remains fail-closed.

The test-only `LspServer::test_configure_ai_completion` seam is retained so the
AI provider and completion semantics can be tested without falsely treating a
generic transport as trusted product configuration.

## Current authority inventory

### Generic parser

`crates/perl-lsp-rs-core/src/config/mod.rs` owns the live generic parser and the
current `user_enabled`, provider, model, derived enabled, project opt-out, and
safe preference/resource fields.

Disposition:

- ignore `enabled`, `provider`, and `model` on the generic LSP channel;
- emit bounded warnings that identify the rejected field without echoing private
  values;
- keep project opt-out behavior unchanged;
- keep non-arm/select fields in this PR only where they were already admitted;
  their hard envelopes and lifecycle remain #10917/#10909;
- do not remove the AI feature or its test-only trusted setup seam.

### Configuration authority catalog

`crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs` currently lists
`InitializationOptions` and `GlobalClientSettings` as sources for
`ai.user_enabled`, `ai.provider`, and `ai.model`. It also lists those sources in
the derived `ai.effective_enabled` row.

Disposition:

```text
arm/select fields
  CompiledDefault + TrustedUserSettings only

derived effective enabled
  CompiledDefault + TrustedUserSettings + ProjectFile reducer
```

The catalog must not claim a trusted production adapter exists. `TrustedUserSettings`
is the reserved authority class; current runtime observation channels do not
produce it yet.

### Generic settings schema

`schemas/perllsp-settings.schema.json` currently advertises `enabled`,
`provider`, and `model` as generic client-controlled fields.

Disposition: remove those three properties from the generic schema and state
that generic clients can submit only non-activating preferences/resource
requests. The schema cannot advertise a field the runtime rejects as authority.

### Runtime backend construction and request path

`LspServer::refresh_ai_backend` is the first construction seam. The ordinary
inline-completion handler consults `ai_completion.enabled` before invoking a
stored backend.

Disposition:

- generic configuration attempts must leave effective AI disabled and backend
  storage empty;
- an explicitly invoked inline-completion request after hostile generic config
  must perform zero backend calls;
- trusted test setup remains able to exercise the existing backend semantics;
- generation-bound stale-request and first-egress revalidation remain #10909,
  not this PR.

### Documentation and first-party client surface

VS Code already declares `perl-lsp.aiCompletion.enabled` and streaming enablement
at machine scope, but the server receives the resulting payload through a generic
configuration channel that carries no trusted provenance. The primary extension
also exposes no endpoint/credential adapter.

Disposition: document the present security-first product state honestly:

```text
project config may opt out
first-party/generic production clients cannot arm or select remote AI yet
remote AI stays unavailable until a trusted user/operator adapter lands
deterministic inline completion remains available
```

Do not describe machine-scoped VS Code storage as sufficient server authority.
Do not restore endpoint or credential acceptance to generic LSP payloads.

## Scope distinction

This issue owns activation and provider/model selection plus a discriminating
zero-first-egress control.

It does not own:

- destination/SSRF/header transport hardening (#5004/#4955);
- complete accepted configuration generations or stale session invalidation
  (#10857/#10909);
- consent UX (#5049);
- general AI resource-envelope cutover (#10917);
- provider expansion, ranking, prompt semantics, streaming presentation, or
  installed-editor support.

## Strongest counter-read

A machine-scoped VS Code setting is user-owned, so one could keep accepting
`enabled` from the generic server configuration channel and rely on the
extension to enforce provenance.

That does not close the server contract. Non-VS-Code clients have no scope
semantics; workspace/configuration results are undifferentiated; initialization
options carry no unforgeable source class; and a later adapter can accidentally
forward workspace values. The server cannot infer trusted user authority from a
field name or array slot. Fail-closed activation is required until the transport
itself supplies an admitted authority identity.

## Delivery boundary

One PR should:

1. close generic parser authority for `enabled`, `provider`, and `model`;
2. correct the generic schema and configuration-authority catalog;
3. preserve project opt-out and existing trusted test setup;
4. add state, construction, and exact invoked-request zero-backend-call proof;
5. add recurrence protection and current documentation;
6. record the residual #10909 lifecycle boundary without claiming it complete.

A valid merge closes #4997 and advances #10909/#6736. It does not establish a
production trusted-user AI adapter or complete remote AI support.
