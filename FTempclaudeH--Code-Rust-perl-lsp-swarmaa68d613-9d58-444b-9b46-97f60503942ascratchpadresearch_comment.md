<!-- research-triage-pass issue: 2297 verified_on: origin/main -->

## Current state

**Hover contentFormat**: Absent on origin/main. `crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs:21` advertises `HoverProviderCapability::Simple(true)` without HoverOptions structure that would include contentFormat declaration. Hover handler in `crates/perl-lsp-rs/src/runtime/language/hover.rs` does not check client contentFormat capability.

**Signature Help documentation**: Null on origin/main. `crates/perl-lsp-rs/src/runtime/language/hover/signature_help.rs` builds ParameterInformation with `"documentation": null` regardless of availability. No contentFormat negotiation for MarkupContent when docs are present.

**Semantic Tokens delta**: **ALREADY FIXED on origin/main**. Contrary to the issue claim:
- `crates/perl-lsp-rs-core/src/protocol/capabilities/sections.rs:168` declares `full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) })`
- `crates/perl-lsp-rs/src/runtime/dispatch/routing.rs` routes `textDocument/semanticTokens/full/delta` to `handle_semantic_tokens_delta_dispatch()`
- Code comment confirms LSP 3.17 compliance target
- This gap has been resolved; no implementation work needed here.

## Spec verification

**LSP 3.17 contentFormat claims**: CONFIRMED per [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/) §textDocument/hover:
- Server must advertise `HoverOptions.contentFormat: MarkupKind[]` when supporting multiple formats
- Clients declare capability via `HoverClientCapabilities.contentFormat: MarkupKind[]`
- Response `Hover.contents.kind` must match advertised support

**ParameterInformation.documentation**: CONFIRMED per LSP spec - field accepts `string | MarkupContent`. MarkupContent requires `kind` field (plaintext or markdown). Current implementation violates this by always returning null.

**SemanticTokensFullOptions type**: Per LSP 3.17, the union is correctly represented as `bool | { delta?: bool }`. Current `Delta { delta: Some(true) }` form is spec-compliant.

## Disposition

### NOT DONE
- **Hover contentFormat negotiation**: Implement. Parse `InitializeParams.capabilities.textDocument.hover.contentFormat`, store in ClientCapabilities, update `sections.rs` to advertise both formats, respect client capability in hover handler.
- **Signature Help documentation**: Implement. Wrap present documentation as MarkupContent `{ "kind": "markdown", "value": "..." }`, respect hover capability negotiation.

### ALREADY FIXED
- **Semantic Tokens delta**: No implementation needed. Feature is complete and routed on origin/main.

## Recommendation

**Reframe issue scope**: Remove semantic tokens delta from acceptance criteria; it's already implemented. This is a 2-part fix (hover + signature help) not a 3-part one. Both are small changes that can land in parallel. Smallest first: hover contentFormat (1 file: sections.rs + capability enum check in hover.rs).

---

<sub>Independent research verified against origin/main HEAD + LSP 3.17 official specification. Claim boundary: file paths, code signatures, and external protocol spec claims only.</sub>
