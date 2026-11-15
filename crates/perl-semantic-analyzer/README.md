# perl-semantic-analyzer

Semantic analysis for Perl source.

Use this crate when you already have an AST and need symbols, scopes, types,
declaration lookup, or dead-code signals before you get to editor features.

## Where it fits

`perl-semantic-analyzer` sits above `perl-parser-core` and next to
`perl-workspace`. It turns parsed trees into analysis artifacts that the
workspace, refactoring, and LSP layers can consume.

## Key entry points

- `analysis::symbol::SymbolExtractor`
- `analysis::scope_analyzer::ScopeAnalyzer`
- `analysis::type_inference::TypeInferenceEngine`
- `analysis::semantic::SemanticAnalyzer`
- `analysis::declaration::DeclarationProvider`
- `analysis::index::WorkspaceIndex`
- `known_scalar_argument_count`
- `framework_adapters` -- Dancer2, Moose, Mojo::Base, Mojolicious, and DBIx::Class
  detection, import interpretation, and fact production over shared
  `perl-semantic-facts` contracts.

`known_scalar_argument_count` supplies the shared conservative argument count for
critic, diagnostics, and quick-fix admission. It returns `None` when any argument
has unknown cardinality in Perl list context, including calls, arrays, and command
output. An empty argument list returns `Some(0)`.

Framework contracts and normative tables remain in `perl-semantic-facts`.
The [API migration map](../../docs/reference/FRAMEWORK_ADAPTER_OWNERSHIP_MIGRATION.md)
lists the concrete implementation functions now owned by this crate. The public
`framework_adapters` module is at the crate root, outside `analysis`, so the parser
compatibility wildcard does not acquire an adapter implementation namespace.

## Example

```rust
use perl_parser_core::Parser;
use perl_semantic_analyzer::analysis::symbol::SymbolExtractor;

let mut parser = Parser::new("sub hello { my $x = 1; }");
let ast = parser.parse()?;
let _symbols = SymbolExtractor::new().extract(&ast);
```

## Typical use

Use `perl-semantic-analyzer` when you are building hover, declaration, type,
or diagnostics features from parsed Perl source. If you need cross-file lookup
or document caching, pair it with `perl-workspace`.

## Architecture notes

- [Dependency boundary audit](docs/dependency-boundary-audit.md)

## Local unreachable-code facts

`collect_unreachable_spans(&Node)` returns ordered source spans from one local
control-flow analysis. The semantic analyzer also owns the existing `Pl406*`
per-variant disposition registry. These crate-root exports keep implementation
modules private to the analysis layer. LSP PL406 diagnostics and native critic
findings project the same facts into their own messages, severity and remediation.
The lower analysis does not depend on either output representation.
