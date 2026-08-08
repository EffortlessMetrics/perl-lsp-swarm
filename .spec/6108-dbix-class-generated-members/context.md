# Context: #6108 — DBIx::Class relationship and column accessor extraction

## Problem

DBIx::Class is parsed successfully, but the current framework model has no DBIx variant and the generated-member extractor only handles Moo/Moose/Mouse/Class::Tiny `has` declarations and Class::Accessor calls. DBIx schema packages therefore lack synthesized column and relationship members for semantic symbol, completion, and declaration consumers.

## Why this approach

The existing `ClassModelBuilder` already owns framework detection, walks parsed `MethodCall` nodes, and records source locations. The analyzer and workspace `GeneratedMemberExtractor` paths already own deterministic IDs, anchors, provenance, and confidence for their respective consumers. Extending those seams keeps DBIx support additive and lets semantic queries and the LSP completion path reuse the existing fact contract. The missing `GeneratedMemberKind::Method` is made explicit in analyzer facts because relationship methods are semantically distinct from column accessors.

## Alternatives rejected

- **Treat relationships as `Accessor`:** rejected because the issue contract distinguishes relationship methods from column accessors, and collapsing the distinction would make downstream semantic classification lossy.
- **Add a separate DBIx parser or workspace graph:** rejected because the parser already produces the required `MethodCall` nodes and this slice does not require relationship navigation or MRO changes.
- **Match arbitrary source text:** rejected because parsed-node matching preserves comment/string boundaries and avoids foreign-package false positives.
- **Implement `many_to_many` in the first slice:** rejected because it is a separate declaration shape and the issue's required acceptance is satisfied by `add_columns`, `has_many`, and `belongs_to`.

## Prior art / duplicates

The analyzer and workspace `GeneratedMemberExtractor` paths are the canonical existing synthesis paths for their consumers. `ClassModelBuilder::try_extract_class_accessor_methods` is the closest target-matching and anchor precedent. Parser tests already prove DBIx `has_many`, `belongs_to`, and `add_columns` forms parse cleanly. Related #2978 (relationship navigation) and #2976 (schema migration) remain separate concerns; this PR produces declaration facts and completion inputs, but does not add relationship graph navigation.

## Links

- Issue: #6108
- Research verification: issue comments by `EffortlessSteven` on #6108
- Related issue: #1639 — false-completed predecessor superseded by #6108
- Related issue: #2373 — framework-generated member conformance precedent
- Related issue: #2978 — relationship navigation, explicitly out of scope
- Related issue: #2976 — schema migration, explicitly out of scope
- Source contract: `crates/perl-semantic-facts/src/lib.rs` `GeneratedMember`
