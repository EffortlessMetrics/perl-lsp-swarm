# Dynamic Perl constructs

The parser recognizes dynamic Perl syntax without executing it. A valid AST
does not establish which source, files, or symbols will exist at runtime.
Completion, navigation, and refactoring can therefore be incomplete even when
the surrounding Perl parses successfully.

## Construct boundaries

| Construct | What static analysis can establish | Current limitation |
| --- | --- | --- |
| `eval { ... }` | The block is parsed as Perl source. | Runtime effects and exception outcomes remain unknown; the block itself is not opaque string eval. |
| `eval "..."`, `eval $code` | The argument expression is parsed. Some literal `eval 'sub NAME { ... }'` declarations produce low-confidence, name-bearing boundary evidence. | The resulting string is not generally parsed or analyzed as Perl. Symbols and references inside generated code may be absent from completion, definition, and rename results. |
| `do { ... }` | The block is parsed as an ordinary expression. | This is not a file load. |
| `do "config.pl"` | The source contains a literal filename candidate. | Semantic file-dependency tracking is not established. A literal filename or candidate link URI does not verify Perl's runtime target. |
| `do $file` | The operand remains an ordinary variable expression. | The runtime filename and loaded definitions are unknown. Do not interpret a definition result for `$file` as navigation to the loaded file. |
| `AUTOLOAD` | The handler can be detected; supported method lookup paths can identify dynamic dispatch through it. | Arbitrary method names handled at runtime are not indexed as source declarations. A handler location is not an exact definition of every generated method. |
| `*{$name} = sub { ... }` | The parser retains the typeglob and its expression; compiler lowering can record a dynamic stash mutation. | The installed symbol name is unknown. Dynamic brace-containing names are omitted from the static symbol-reference stream rather than exposed as concrete symbols. Static typeglobs such as `*foo` retain their own reference handling. |
| Heredocs containing SQL, Template Toolkit, or other template text | The heredoc is a Perl string literal with source boundaries. | Its embedded language is not syntax-checked as Perl or validated by an embedded-language tool. Large literal bodies do not make the surrounding Perl unparseable. |

Double-quoted eval arguments also interpolate before runtime compilation. That
interpolation and the code produced by it are separate concerns: recognizing an
outer variable does not establish the meaning of the resulting Perl program.

## How the LSP degrades

Where matching semantic evidence reaches a provider, a dynamic boundary can
produce an explanatory hover, a fallback navigation result, or a blocked
refactoring plan. Completion may omit runtime-generated symbols. `AUTOLOAD`
hover paths explicitly describe dynamic dispatch. These answers explain the evidence
available to the provider; they do not certify the runtime-generated symbol.

Coverage is partial. In particular, a hover or definition request inside
arbitrary string-eval content may still return no result. The live workspace
fact shard currently extracts named-sub evidence from literal eval strings;
that does not provide a covering boundary occurrence for every string eval or
eval heredoc. The semantic hover cutover does not currently query an entity-less,
position-scoped boundary before falling back on an absent symbol. A compiler
HIR or project-model boundary alone therefore does not guarantee a user-visible
hint at that position.

The heredoc anti-pattern diagnostic `PL805` can report a heredoc in an eval
string. That diagnostic is separate from general semantic boundary publication
and from SQL or template validation. It does not establish that hover,
completion, or rename understands the embedded code.

`do EXPR` likewise has a conservative compiler HIR boundary, but its public
source-form classification and provider-facing file-load behavior remain
separate work. A literal filename, computed filename, and `do BLOCK` must retain
different meanings. The LSP does not execute a project expression to discover a
file or generated symbol.

An [alternative document-link helper](../../crates/perl-lsp-rs/src/features/lsp_document_link.rs)
can construct a file URI for a literal `do` path. The [active document-link dispatch](../../crates/perl-lsp-rs/src/runtime/dispatch/text_document.rs)
calls a different provider; helper capability alone does not establish live
`do` navigation or semantic dependency tracking.

## Working with dynamic code

- Review eval strings, symbolic dispatch, and symbol-table writes manually when
  renaming. Automatic edits cannot account for every runtime reference; see
  [rename limitations](RENAME_LIMITATIONS.md).
- Use explicit declarations in analyzable Perl modules where editor navigation
  and completion need stable symbol identities.
- Validate loaded files and template bodies with the relevant language or
  template tooling. A successful parse of the containing Perl file does not
  validate their contents.

## Implementation references

These are distinct evidence layers, not interchangeable completion claims:

- [Parser control flow](../../crates/perl-parser-core/src/engine/parser/control_flow.rs)
  preserves eval/do blocks and argument expressions; [HIR lowering](../../crates/perl-parser-core/src/hir/lower.rs)
  records compiler-local dynamic boundaries.
- [Project-model import walking](../../crates/perl-workspace-core/src/import_walk.rs)
  records string-eval and typeglob boundaries. [Literal eval-sub extraction](../../crates/perl-workspace/src/semantic/eval_sub_extractor.rs)
  supplies the narrower named-sub evidence to the live workspace shard.
- [Symbol-reference extraction](../../crates/perl-symbol/src/surface/ref.rs)
  omits dynamic typeglob names from concrete references.
- [Semantic hover cutover](../../crates/perl-lsp-rs-core/src/providers/navigation/hover_shadow.rs),
  [live hover routing](../../crates/perl-lsp-rs/src/runtime/language/hover/live_compiler_hover.rs),
  and [heredoc diagnostics](../../crates/perl-lsp-rs-core/src/providers/diagnostics/heredoc_antipatterns.rs)
  determine which explanations reach editor requests.

The remaining provider and per-construct acceptance work is tracked by
[#1761](https://github.com/EffortlessMetrics/perl-lsp-swarm/issues/1761).
