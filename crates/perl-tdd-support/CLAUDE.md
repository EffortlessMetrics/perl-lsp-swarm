# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

`perl-tdd-support` is a **Tier 2 testing utility crate** (single-level dependency on `perl-parser-core`) providing TDD helpers, test generation, and safe assertion utilities for the Perl LSP workspace.

**Purpose**: Safe unwrap replacements (`must`/`must_some`/`must_err` and their context-preserving `must_with`/`must_some_with`/`must_err_with` counterparts), Perl test code generation from ASTs, test discovery/execution with TAP parsing, TDD workflow state management, refactoring analysis, and ignored test governance.

**Version**: workspace (currently 0.12.3)

## Commands

```bash
cargo build -p perl-tdd-support          # Build this crate
cargo test -p perl-tdd-support           # Run tests
cargo clippy -p perl-tdd-support         # Lint
cargo doc -p perl-tdd-support --open     # View documentation
```

## Architecture

### Dependencies

- `perl-parser-core` -- Core parser types (`Node`, `NodeKind`, `Parser`, `ParseResult`)
- `serde`, `serde_json` -- Serialization for governance types and test JSON output
- `anyhow` -- Error handling

### Optional Dependencies (feature `lsp-compat`)

- `lsp-types` -- LSP code actions and diagnostics conversion
- `url` -- URI handling for LSP integration

### Key Modules

| Module | Purpose |
|--------|---------|
| `must` | `must()`, `must_some()`, `must_err()` -- safe unwrap replacements using `#[track_caller]` |
| `tdd::test_generator` | `TestGenerator` (multi-framework), `TestRunner` (prove/perl), `RefactoringSuggester` |
| `tdd::tdd_basic` | Simplified `TestGenerator`, `RefactoringAnalyzer`, `TddWorkflow` state machine |
| `tdd::tdd_workflow` | Full TDD workflow manager with coverage tracking, config, and LSP integration |
| `tdd::test_runner` | `TestRunner` for AST-based test discovery and TAP execution |
| `governance` | `IgnoredTestGuardian`, baseline tracking, quality gates, trend reporting |

### Re-exports from `perl-parser-core`

The crate re-exports `Node`, `NodeKind`, `SourceLocation`, `ParseError`, `ParseResult`, `Parser`, `ast`, `position`, `error`, `parser` at the crate root.

### Features

| Feature | Purpose |
|---------|---------|
| `default` | Core TDD functionality |
| `lsp-compat` | Enables `lsp-types` and `url` for LSP code action/diagnostic conversion |

## Usage

### Safe unwrap replacements (used across workspace tests)

```rust
use perl_tdd_support::{must, must_some, must_err};

#[test]
fn test_example() {
    let ast = must(parser.parse());
    let symbol = must_some(ast.find_symbol("foo"));
    let err = must_err(parser.parse_bad_input());
}
```

The context-preserving counterparts are re-exported too, and are the correct form
whenever the call site previously carried an `.expect("…")` explanation — the bare
helpers drop it from the panic diagnostic ([#14291]):

```rust
use perl_tdd_support::{must_with, must_some_with, must_err_with};

#[test]
fn test_example_with_context() {
    let ast = must_with(parser.parse(), "the fixture parses");
    let symbol = must_some_with(ast.find_symbol("foo"), "the fixture declares foo");
}
```

`cargo xtask ci-hygiene check-must-context` reports a change that removes an
`.expect("…")` and puts a bare helper in its place.

[#14291]: https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/14291

### Test generation from AST

```rust
use perl_tdd_support::test_generator::{TestGenerator, TestFramework};

let generator = TestGenerator::new(TestFramework::TestMore);
let test_cases = generator.generate_tests(&ast, source);
```

### TDD workflow state machine

```rust
use perl_tdd_support::tdd_basic::{TddWorkflow, TddState};

let mut workflow = TddWorkflow::new("Test::More");
workflow.start_cycle("add");        // -> Red
workflow.run_tests(true);           // -> Green
workflow.start_refactor();          // -> Refactor
workflow.complete_cycle();          // -> Idle
```

### Test discovery and execution

```rust
use perl_tdd_support::test_runner::TestRunner;

let runner = TestRunner::new(source, uri);
let tests = runner.discover_tests(&ast);  // finds test_* functions
let results = runner.run_test("file:///t/basic.t");  // runs with prove/perl, parses TAP
```

## Important Notes

- The `must` module has `#![allow(clippy::panic)]` since these helpers intentionally panic in tests
- The crate re-exports core parser types so test files can import from one place
- `test_generator` contains two parallel implementations: a full one with `TestFramework` enum and `RefactoringSuggester`, and a simplified one in `tdd_basic`
- `test_runner::TestRunner` requires source+URI at construction and executes tests via subprocesses
- `test_generator::TestRunner` is **generation-only / non-executing** until convergence with #4972; its `run_tests`, `watch`, and `get_coverage` fail closed rather than fabricating results
- The `governance` module is data-model heavy (serializable structs for CI integration)
- Used as a `dev-dependency` or test utility across the workspace
