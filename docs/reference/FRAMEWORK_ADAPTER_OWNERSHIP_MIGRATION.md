# Framework adapter ownership migration

The concrete framework implementation moves from `perl-semantic-facts` to the existing
`perl-semantic-analyzer` crate under #19461 and the ownership ruling in #14991.
The dependency remains `perl-semantic-analyzer` → `perl-semantic-facts`; no adapter crate
or reverse dependency is introduced.

## Public API change

This is an intentional breaking change to the explicitly published facts crate, governed
by #8097 and [the stability policy](STABILITY.md). It requires a minor release, with this
migration guidance and the Unreleased changelog entry; it must not ship in a patch release.
Passing workspace tests establishes internal migration, not downstream source compatibility.

For every item below, replace the old prefix
`perl_semantic_facts::framework_adapters::<module>::` with
`perl_semantic_analyzer::framework_adapters::<module>::`. Item names and argument/result
contracts are retained. This map includes all 41 moved public free functions and the one
moved public implementation view.

| Module | Moved public items |
| --- | --- |
| `dancer2` | `dancer2_descriptor`, `detect_dancer2`, `parse_dancer2_import_args`, `dancer2_application_identity`, `dancer2_activation_facts` |
| `dancer2_two_x` | `dancer2_two_x_descriptor`, `detect_dancer2_two_x`, `Dancer2TwoXKeywordView`, `dancer2_two_x_route_family_facts`, `parse_dancer2_two_x_import_args`, `parse_parenthesized_dancer2_two_x_import_args`, `qw_words`, `dancer2_two_x_application_identity`, `dancer2_two_x_activation_facts` |
| `dancer2_hooks` | `dancer2_hook_establishes_request_context`, `coerce_dancer2_hook_name`, `normalize_dancer2_hook_name`, `dancer2_hook_facts`, `dancer2_hook_handler_context_facts` |
| `dancer2_routes` | `normalize_dancer2_method`, `dancer2_keyword_methods`, `dancer2_route_facts`, `dancer2_route_family_facts` |
| `moose` | `moose_class_descriptor`, `moose_role_descriptor`, `moose_descriptors`, `detect_moose_class`, `detect_moose_role` |
| `mojo_base` | `mojo_base_descriptor`, `detect_mojo_base`, `parse_mojo_base_import_args`, `mojo_base_activation_facts` |
| `mojo_base_facts` | `mojo_base_object_facts` |
| `mojolicious` | `mojolicious_lite_descriptor`, `detect_mojolicious_lite`, `parse_mojolicious_lite_import_args`, `mojolicious_lite_import_evidence`, `mojolicious_lite_activation_facts`, `mojolicious_role_facts_from_mojo_base` |
| `dbix_class` | `dbix_class_descriptor`, `detect_dbix_class`, `dbix_result_profile_facts` |

`Dancer2TwoXKeywordView::new` and its `RouteFamilyKeywordView` implementation move with
the view type. The shared `RouteFamilyKeywordView` trait remains in the facts crate.
Keeping the implementation view with its trait implementation satisfies Rust ownership
rules without moving the shared contract or creating a dependency cycle.

Callers needing both behavior and records import each from its owner:

```rust
use perl_semantic_analyzer::framework_adapters::dancer2::{
    dancer2_activation_facts, detect_dancer2,
};
use perl_semantic_facts::framework_adapters::dancer2::{
    Dancer2ActivationFacts, Dancer2ImportEvidence,
};
```

The facts crate cannot re-export the moved implementation: that would introduce a
facts-to-analyzer dependency cycle. The new analyzer module is intentionally at its crate
root, outside `analysis`; the parser facade’s existing analysis wildcard therefore does
not acquire this implementation namespace.

## Contracts that retain their owner

All existing framework record and enum type paths remain in the facts crate, except for
the concrete borrowed `Dancer2TwoXKeywordView` described above. Existing non-exhaustive
protection remains intact. Added record constructors assemble caller-supplied fields;
they do not choose activation outcomes, suppress facts, or parse source.

The retained contracts include:

- Adapter IDs, profile and schema identities, declaration carriers, activation outcomes,
  evidence records, provenance, and the checked framework SDK.
- Dancer2 1.x and 2.x DSL registries, hook-name tables, route keyword/method tables, and
  the `dancer2_two_x_keyword` reference lookup.
- The complete QuickORM reference registry and its pure table lookups. The xtask
  `quickorm-api-matrix` consumer continues to use the facts crate directly.
- The four Mojo::Base member, reader, setter, and parent identity functions, with their
  existing domain separators and generation hashing.
- DBIx result-class and result-source identity construction. The class key escapes
  scope and package components; the source key escapes the whole already-escaped
  class key and the table name. Producer admission and trimming stay in the analyzer.

No serialized field, enum tag, generic SDK version, stable hash domain, or identity-key
format changes as part of this move. The existing records that do not implement serde
do not acquire a new serialization promise.

## Tests and consumers

Concrete adapter unit tests and the mixed Dancer2 semantic-profile target move with the
implementation. The latter executes the import producer for the odd-argument refusal
instead of treating the presence of a constant in a source file as proof of execution.
The pure Dancer2 registry oracle, generic SDK authority/serialization tests, QuickORM
tests, and Mojo identity tests remain in the facts crate. DBIx key tests pin component
escaping and the nested class-key escaping at the lower owner.

Analyzer AST producers and core Dancer2 projections call the new implementation paths.
Consumers that only carry fact types retain their facts imports. No parser, AST, symbol,
or workspace production consumer needs an analyzer dependency to transport these records.

## Scope

This cut removes interpretation code from the facts package and makes its ownership
boundary enforceable. It adds no independent executable or install surface. Aggregate
download, dependency closure, binary size, public API qualification, and package savings
need their own measured evidence; source movement alone does not establish them.

Existing shadow dispositions remain shadow dispositions. This move does not promote
framework output to canonical ProjectModel publication authority. Later framework work,
including the Dancer2 template adapter from #18862, must preserve the same contract and
implementation boundary when integrated.
