# #4997 - AI egress activation authority context

## Proposition

No client-supplied workspace/resource/project/generic/unknown-provenance value
may arm the remote AI backend or select its request identity. Enablement
authority follows the trusted-operator pattern landed for external include
paths (#4998): a typed server-owned disposition that no LSP channel can
strengthen. The load-bearing property is:

> A repository-controlled value cannot cause the first outbound AI request.

## Current-main state when this packet was written

`origin/main@ab3cece9d` (2026-08-22). PR #11861 had aligned the declared AI
egress authority catalog with machine-owned VS Code scopes and project opt-out,
and #11880 had landed `ExternalIncludePathAuthority` for include roots — but
`ServerConfig::update_from_value` still mapped generic
`aiCompletion.enabled/provider/model/streaming.enabled` arrivals straight into
`user_enabled` / selection fields with zero provenance distinction, and
`refresh_ai_backend` constructed `OpenAiProvider` from that raw flag alone. A
non-VS-Code client forwarding workspace-derived configuration could arm first
outbound egress through a user-preconfigured backend.

## This slice

- Add typed `AiActivationAuthority` (`Unavailable` | `TrustedUserOperator`)
  on `AiCompletionConfig`, mirroring `ExternalIncludePathAuthority`.
- Reject `enabled`, `provider`, `model`, and `streaming.enabled` from every
  generic channel arrival in `update_from_value`; rejections warn naming the
  key and preserve previously accepted trusted state.
- Gate `refresh_ai_backend` construction on accepted trusted activation plus
  the effective flag, not the raw enabled bit alone.
- Project opt-out remains the only production reduction channel; project data
  still cannot perform false -> true.
- Align honesty surfaces: generic settings schema transports emptied for the
  four authority keys, configuration-authority catalog arm/select rows moved
  to `CompiledDefault + TrustedUserSettings`, recurrence gate extended,
  docs/reference updated to say remote activation is pending the adapter.

## Non-goals

- No trusted operator adapter implementation; `TrustedUserOperator` is
  reachable only through `AiCompletionConfig::admit_trusted_user_operator_activation`
  (internal fixtures/tests) until the #10807/#10813/#10817 observation train lands.
- No accepted-configuration-generation store, session identity wrapper, or
  raw-consumer cutover (#10387/#10909 own those).
- No endpoint/credential surface change (#4955/#5684 rows unchanged).
- No consent UX, streaming presentation, or envelope hardening changes
  (#5004/#5049 own destination transport and envelopes).
