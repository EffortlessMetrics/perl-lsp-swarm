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
| string-value-object-form | Object-form `StringValue` inline insert text | implementation-owner | #858 | #773 / PR #776; discarded candidate PR #861 | #11803 / PR #12096 | undefined |
| multi-range-formatting | Multi-range formatting | implementation-owner | #7089 | focused completion owner #10248 | #11803 / PR #12096 | undefined |
| generated-code-action-tags | `CodeAction.tags` and `CodeActionTag.LLMGenerated` | accepted-disposition | n/a | #598 / PR #601; deterministic-action ruling #4209 | a reviewed generated-action source | undefined |
| command-tooltip-non-codelens | `Command.tooltip` | implementation-owner | #13633 | completed CodeLens slice #511 | #11803 / PR #12096 | undefined |
| relative-pattern-document-selector | `RelativePattern` watcher registrations | accepted-disposition | n/a | protocol-identity correction owner #8897 | matrix identity split in #8897; protocol types #11803 / PR #12096 | undefined |
| markdown-command-theme-icons | Markdown command links and theme-icon syntax guard | accepted-disposition | n/a | claim authority #6731; boundary spec PLSP-SPEC-0029 | a separate editor-specific proposal | undefined |
| notebook-318-additions | Notebook 3.18 additions | accepted-disposition | n/a | claim authority #6731; bounded selector ruling #8897 | a concrete Perl notebook document model and editor need | undefined |

## Update rule

A PR that changes any row above must update the matrix, this ledger, the relevant issue/evidence references, and both LSP 3.18 tests in the same change. Removing a negative gate requires positive wire proof; accepting a new absence requires an explicit rationale rather than a free-form “later” status.
