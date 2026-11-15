# perl-tidy

Native Rust Perl formatting for command-line and language-server applications.
The library accepts exact source bytes and pure native policy, and returns typed outcomes plus
source-bound byte edit plans. It does not invoke Perl::Tidy or discover tools on PATH.

`perl-tidy-perltidy` owns explicit external compatibility. Executable I/O belongs to the separate
`perl-tidy-cli` integration. Protocol coordinate conversion and request/document currentness belong
to the application adapter.

## Format an exact document

```rust
use perl_tidy::{
    ExactSource, FormatContext, FormatDisposition, FormatRequest,
    NativeFormatConfig, NativeFormatter,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = ExactSource::new("my$x=1;\n");
    let request = FormatRequest::document(&source);
    let typed = NativeFormatter::new().format(
        &source,
        &request,
        &NativeFormatConfig::default(),
        &FormatContext::default(),
    );
    let rendered = typed.apply_exact(&source)?;
    match typed.outcome.disposition {
        FormatDisposition::Applied | FormatDisposition::NoChange => {
            assert_eq!(rendered, "my $x = 1;\n");
        }
        FormatDisposition::Refused | FormatDisposition::FailedOrNotProven => {
            return Err(format!("formatting withheld: {:?}", typed.outcome.reason).into());
        }
    }
    Ok(())
}
```

The result is validated against the same predecessor before its rendered text is used.
Refusal is a separate outcome and carries unchanged text without an edit plan.

## Contracts and limits

`NativeFormatConfig` is style policy only. It cannot select an external executable, a profile,
an environment, a timeout, or disabled operation. Applications select their formatter before
constructing a native request. Native admission accepts indentation widths 1–16 and line widths
1–1000; values outside those bounds are explicitly refused before parsing or allocating indentation.
This is deliberate numeric hardening of the previous renderer. Within the admitted native policy
and target set, the existing layout renderer remains the sole output authority. Legacy raw range
entries now share typed admission and may refuse selections that bypassed it before extraction.

`ExactSource` binds UTF-8 text to shared `ContentDigest` identity and optional caller-owned logical
source and generation. `SourceByteRange` is half-open and never clamps endpoints. `FormatRequest`
keeps requested and admitted bytes separate, with explicit provenance for any caller widening.
The engine currently admits complete LF/CRLF line bodies for range layout. Unsupported partial
line targets refuse. Exact bare-CR partial targets that the LF-based renderer cannot represent
are refused; extraction does not add bare-CR layout support.

Every applied result includes a `FormatEditPlan`. `apply_exact` revalidates predecessor identity,
UTF-8 boundaries, request admission, ordered non-overlapping edits, coverage, terminal consistency,
and the resulting digest before returning rendered text. `apply_to` is the anonymous-content
convenience and deliberately rejects a result bound to a named source or known generation.
Serialization transports evidence; it does not prove receipt authenticity or live currentness.
The application still owns checking its current document revision before applying edits.

The existing parse gates, literal-preservation limits and typed refusal classes remain in force.
No fallback formatter or second layout algorithm is included. Pipeline operation counters remain
deterministic evidence; wall-clock benchmarks are advisory. Allocation bounds and algorithmic complexity are not proven by these tests, and #10302 remains open for that work.

The public `native` module, `FormatDoc` IR and pipeline counters remain part of the current 0.x
migration surface. The public API ratchet records those exports; it does not freeze their support
forever. Existing exhaustive enums still require exhaustive matches unless callers choose a
wildcard. Any future removal or incompatible reshaping follows the published minor-release policy.

## Ownership

| Boundary | Owner | Reason |
| --- | --- | --- |
| Parsing, native layout, policy validation, typed outcomes and byte edit plans | `perl-tidy` | Every consumer receives one native result and one independently applicable plan. |
| External arguments and text execution through an explicitly supplied runtime | `perl-tidy-perltidy` | Native users do not acquire process or tool-discovery dependencies. |
| UTF-16 conversion, editor options, live document checks and engine selection | LSP application adapter | Protocol and session policy remain above the engine. |
| Former mode-bearing config and UTF-16 result signatures | `perl-lsp-perltidy` | Published callers can migrate without retaining another formatter. |
| Command-line input, output, check and write transactions | Separate `perl-tidy-cli` integration | Filesystem effects do not become library policy. |

`FormatContext` and `FormatIdentity` retain adapter labels for reporting. They cannot mint a logical
source identity. Use `ExactSource::with_identity` with the actual resolver-owned identity and
generation when those exist; the `&str` convenience entry points deliberately use anonymous content.
Live configuration and document currentness remain caller-owned.

The compatibility facade targets removal in 0.18.0 under #9292 after downstream and external
acceptance. Profile resolution, cancellation, production process supervision and installed CLI
product evidence remain separate acceptance work; this extraction does not close those controllers.
