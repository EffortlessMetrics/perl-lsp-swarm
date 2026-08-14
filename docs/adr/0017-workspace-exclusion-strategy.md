# ADR-0017: Workspace and Archive Strategy

**Status**: Accepted
**Date**: 2025-02-15
**Decision Makers**: Perl LSP Architecture Team
**Related**: [ARCHITECTURE_OVERVIEW.md](../reference/ARCHITECTURE_OVERVIEW.md), [CRATE_ARCHITECTURE_GUIDE.md](../reference/CRATE_ARCHITECTURE_GUIDE.md)

## Context

The Perl LSP workspace contains crates with different build requirements. Current Tree-sitter compatibility crates are maintained workspace members, while legacy parser sources and specialized tooling are kept outside the default workspace.

1. **Build-surface clarity**: Current compatibility crates must be visible to workspace tooling and CI.
2. **Legacy isolation**: Archived or specialized sources should not become default workspace members.
3. **User installation friction**: Published crates should document real prerequisites rather than historical ones.
4. **Development vs production tension**: Internal tooling and published crate requirements remain distinct.

### Problem Statement

The current tree-sitter integration crates have different ownership and build
surfaces:
- `crates/tree-sitter-perl-c` is a workspace member that compiles its vendored C
  grammar with `cc`; it declares the required C symbol by hand and does not use
  bindgen or libclang.
- `crates/tree-sitter-perl-rs` is a workspace member providing the Rust facade.
- `tree-sitter-perl/` is the legacy top-level C parser and remains excluded.

The workspace must describe those current boundaries instead of treating the
maintained compatibility crates as archived.

## Decision

**We keep maintained compatibility crates in the workspace and exclude only legacy or specialized trees.** This makes current source, CI, and package tooling addressable while keeping the legacy parser and archive outside the default build graph.

### Excluded Trees

| Tree | Exclusion Reason |
|-------|------------------|
| `tree-sitter-perl/` | Legacy top-level C parser |
| `fuzz/` | cargo-fuzz specialized requirements |
| `archive/` | Archived legacy components |

### Implementation

The workspace configuration includes the maintained compatibility crates and
excludes the legacy/specialized trees:

```toml
# Cargo.toml workspace configuration
[workspace]
members = [
    "crates/perl-parser",
    "crates/perl-lsp-rs",
    "crates/perl-dap",
    "crates/perl-lexer",
    "crates/tree-sitter-perl-c",
    "crates/tree-sitter-perl-rs",
    # ... other current crates
]

exclude = ["tree-sitter-perl", "fuzz", "archive"]
```

### Architectural Benefits

1. **Current-source visibility**: CI and workspace tooling can build and test maintained compatibility crates.
2. **Legacy isolation**: The old top-level parser and archive cannot silently become current product dependencies.
3. **Honest prerequisites**: The C binding's vendored grammar and `cc` build path are visible; libclang/bindgen are not claimed.
4. **Clear package boundaries**: The Rust facade and C binding remain independently addressable.

## Consequences

### Positive

- **Current workspace coverage**: Maintained Tree-sitter crates are addressable by CI and package tooling.
- **Explicit C boundary**: The C binding's vendored grammar and compiler requirement are visible.
- **Clean legacy boundary**: The old top-level parser remains outside the current workspace.

### Negative

- **C toolchain coverage**: Building the C binding still requires a usable C compiler/toolchain.
- **Legacy separation**: The excluded top-level parser is not covered by the current workspace.
- **Benchmark selection**: Optional benchmark commands may still use explicit manifest paths.
- **Feature parity tracking**: Compatibility tiers remain governed by #4752, not workspace membership alone.

### Mitigations

- Separate benchmark infrastructure for tree-sitter comparison
- Documentation distinguishes current workspace members from excluded legacy trees
- Benchmark scripts handle optional C-toolchain availability gracefully

## References

- [ARCHITECTURE_OVERVIEW.md](../reference/ARCHITECTURE_OVERVIEW.md) - Workspace configuration strategy
- [CRATE_ARCHITECTURE_GUIDE.md](../reference/CRATE_ARCHITECTURE_GUIDE.md) - Excluded crates section
- [status/index.md](../project/status/index.md) - Current workspace status
