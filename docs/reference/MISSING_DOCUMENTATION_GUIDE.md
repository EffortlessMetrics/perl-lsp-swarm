# Public API Documentation and Examples

*Diátaxis: reference and how-to guidance*

This guide defines how to improve Rust API documentation in the current
workspace. It is deliberately narrower than a generic documentation style
guide: the important contract is that a contributor can find the public crate
surface, understand its limits, and tell whether a code example is intended to
compile.

## Scope

This guide applies to published and user-facing library crates, especially:

- `perl-parser` and `perl-parser-core`;
- `perl-lexer`, `perl-ast`, and `perl-uri`;
- `perl-semantic-analyzer` and `perl-semantic-facts`;
- `perl-lsp-rs` and `perl-lsp-rs-core`;
- `perl-dap`;
- `tree-sitter-perl-rs` and `tree-sitter-perl-c`.

The workspace has been consolidated over time. Do not copy crate names,
module paths, constructors, or examples from historical issues, old branches,
or pre-collapse documentation without checking the current public manifest and
implementation.

## What good public documentation must establish

A public item needs enough information for a downstream user to answer:

1. What does this item represent or do?
2. What inputs does it accept, and what invariants must callers provide?
3. What does success return?
4. How are errors, unsupported syntax, stale state, cancellation, or
   fallback represented?
5. Is the item intended for downstream use, an internal implementation seam,
   or a compatibility surface?
6. Which neighbouring public types or commands should the user use next?

Add `# Errors`, `# Panics`, `# Safety`, or `# Examples` sections when they
carry real contract information. Do not add boilerplate sections that merely
repeat the signature.

For editor-facing APIs, document the claim boundary explicitly. For example,
distinguish:

- parse success from semantic correctness;
- a returned fallback from an exact answer;
- a static fact from runtime execution;
- a diagnostic report from a merge or release gate;
- a supported configuration from a best-effort compatibility path.

Avoid performance numbers unless the repository has a current, reproducible
receipt or benchmark that supports them.

## Examples are contracts

Classify every Rust example in public documentation as one of these:

| Class | Use when | Required treatment |
|---|---|---|
| Runnable doctest | The example is self-contained and should execute | Keep it compilable and run `cargo test --doc` |
| Compile-checked example | Compilation matters but execution would need an external editor, Perl installation, process, or fixture | Use `no_run` or represent it in a package-consumer fixture |
| Compile-fail contract | The example documents an intentional type or API rejection | Use `compile_fail` and make the failure narrow and stable |
| Schematic example | The text is intentionally pseudocode or depends on unavailable external state | Mark it `ignore` only with prose explaining why |
| Non-Rust example | The snippet is configuration, JSON, Perl, shell, or protocol text | Validate it with the relevant fixture or command when one exists |

A copy-paste Rust example that imports an absorbed crate, calls a removed
constructor, or names an internal-only module is not a schematic example. Fix
it, remove it, or replace it with a truthful conceptual example. Do not hide a
broken example behind `ignore`.

Examples for published crates should use the crate and type paths that a
downstream consumer receives. Workspace-only paths are acceptable only when
the surrounding text clearly labels the example as internal contributor
material.

## Documentation workflow

Use the smallest proof that can falsify the change.

### 1. Establish the current surface

Before writing an example or documenting a public item:

```bash
cargo metadata --format-version=1 --no-deps
cargo tree -p <crate>
rg 'pub(\\s+(async|unsafe|const|extern\\s+"[^"]+"))*\\s+(fn|struct|enum|trait|type|const|static)|pub\\s+use' crates/<crate>/src
```

Read the owning crate's `lib.rs`, its public re-exports, and the nearest
tests. The inventory expression includes qualified declarations such as
`pub async fn`; confirm matches against the syntax and re-export surface rather
than treating a text search as proof of completeness. If the item is part of a
generated or compatibility surface, identify the source of truth before editing
prose.

### 2. Write the smallest useful contract

Prefer a short summary plus the one or two details that change how a caller
uses the item. Include a minimal example only when it teaches the actual
entry path. Document limitations at the point where a user would otherwise
infer a stronger claim.

For parser and semantic APIs, examples should normally show:

- the authoritative crate import;
- the current constructor or entry function;
- the result/error handling shape;
- whether recovery nodes, dynamic boundaries, or unresolved state are
  expected.

For LSP and DAP APIs, examples should normally show:

- the protocol or server entry point;
- the lifecycle/configuration preconditions;
- cancellation or cleanup expectations where relevant;
- the difference between a library facade and the executable server.

### 3. Run documentation proof

For a focused crate:

```bash
cargo fmt -p <crate> -- --check
cargo test -p <crate> --doc --locked
cargo doc -p <crate> --no-deps --locked
```

For a public-surface or dependency change, also run the affected package tests
and the repository's applicable documentation or package gate. A successful
`cargo doc` proves that documentation can be rendered; it does not prove
that every fenced example is a valid downstream example.

### 4. Check packaged-consumer examples

A crate may disable ordinary doctests because of dependency cycles, generated
configuration, platform requirements, or compile cost. That setting does not
make public examples self-validating.

When `doctest = false` remains necessary:

1. record the crate, reason, owner, and review/exit condition;
2. inventory the public Rust examples;
3. map each example to a doctest, `no_run` check, or compiled package-consumer
   fixture;
4. compile the fixture against the packaged public surface where practical;
5. keep external-process and editor-dependent steps out of the compile claim.

The fixture must prove the package a user receives, not only an unusually
permissive workspace dependency graph.

## Review checklist

A documentation change is ready when:

- public imports and constructors match the current crate surface;
- no historical absorbed-crate name appears in a current copy-paste example;
- examples are classified and have an appropriate proof path;
- errors, unsupported behavior, and fallback boundaries are not implied away;
- links point to current files and anchors;
- claims are current-head or explicitly versioned;
- generated/status documents were regenerated when their source contract changed;
- `git diff --check` passes;
- the focused documentation proof is recorded in the PR.

## What this guide does not establish

This guide does not claim that all public items are documented, that all doctests
are enabled, or that every published crate currently has a complete
consumer-fixture ledger. Those are measurable follow-up claims owned by the
documentation-quality work, including issue #2318 and the compiled-example
contract in issue #4947.

It also does not turn documentation into a release gate by itself. Release
readiness, publication, and support claims remain governed by their own
current evidence and repository policy.
