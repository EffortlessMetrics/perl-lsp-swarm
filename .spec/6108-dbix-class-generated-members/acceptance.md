# Acceptance Criteria: #6108 — DBIx::Class relationship and column accessor extraction

## §Behavior

| Input / Condition | Expected Result | Notes |
|---|---|---|
| `use DBIx::Class(:Core)` followed by `__PACKAGE__->add_columns(qw/id name/)` | `id` and `name` are generated members of kind `Accessor` | Each member is anchored to the declaration call. |
| `__PACKAGE__->has_many('posts', 'Post', 'author_id')` | `posts` is a generated member of kind `Method` | Relationship methods use framework-synthesis provenance and medium confidence. |
| `__PACKAGE__->belongs_to('author', 'Author')` | `author` is a generated member of kind `Method` | Same anchor/provenance contract as `has_many`. |
| A foreign package invokes `Other::Package->has_many(...)` | No DBIx member is emitted for the current package | Target matching is deny-by-default. |
| A name is dynamic or malformed | No guessed member is emitted; extraction remains fallible/empty | No panic and no fabricated symbol. |
| Existing Moo/Moose/Mouse/Class::Tiny/Class::Accessor input | Existing generated-member kinds and counts remain unchanged | Regression control. |

All tests pass: `cargo test --locked -p perl-semantic-facts` and `cargo test --locked -p perl-semantic-analyzer --lib`.
No clippy warnings: `cargo clippy --locked -p perl-semantic-analyzer --lib -- -D warnings`.
Formatted: `cargo fmt --all -- --check`.

## §Hazards

| Class | Invariant | Surface (specific file/fn this change touches) | Required adversarial test |
|---|---|---|---|
| ID/ref-space collision | Generated entity IDs remain deterministic and include package, member name, and source anchor; adding a kind does not create duplicate identity space. | `generated_member_extractor.rs::make_member` | Same name in two declarations and two packages yields distinct anchored IDs. |
| Bounds/overflow | Source offsets used as `AnchorId` remain converted through the existing bounded type path; no new arithmetic is introduced. | `class_model.rs::try_extract_dbix_class_members` | Empty/large declaration inputs produce members or no members without panic. |
| Protocol-safety | N/A — this slice does not modify an LSP request handler or wire protocol. | N/A | N/A — analyzer-only. |
| Scanner literal/comment blindness | N/A — extraction uses parsed `NodeKind` values, not a byte scanner. | `class_model.rs::try_extract_dbix_class_members` | A comment/string containing `has_many` without a method-call node emits nothing. |
| Test-encodes-the-bug | Positive tests must assert the generated kind, package, provenance, confidence, and source anchor, not only member names. | Analyzer focused tests | Foreign-target and non-DBIx negative controls remain empty. |
| Coverage/measurement integrity | New public enum variant is included in semantic-facts property and JSON round-trip generators. | `perl-semantic-facts` tests | Round-trip `GeneratedMemberKind::Method` and enumerate it in generated-member properties. |

**Subsystem-specific defaults consulted:** `docs/reference/SUBSYSTEM_HAZARD_DEFAULTS.md`; this is semantic-analyzer/facts work, not a parser grammar, LSP handler, DAP, or CI transform change.

## §Contracts

| Contract | Source document + section | How this change satisfies or extends it |
|---|---|---|
| Generated-member provenance and anchor contract | `crates/perl-semantic-facts/src/lib.rs` `GeneratedMember` | New DBIx facts use the existing `FrameworkSynthesis`/`Medium` contract and declaration anchor path. |
| Parser AST consumption | `docs/reference/PARSER_CONTRACTS.md` | No AST variant is added; the implementation consumes existing `MethodCall`, `FunctionCall`, `String`, `Identifier`, and array/qw nodes. |

## §API-Shape

| Item | Kind | Signature / Range | Dup-risk (grep result) | Caller count |
|---|---|---|---|---|
| `GeneratedMemberKind::Method` | public enum variant | Existing `GeneratedMemberKind` serialized enum | No existing `Method` variant found | Existing enum consumers are non-exhaustive or explicit variant tests |
| `Framework::DbixClass` | analyzer enum variant | Existing internal framework enum | No existing DBIx framework variant found | `ClassModelBuilder` and extractor |
| Synthetic generated-kind metadata | internal field/constructor | Existing `MethodInfo` metadata path | No duplicate capture path found | `class_model.rs` → `generated_member_extractor.rs` |

## §Test-Grid

| Scenario | Kind | Test name | Invariant discharged |
|---|---|---|---|
| DBIx framework detection | positive | `detects_dbix_class_framework` | Framework recognition |
| Column list | positive | `dbix_add_columns_generate_accessors` | Accessor generation and static names |
| Relationship | positive | `dbix_relationships_generate_methods` | Method classification and anchors |
| `:Core` import args | positive | `dbix_core_import_generates_members` | Import form handling |
| Foreign target | negative | `dbix_foreign_package_call_is_ignored` | Deny-by-default target matching |
| Dynamic/malformed name | negative | `dbix_dynamic_member_name_is_ignored` | No fabricated facts/no panic |
| Duplicate declaration names | adversarial | `dbix_duplicate_names_keep_one_fact_per_declaration` | Determinism and identity |
| Existing framework regression | regression | `non_dbix_frameworks_keep_existing_generated_members` | No cross-framework behavior change |
| Serialization | property | `generated_member_kind_method_round_trips` | Facts schema integrity |

## §Blast-Radius

| Consumer | Crate | Dependency type | Impact | Required update |
|---|---|---|---|---|
| `SemanticAnalyzer::generated_members` | `perl-semantic-analyzer` | direct model/extractor output | New DBIx facts become visible to semantic consumers | Focused analyzer proof |
| Generated-member JSON/property tests | `perl-semantic-facts` | public enum serialization | New variant must round-trip | Update enumerators |
| Workspace/LSP completion and goto consumers | downstream analyzer/LSP crates | transitive facts | Should consume existing `GeneratedMember` anchors without new handler changes | Verify current tests; no consumer redesign in this PR |

Must-not-touch boundary: parser grammar, workspace graph/MRO, relationship navigation, schema migration, CI, release files, and unrelated framework extractors.
