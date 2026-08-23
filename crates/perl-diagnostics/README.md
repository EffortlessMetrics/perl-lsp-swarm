# perl-diagnostics

Unified diagnostic codes, transport-neutral byte spans, types, and catalog for Perl LSP.

This crate consolidates three previously separate diagnostic crates into a
single, coherent API:

| Former crate | Now |
|---|---|
| `perl-diagnostics-codes` | `perl_diagnostics::codes` |
| `perl-lsp-diagnostic-types` | `perl_diagnostics::types` |
| `perl-lsp-diagnostic-catalog` | `perl_diagnostics::catalog` |

## Modules

- **`codes`** — canonical `DiagnosticCode`, `DiagnosticCategory`,
  `DiagnosticSeverity`, and `DiagnosticTag` enums. All other modules derive
  their severity/tag types from here; there is exactly one definition in the
  workspace.

- **`types`** — validated `ByteSpan`, `Diagnostic`, and `RelatedInformation`
  types. `DiagnosticSeverity` and `DiagnosticTag` are re-exported from `codes`
  so the legacy `types::DiagnosticSeverity` import path still resolves to the
  same type.

- **`catalog`** — metadata helpers that map a `DiagnosticCode` to its
  human-readable message, related documentation URL, and default severity.

All public items are additionally re-exported from the crate root via the
`api` module so consumers need only `use perl_diagnostics::*`.

## Byte-span contract

`ByteSpan` is a validated half-open UTF-8 byte interval `[start, end)`:

- `start <= end` is required;
- zero-width spans are deliberate and supported;
- reversed spans are rejected, never swapped or clamped;
- source length and UTF-8 scalar boundaries are checked against the exact
  source snapshot by the consumer;
- LSP line/column, URI, and negotiated position-encoding policy stay outside
  this crate.

## Usage

```toml
[dependencies]
perl-diagnostics = { path = "../../crates/perl-diagnostics" }
# Enable serde support for JSON serialization:
# perl-diagnostics = { path = "...", features = ["serde"] }
```

```rust
use perl_diagnostics::{ByteSpan, Diagnostic, DiagnosticCode, DiagnosticSeverity};

let span = ByteSpan::new(0, 12)?;
let diagnostic = Diagnostic::new(
    DiagnosticCode::MissingStrict,
    DiagnosticSeverity::Warning,
    span,
    "Missing 'use strict'",
);
# Ok::<(), perl_diagnostics::InvalidByteSpan>(())
```

## Type unification

`DiagnosticSeverity` and `DiagnosticTag` are defined once in `codes` and
re-exported through `types`. This means `types::DiagnosticSeverity` and
`codes::DiagnosticSeverity` are the same type — no orphan-impl issues and no
`From` conversion is needed when passing values between modules.

## Features

| Feature | Default | Effect |
|---|---|---|
| `serde` | off | Enables stable serialization/deserialization for public types |
