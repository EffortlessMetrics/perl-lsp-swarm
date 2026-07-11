# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-pod`
- **Version**: workspace (inherits)
- **Tier**: 2 (zero-dependency leaf utility)
- **Purpose**: POD (Plain Old Documentation) extractor — parses Perl `.pm` source files and returns structured `PodDoc` suitable for LSP hover display.

## Commands

```bash
cargo build -p perl-pod           # Build
cargo test -p perl-pod            # Run tests
cargo clippy -p perl-pod          # Lint
cargo doc -p perl-pod --open      # View documentation
```

## Architecture

The entire implementation lives in a single file:

| File | Purpose |
|------|---------|
| `src/lib.rs` | All parsing logic (~720 lines); `PodDoc`, `extract_pod()`, `extract_pod_from_file()` |

**Zero production dependencies.** (`tempfile` is dev-only.)

### Public API

| Item | Signature | Purpose |
|------|-----------|---------|
| `PodDoc` | struct | Extracted documentation: `name`, `synopsis`, `description`, `methods: HashMap<String, String>`, `arguments`, `return_values`, `examples`, `see_also`, `is_empty()` |
| `extract_pod` | `fn(source: &str) -> PodDoc` | Parse POD from a string |
| `extract_pod_from_file` | `fn(path: &Path) -> io::Result<PodDoc>` | Read file and parse POD |

## POD Handling

### Supported directives

- `=head1` sections: NAME, SYNOPSIS, DESCRIPTION, ARGUMENTS, RETURN VALUES, EXAMPLES, SEE ALSO
- `=head2` sections: method documentation (keyed into `methods` map by section name)
- `=over` / `=item` / `=back`: lists
- `=cut`, `=pod`, `=encoding`, `=begin` / `=end` / `=for`

### Inline formatting codes

Strips all POD inline codes: `B<>`, `I<>`, `C<>`, `L<>`, `F<>`, `S<>`, `E<>`, `X<>`, `Z<>` — including multi-angle-bracket forms like `C<< $obj->method >>`.

- `L<>` links → markdown `[display](perl-module://target)` for LSP clickability
- `E<>` → decoded entity (named + numeric: decimal, `0x` hex, octal)

### Safety

- `MAX_POD_FORMATTING_DEPTH = 100` prevents stack overflow from deeply nested inline codes (tested at 5000 levels)

## Does NOT own

- POD-to-HTML rendering (not implemented here)
- Inline-code syntax highlighting
- Cross-reference resolution for `L<>` links

## Neighbors

| Crate | Relationship |
|-------|-------------|
| `perl-workspace-core` | Consumes `PodDoc` via `PodFact` / `PodSection` for workspace model facts |
| `perl-lsp-rs-core` | Drives hover display using extracted `PodDoc` |

## Important Notes

- `#![deny(unsafe_code)]`, `#![warn(rust_2018_idioms)]`, `#![warn(missing_docs)]`
- All new POD directives or formatting codes should be added to the single `src/lib.rs` file
- Tests should be added as `#[test]` blocks within `src/lib.rs` or a separate `tests/` file
