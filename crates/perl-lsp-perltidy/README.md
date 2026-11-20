# perl-lsp-perltidy

Deprecated compatibility facade for existing users of the published formatter package.
All native parsing, layout, classification, counters and policy fingerprints delegate to
[`perl-tidy`](../perl-tidy). External configuration and source-in/text-out execution delegate to
[`perl-tidy-perltidy`](../perl-tidy-perltidy). This package owns no formatter algorithm.

## Migration

The deprecation and removal target is **0.18.0**, the next 0.x compatibility boundary.
Issue #9292 owns actual removal after downstream migration and external-adapter acceptance.
This target does not publish a release or authorize removal before those acceptance conditions.

| Existing surface | Owning API for new callers |
| --- | --- |
| `FormatConfig { mode, ... }` | Select an engine in the application; pass `perl_tidy::NativeFormatConfig` to native |
| `NativeFormatter`, `PerlFormatter`, `FormatDoc` | Equivalent names in `perl_tidy` |
| UTF-16 `TextPosition`, `TextRange`, `TextEdit` | Admit wire coordinates in the application; use `ExactSource`, `SourceByteRange`, `FormatRequest`, `FormatEditPlan` |
| `PerlTidyConfig` | `perl_tidy_perltidy::ExternalPerlTidyConfig` |
| `PerlTidyFormatter` source-in/text-out | `perl_tidy_perltidy::LegacyPerlTidyExecutor` with explicit runtime and resolved arguments |
| `BuiltInFormatter` | `perl_tidy::NativeFormatter` |

The mode-bearing configuration, UTF-16 result shapes, trait and typed formatter signatures remain
available here. `Off` returns unchanged source without running the engine. Passing `ExternalLegacy`
to the native facade is explicitly refused; it never silently runs native formatting. Legacy line
selection is represented as an explicit widening policy before delegating to the byte formatter.
Invalid wire positions, including surrogate interiors, refuse before any native work.

`BuiltInFormatter` now delegates to the same parse-gated native engine. Its retired delimiter scanner
is not retained: malformed or unsupported source follows native refusal instead of receiving the
old scanner's speculative edits. Accepted native policy is bounded to indentation widths 1–16 and
line widths 1–1000; out-of-range policy is refused before parsing or allocation. Raw and typed
range entries now share canonical typed admission, so raw-only reindentation of selections relative
to handwritten surrounding blocks is refused. Full-document reindentation remains supported.
The native range renderer still uses LF/CRLF line bodies: valid bare-CR wire selections that it
cannot represent exactly are refused instead of widening silently. Native content
identity now uses the shared SHA-256 `ContentDigest`; config fingerprints carry a versioned domain.

The original public result struct shapes remain available in this facade. Previously withdrawn
runtime methods remain withdrawn. The old cache, file-in-place and line-range external helpers
remain for compatibility. They are not
the canonical product API, and the legacy subprocess runtime is not a production process supervisor.
New native callers do not depend on this package, tool discovery, executable availability or a
subprocess runtime.

Core application imports through `perl_lsp_rs_core::tooling::perltidy` now expose the canonical byte
API. Callers that require the former UTF-16 DTOs should import this compatibility package explicitly.
The core `FormattingProvider<R>` retains generic and borrowed-runtime construction, with a separate
runtime-free native constructor and pure native policy setter.

## License

MIT OR Apache-2.0
