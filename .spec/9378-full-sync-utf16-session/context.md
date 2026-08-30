# Context — #9378 full-sync UTF-16 initialize/session contract

Claim: the accepted initialized session contains one immutable text/position
contract — `sync_kind = full`, `position_encoding = utf-16` — and the
initialize response, stored session state, and bounded evidence are all
derived from that single accepted value.

Parent: #8531. Decision gate: #8129 (`full_document_utf16`, recorded
2026-08-29, `selected_for_implementation`). Train: #8686 B03.
Feeds: #9380 (full-replacement transaction), #9383.

## Current-main inventory (pinned a83ad9a027)

Three independent authorities own the text/position contract today:

1. `crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs`
   — `handle_initialize` negotiates
   `general.positionEncodings` by picking the first locally supported entry
   from `["utf-8", "utf-16"]` and **falls back to UTF-16 when the client
   offers no supported entry** (no-common silently accepted). The selected
   value is stored on `ClientCapabilities.position_encoding`
   (`crates/perl-lsp-rs/src/state/document.rs`) and may become `Utf8`.
2. Same file — `let sync_kind = 1;` hard-coded local variable feeds
   `TextDocumentSyncOptions::new` in the response.
3. Same file — `capabilities["positionEncoding"] = "utf-16"` hard-pinned
   string, pinned independently of the negotiated value because providers
   still compute UTF-16 offsets.

Known wrong behavior on main (positively encoded by
`initialize_falls_back_to_utf16_when_position_encodings_have_no_supported_values`,
`crates/perl-lsp-rs/src/runtime/lifecycle/capabilities.rs`):
a no-common offer such as `["utf-32", "utf-7"]` is accepted with UTF-16
despite the client explicitly excluding UTF-16.

Initialize atomicity on main: `initialize_requested` is CAS-set before any
validation and `client_capabilities` is mutated incrementally while parsing;
a late failure would leave partially published capability state with a
consumed one-shot guard.

Consumers of the competing negotiated value:

- `crates/perl-lsp-rs/src/runtime/diagnostics.rs` (pull-diagnostics
  projection, two sites) reads `client_capabilities.position_encoding`;
- `crates/perl-lsp-rs/src/runtime/lifecycle/effective_surface_parity_tests.rs`
  feeds a free-standing `negotiated_encoding(params)` helper into the core
  model input `client.negotiated_position_encoding`;
- core model:
  `crates/perl-lsp-rs-core/src/protocol/effective_surface.rs`
  (`PositionEncodingContract.negotiated_preference`,
  `DowngradeReason::PositionEncodingPin`).

Ledger/artifact surfaces describing this behavior:

- `crates/perl-lsp-rs-core/src/protocol/final_surface_inventory/rows.rs`
  mutation row `mut.handle_initialize.positionEncodingPin`
  ("negotiated, stored, NOT advertised") and compat row
  `compat.protocol.positionEncodingUtf16Pin`;
- generated artifact `docs/specs/lsp-final-surface-inventory.json`
  (byte-checked against the ledger; regenerate via the crate's ignored
  regeneration test);
- census: `crates/perl-lsp-rs/src/runtime/lifecycle/final_surface_census.rs`
  (structural pointer coverage only; no encoding-selection semantics).

Tests pinning current behavior that must move with the contract:

- `capabilities.rs` unit tests: `initialize_prefers_first_supported_position_encoding`
  (selects UTF-8), `initialize_accepts_utf16_when_it_is_first_supported_position_encoding`,
  `initialize_falls_back_to_utf16_when_position_encodings_have_no_supported_values`;
- `crates/perl-lsp-rs/tests/lsp_3_17_lifecycle_tests.rs`
  `test_position_encoding_advertised_is_clamped_to_utf16_pending_phase_2`;
- `effective_surface_parity_tests.rs`
  `pull_diagnostic_client_with_refresh_supports_and_utf8_preference_matches`
  (subject offers `["utf-32", "utf-8"]`).

## Branch-selection gate

#8129 selected `full_document_utf16` for the v0.18 release candidate and
stable 0.18.0 only. Production activation in this leaf is bounded to that
selection: no negotiated UTF-8, no incremental/ranged sync, no provider
migration, no VS Code packaging, no public support claims. The long-term
atomic-incremental programme (#1690/#7409/#7417/#7713/#9282) remains open
and is not blended here.

## Reviewed dispositions pinned by this leaf

- **PresentEmpty** `general.positionEncodings: []` — selects UTF-16 by the
  protocol default, recorded as its own selection reason (`EmptyOffer`),
  distinct from `OfferAbsent`. Basis: the issue's selection rules fail only
  "supplied nonempty list[s] with no UTF-16"; the accepted LSP
  interpretation (rust-analyzer precedent) treats an empty list as no
  expressed constraint rather than a constraint that cannot be satisfied.
- **Absent and null** both mean "no offer" on the wire (JSON `null` for an
  optional array is the absent spelling, not a malformed list) — UTF-16
  default, recorded distinctly from a present-but-invalid value.
- **PresentMalformed** (list present with non-string entries, or the value
  is not an array) — typed initialize failure; `as_array` failure never
  collapses into absence.
- Unknown encoding strings are retained in the offer receipt and are not
  fatal; unknown + UTF-16 still selects UTF-16.
- A rejected initialize consumes the one-shot initialize guard (documented
  latitude from the #9378 architecture review): it publishes no session
  contract, no capability mutation, no workspace/config side effect, and
  cannot be retried in the same process.
