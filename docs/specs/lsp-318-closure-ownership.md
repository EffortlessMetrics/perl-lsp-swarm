# LSP 3.18 Closure Ownership

Status: checked
Owner: perl-lsp maintainers
Claim authority: [#6731](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/6731)
Conformance matrix: [lsp-318-conformance-matrix.md](lsp-318-conformance-matrix.md)
Check: `cargo test -p xtask --test lsp_318_closure_ownership --locked`

The conformance matrix classifies the current LSP 3.18 surface. This ledger owns the narrower question the matrix cannot answer by status alone: what closes each negative-gated, not-applicable, or partly unclaimed boundary.

`implementation-owner` means the surface remains intended work and the Owner column pins its owning issue. The Evidence column records historical proof, boundary decisions, and related context separately. `accepted-disposition` means absence is deliberate under the current server model or depends on a source that does not yet exist. Neither disposition claims implementation.

| ID | Matrix feature | Disposition | Owner | Evidence | Dependency | Rationale |
| --- | --- | --- | --- | --- | --- | --- |
| string-value-object-form | Object-form `StringValue` inline insert text | implementation-owner | #858 | #773 / PR #776; discarded candidate PR #861 | #11803 / PR #12096 | The current provider and wire proof remain plain-string-only; object form needs an explicit snippet-kind contract rather than value-string promotion. |
| multi-range-formatting | Multi-range formatting | implementation-owner | #7089 | focused completion owner #10248 | #11803 / PR #12096 | The route and capability are withdrawn until every requested range is composed atomically against one current document version. |
| generated-code-action-tags | `CodeAction.tags` and `CodeActionTag.LLMGenerated` | accepted-disposition | n/a | #598 / PR #601; deterministic-action ruling #4209 | a reviewed generated-action source | Capability parsing and tag filtering exist, but deterministic actions must remain untagged and no generated-action producer currently earns the tag. |
| command-tooltip-non-codelens | `Command.tooltip` | implementation-owner | #13633 | completed CodeLens slice #511 | #11803 / PR #12096 | CodeLens is proven; every other reachable command-producing path still needs an inventory and explicit tooltip policy. |
| relative-pattern-watcher | `RelativePattern` watcher registrations (3.17 watcher globs) | accepted-disposition | n/a | protocol-identity correction owner #8897 | protocol types #11803 / PR #12096 | Watcher RelativePattern is an implemented LSP 3.17 surface. Capable clients receive watcher glob objects rooted at the workspace; unsupported clients retain string-glob fallback. |
| relative-pattern-document-selector | Document-filter `relative pattern` (text-document filters) | accepted-disposition | n/a | protocol-identity correction owner #8897 | a producer that needs relative document filters; protocol types #11803 / PR #12096 | The current text-document filter remains negative-gated because no producer needs it. The watcher RelativePattern identity above is not proof for this distinct LSP 3.18 document-filter surface. |
| relative-pattern-notebook-selector | Notebook document-filter `relative pattern` | accepted-disposition | n/a | protocol-identity correction owner #8897 | a concrete notebook document model and editor need | Notebook-filter support remains absent without a notebook document-selector producer; it is independently gated from text-document filters. |
| markdown-command-theme-icons | Markdown command links and theme-icon syntax guard | accepted-disposition | n/a | claim authority #6731; boundary spec PLSP-SPEC-0029 | a separate editor-specific proposal | These are editor markdown extensions rather than LSP 3.18 capabilities, so absence remains the honest protocol disposition. |
| notebook-318-additions | Notebook 3.18 additions | accepted-disposition | n/a | claim authority #6731; bounded selector ruling #8897 | a concrete Perl notebook document model and editor need | The server keeps only preview LSP 3.17 notebook-sync lifecycle/store evidence, so the 3.18 notebook diagnostic and code-action-kind additions stay unclaimed until a concrete Perl editor need appears; #8897 must not silently expand into a full notebook implementation program. |

## Update rule

A PR that changes any row above must update the matrix, this ledger, the relevant issue/evidence references, and both LSP 3.18 tests in the same change. The generated matrix marks a partly-unclaimed boundary with `<!-- closure: partly-unclaimed -->`; that marker is machine-readable and must be preserved with the relevant row. Removing a negative gate requires positive wire proof; accepting a new absence requires an explicit rationale rather than a free-form “later” status.
