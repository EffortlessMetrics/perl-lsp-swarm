# Context: #1404 — Semantic: Dead code detection for unused private subroutines

## Problem

In many Perl projects, subroutines starting with an underscore (e.g., `sub _internal_func`) are considered "private" to the package. If such a subroutine is defined but never called within the same project/workspace, it is likely dead code that bloats the codebase. Currently, perl-lsp detects unused variables and parameters via scope analysis, but has no specialized check for unused private subroutines.

**User impact**: Developers cannot use the LSP server to identify dead private subroutine code in their projects, forcing manual review or external tooling (Perl::Critic).

## Why this approach

1. **Heuristic-based detection** in the scope analyzer via IssueKind enum:
   - Minimal intrusion: reuses existing ScopeAnalyzer infrastructure
   - Consistent with existing unused-variable detection
   - Analyzes at single-file scope (workspace-level cross-file would be future enhancement)

2. **Diagnostic code in perl-diagnostics**:
   - Follows established pattern for subroutine-related diagnostics (PL300-PL399 range)
   - Maps to DiagnosticCode enum for LSP integration

3. **Configuration via .perl-lsp.toml**:
   - Users can disable the check (e.g., frameworks like Moose use `_` for magic hooks)
   - Aligns with existing pragma and feature-gate patterns in the codebase

4. **Scope analyzer as entry point** (not workspace index):
   - Single-file analysis matches current ScopeAnalyzer design
   - Workspace-wide cross-file reference checking deferred to future issue

## Alternatives rejected

- **Workspace-wide cross-file detection**: Deferred. Requires tracking reference sites across all workspace files in addition to definitions. Current issue scopes to single-file only.
- **Method-based detection only**: Rejected. Perl has no syntactic distinction between methods and functions; underscore convention applies to both.
- **Exempt all underscore subroutines**: Rejected. Users need granular control via config; blanket exemption would hide real dead code.
- **Integrate into symbol extractor only**: Rejected. Scope analyzer is the right layer for semantic issue detection (matching unused-variable pattern).

## Prior art / duplicates

- **Perl::Critic rule**: `Subroutines::ProhibitUnusedPrivateSubroutines` exists in CPAN but is a linting rule. This implementation is a **native LSP diagnostic** and may differ in scope/strictness.
- **No existing perl-lsp implementation**: Grep of codebase confirms no `UnusedPrivateSubroutine` diagnostic exists yet.

## Links

- Issue: #1404
- Subsystem: `crates/perl-semantic-analyzer` (scope_analyzer) + `crates/perl-diagnostics` + `crates/perl-lsp-rs-core` (diagnostics provider)
- Related: LSP diagnostics pipeline (docs/reference/LSP_IMPLEMENTATION_GUIDE.md)
- PARSER_CONTRACTS.md: N/A — this is a semantic-analysis feature, not a parser contract
- Perl::Critic reference: `Subroutines::ProhibitUnusedPrivateSubroutines`
