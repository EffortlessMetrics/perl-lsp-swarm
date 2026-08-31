# File policy

Rust and `xtask` are the default implementation surfaces for this repository.
Non-Rust files are allowed where they serve a clear product, test, release, or CI
purpose, but the Rust 1.95 / 0.14.0 rollout targets receipt-backed file policy so
those surfaces do not expand anonymously.

## Legitimate non-Rust surfaces

The rollout map recognizes these legitimate non-Rust areas:

- Perl fixtures and corpus data.
- Tree-sitter C/native parser bindings.
- VS Code extension surfaces.
- GitHub workflows.
- CI scripts.
- Generated docs/status artifacts.
- Release metadata.

## Target ledger fields

The planned non-Rust allowlist ledger should require each entry to include:

- `id`
- `glob`
- `kind`
- `language`
- `surface`
- `classification`
- `owner`
- `reason`
- `covered_by`
- `created`
- `review_after`

Broad globs should also include `broad_glob_reason`.

## Rollout boundary

Enforcement belongs to the file-policy checker and the authoritative ledger at
`policy/non-rust-allowlist.toml`. The exact candidate-tree inventory is generated
under `target/policy/` and published by CI and documentation builds. No full-tree
inventory is committed: whole-repository counters and sorted path rows are
observations of a tree, not merge authority.
