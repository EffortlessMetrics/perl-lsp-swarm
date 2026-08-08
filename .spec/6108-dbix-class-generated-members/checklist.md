# Implementation Checklist: #6108 — DBIx::Class relationship and column accessor extraction

## Change order (compiles at each step)

### Step 1: Extend generated-member classification

- **File:** `crates/perl-semantic-facts/src/lib.rs`
- **Change:** Add `GeneratedMemberKind::Method` for framework-generated relationship methods.
- **Details:** Preserve existing serialized variants and update the semantic-facts property/round-trip generators that enumerate every kind.
- **Verify:** `cargo check --locked -p perl-semantic-facts`

### Step 2: Recognize DBIx::Class and capture declaration facts

- **File:** `crates/perl-semantic-analyzer/src/analysis/class_model.rs`
- **Change:** Add `Framework::DbixClass`; recognize `use DBIx::Class` and `use DBIx::Class::Core`; capture current-package `add_columns`, `has_many`, and `belongs_to` calls as anchored synthetic method metadata.
- **Details:** Accept the parser's existing `MethodCall` forms, support static string/identifier/qw-style names, deduplicate names per declaration, and reject calls on foreign package targets. Use `GeneratedMemberKind::Accessor` for columns and `GeneratedMemberKind::Method` for relationships.
- **Depends on:** Step 1
- **Verify:** `cargo check --locked -p perl-semantic-analyzer`

### Step 3: Emit DBIx generated members through both producer paths

- **Files:** `crates/perl-semantic-analyzer/src/analysis/generated_member_extractor.rs`, `crates/perl-workspace/src/semantic/generated_member_extractor.rs`
- **Change:** Map the captured metadata to analyzer `GeneratedMember` facts and workspace `EntityFact` entries with the existing deterministic anchor/entity/provenance paths.
- **Details:** Keep Moo/Moose/Mouse/Class::Tiny/Class::Accessor behavior unchanged; preserve declaration anchors for downstream goto-definition resolution.
- **Depends on:** Steps 1–2
- **Verify:** `cargo check --locked -p perl-semantic-analyzer`

### Step 4: Add focused positive and negative proof

- **Files:** analyzer tests, `crates/perl-workspace/src/semantic/generated_member_extractor.rs`, and `crates/perl-workspace/tests/generated_member_facts.rs`.
- **Change:** Cover `add_columns`, `has_many`, `belongs_to`, `use DBIx::Class(:Core)`, foreign-package negative control, duplicate names, and declaration anchors.
- **Details:** Retain existing framework tests and prove generated facts are visible with the expected package, kind, provenance, confidence, source anchor, workspace method candidates, and semantic definition lookup.
- **Depends on:** Steps 1–3
- **Verify:** `cargo test --locked -p perl-semantic-analyzer --lib generated_member -- --test-threads=2`

### Step 5: Final verification

- **Verify:** `cargo fmt --all -- --check`, `git diff --check`, `cargo test --locked -p perl-semantic-facts`, `cargo test --locked -p perl-semantic-analyzer --lib`, and `cargo clippy --locked -p perl-semantic-analyzer --lib -- -D warnings`.

## Callers and consumers

- `ClassModelBuilder::build` is consumed by `GeneratedMemberExtractor::extract` and `extract_from_models`.
- `GeneratedMemberExtractor` feeds `SemanticAnalyzer::generated_members`, which is consumed by workspace/LSP semantic symbol surfaces.
- `GeneratedMemberKind` is serialized by `perl-semantic-facts`; its property and round-trip tests enumerate the variant set.

## Scope boundary

Files IN scope: `crates/perl-semantic-facts/src/lib.rs`, the directly related semantic-facts tests, `crates/perl-semantic-analyzer/src/analysis/class_model.rs`, `crates/perl-semantic-analyzer/src/analysis/generated_member_extractor.rs`, `crates/perl-workspace/src/semantic/generated_member_extractor.rs`, `crates/perl-workspace/tests/generated_member_facts.rs`, and focused tests/spec files.

Files OUT of scope: parser grammar changes, DBIx relationship navigation, schema migration support, `many_to_many`, arbitrary foreign-package calls, new workspace graph machinery, release/CI changes, and changes to Moo/Moose semantics.

## Flags for builder

- The issue's requested `Method` classification is not present in the current facts enum; adding it is an intentional public semantic-facts shape change and must retain serialization compatibility for existing variants.
- The initial slice supports static declaration names only. Dynamic names are ignored rather than guessed.
- Relationship declarations are anchored to the `has_many`/`belongs_to` call span so existing declaration plumbing can resolve the source line; end-to-end goto/completion proof should be added only where current consumers expose a stable test API.
