# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Crate Overview

- **Name**: `perl-semantic-analyzer`
- **Version**: tracks the workspace version.
- **Tier**: 4 (three-level internal dependencies)
- **Purpose**: Semantic analysis, symbol extraction, type inference, scope analysis, and dead code detection for Perl source code. Provides the core analysis layer consumed by LSP provider crates.

## Commands

```bash
cargo build -p perl-semantic-analyzer        # Build
cargo test -p perl-semantic-analyzer         # Run tests
cargo clippy -p perl-semantic-analyzer       # Lint
cargo doc -p perl-semantic-analyzer --open   # View documentation
```

## Architecture

### Dependencies

| Crate | Role |
|-------|------|
| `perl-parser-core` | AST nodes (`Node`, `NodeKind`), `Parser`, source locations |
| `perl-lexer` | Deferred source-only stray-cut facts for shared document analysis |
| `perl-pragma` | Pragma timelines shared by scope analysis and native critic consumers |
| `perl-workspace` | Cross-file workspace index, `SymKind`, `SymbolKey` |
| `perl-symbol` | Shared `SymbolKind`, `VarKind` enums |
| `regex` | Pattern matching in semantic classification |
| `rustc-hash` | `FxHashMap` for fast scope/parent-map lookups |
| `serde` | Serialization for dead code analysis results |

### Modules (`src/analysis/`)

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `argument_cardinality` | `known_scalar_argument_count` | Conservative scalar value counts in Perl list context, shared by critic, diagnostics, and quick-fix admission |
| `document_analysis` | `DocumentDiagnosticAnalysis` | Generation-owned lazy pragma, scope, symbol, type and lexer facts; exact AST/source matching |
| `semantic` | `SemanticAnalyzer`, `SemanticToken`, `SemanticTokenType`, `SemanticTokenModifier`, `HoverInfo` | Semantic token classification and hover info for LSP |
| `symbol` | `SymbolExtractor`, `SymbolTable`, `Symbol`, `SymbolReference`, `Scope`, `ScopeKind`, `ScopeId` | Symbol extraction and symbol table construction from AST |
| `scope_analyzer` | `ScopeAnalyzer`, `ScopeIssue`, `IssueKind` | Scope issue detection (unused/undeclared/shadowed variables) |
| `unreachable_code` / `unreachable_code_disposition` | `collect_unreachable_spans`, existing `Pl406*` disposition types (crate-root exports) | One protocol-neutral local flow walk and its canonical traversal coverage; LSP and native critic own output projection |
| `test2` (private; selected root exports) | `Test2Facts`, `Test2FactsAnalysis`, `Test2Pragmas`, `Test2ResolvedImport` | Existing Test2 import/pragma compatibility reader shared by completion and critic; canonical adapter migration remains #4907/#6946/#6948/#7661/#6981 |
| `type_inference` | `TypeInferenceEngine`, `TypeEnvironment`, `PerlType`, `ScalarType`, `TypeConstraint` | Type inference with scoped environments |
| `declaration` | `DeclarationProvider`, `LocationLink`, `ParentMap` | Go-to-declaration with parent-map AST traversal |
| `dead_code_detector` | `DeadCodeDetector`, `DeadCode`, `DeadCodeType`, `DeadCodeAnalysis`, `DeadCodeStats` | Workspace-level dead code detection (non-WASM only) |
| `index` | `WorkspaceIndex`, `SymbolDef` | Local cross-file symbol index (non-WASM only) |

`DocumentDiagnosticAnalysis` is exported at the crate root. Its implementation
module remains crate-private so `perl-parser`'s analysis wildcard re-export does
not widen the parser's public API.

### Framework interpretation (`src/framework_adapters/`)

This crate owns concrete Dancer2, Moose, Mojo::Base, Mojolicious, and DBIx::Class
descriptors, detection, import interpretation, and fact production. The public
module is at the crate root, outside `analysis`, to keep parser compatibility
exports bounded. `perl-semantic-facts::framework_adapters` owns the stable types,
IDs, provenance, validation contracts, and normative reference tables. Import those
contracts directly; do not copy tables or add a facts-to-analyzer dependency.

The QuickORM API registry remains entirely in the facts crate. See
`../../docs/reference/FRAMEWORK_ADAPTER_OWNERSHIP_MIGRATION.md` for the public
function migration and the retained identity laws. Adapter tests live here;
table, identity, serialization, and checked-SDK contract tests stay below.

### Re-exports from `lib.rs`

The crate re-exports core types from `perl-parser-core` (`Node`, `NodeKind`, `SourceLocation`, `Parser`, `ast`, `error`, etc.) and `perl-workspace::workspace_index`, so downstream crates can use a single import.

`known_scalar_argument_count` is exported at the crate root. Its implementation
module stays private so `perl-parser::analysis` does not acquire another public
module through its compatibility wildcard export.

## Usage Examples

### Symbol Extraction

```rust
use perl_semantic_analyzer::{Parser, analysis::symbol::SymbolExtractor};

let mut parser = Parser::new("sub hello { my $x = 1; }");
let ast = parser.parse()?;
let table = SymbolExtractor::new().extract(&ast);
assert!(table.symbols.contains_key("hello"));
```

### Semantic Analysis

```rust
use perl_semantic_analyzer::analysis::semantic::SemanticAnalyzer;

let analyzer = SemanticAnalyzer::analyze(&ast);
let tokens = analyzer.semantic_tokens();  // For LSP highlighting
let hover = analyzer.hover_at(offset);    // For LSP hover
```

### Scope Analysis

```rust
use perl_semantic_analyzer::analysis::scope_analyzer::{ScopeAnalyzer, IssueKind};

let analyzer = ScopeAnalyzer::new();
let issues = analyzer.analyze(&ast, source, &pragma_map);
for issue in &issues {
    // IssueKind: UnusedVariable, VariableShadowing, UndeclaredVariable, etc.
}
```

### Type Inference

```rust
use perl_semantic_analyzer::analysis::type_inference::{TypeInferenceEngine, TypeEnvironment};

let mut engine = TypeInferenceEngine::new();
let result = engine.infer(&ast);
```

## Important Notes

- Modules `dead_code_detector` and `index` are gated behind `#[cfg(not(target_arch = "wasm32"))]`.
- Source files are large (40-80KB) reflecting the complexity of Perl semantics; this is intentional.
- `SymbolKind` and `VarKind` are re-exported from `perl-symbol` (shared across the workspace).
- The crate bans `unwrap()`, `expect()`, `panic!()`, `todo!()` in non-test code via workspace lints.
- Doctests are disabled (`doctest = false` in Cargo.toml).
