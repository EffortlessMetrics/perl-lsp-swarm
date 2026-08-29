# Shared declaration VERSION syntax (#10716)

Status: current for this claim
Owner: perl-ast
Slot: C0-03 of the core-class source-truth train (#10687)

## Problem

`package NAME VERSION` and native `class NAME VERSION` both admit an optional
version in the declaration header. Today both parse it and throw it away, on
two separate code paths:

- `crates/perl-parser-core/src/engine/parser/declarations.rs` `parse_package`
  builds a `version` string from a `Number`/`VString` token (or an
  identifier-plus-`.N` v-string) and ends with `let _ = version;` — parsed,
  never attached. The concatenation into the package name was removed by
  #5265 because it polluted PL201 messages and package-to-file mapping.
- the same file's `parse_class` consumes the same token shapes with
  `self.tokens.next()?; // consume and discard version token`, keeping
  nothing at all.

So the raw spelling, the byte range, and whether the reading was exact are all
lost, twice, with two independently written recognizers. Without one shared
lower value the later parser cutovers (#11089) and the package/class
structural rails (#10753, #10762) would each re-derive a VERSION taxonomy.

## Scope ruling

This claim adds **one owner-neutral value type** and nothing else. It changes
no `NodeKind` variant, no parser output, no traversal, no semantic result, and
no provider surface. The two discard sites above are documented here as the
consumers this type exists for; rewiring them is #11089's claim, not this one.

## Authority

- Canonical source geometry is `perl_position_tracking::SourceLocation`
  (a `ByteSpan`, half-open `[start, end)`). This claim adds no second range,
  offset, or geometry authority.
- `perl-ast` owned no recovery/completeness enum before this claim, so
  `DeclarationVersionDisposition` is new here rather than reused. It is scoped
  to declaration VERSION and is derived, never stored.
- Version *meaning* is not in scope and is not in `perl-ast`. Two existing
  workspace types model version values for other propositions and are
  deliberately untouched:
  `perl_pragma::version::PerlVersion` (`use v5.36` pragma comparison) and the
  private `perl_semantic_facts::framework_checked::version::ParsedVersion`
  (framework constraint matching). Neither models declaration header syntax,
  so no duplicate authority is created and none is retired.

## Contracts

- A recorded version keeps its **source form** (`Decimal`, `VString`,
  `RecoveredOrUnknown`), its **exact raw spelling**, and its **exact byte
  range**. Decimal and v-string stay different forms regardless of whether a
  later semantic layer would call them equal.
- The retained spelling is the source slice of the retained range:
  `raw.len() == range.end - range.start` is enforced at construction. A caller
  therefore cannot store a reconstructed or normalized string against a real
  source range.
- **Disposition is derived from the form**, not stored beside it. An "exact
  recovered" value is unrepresentable rather than merely rejected.
- **Absence is owner-level `Option::None`.** A version that was present but
  unreadable is `Some(RecoveredOrUnknown)` and keeps whatever text and
  geometry the parser observed. Unknown is not absent.
- **No normalized value.** There is no numeric accessor, comparison, ordering,
  equivalence, activation, import, or directive semantics on this type, and no
  second "compatibility string" field beside the raw spelling.
- Checked construction rejects an inverted range, a range that does not cover
  the spelling, and a zero-width *exact* form. Rejection is a typed `Result`;
  no constructor panics.
- `Display` is one deterministic form-tagged projection
  (`<form>:<raw>@<start>..<end>`). It is a diagnostic/receipt rendering, not
  machine identity, and never renders a normalized interpretation.

## Deliberate omissions

- **No serde.** `perl-ast` carries no serde dependency today; "deterministic
  serialization" is satisfied by derived `Debug`/`PartialEq` and the
  deterministic `Display` above. Adding a dependency for an unbuilt consumer
  would be scope this claim does not own.
- **No convenience constructors** per form. One checked `new` keeps a single
  construction authority.

## Consumers this unblocks

- #11089 — one typed declaration VERSION parser primitive (produces this type
  once for both header paths).
- #10753 — package declaration VERSION field.
- #10762 — canonical `ClassDeclaration` in shadow.

## Proof

`crates/perl-ast/tests/declaration_version_syntax.rs`, rows DVS-001..DVS-012 in
`acceptance.md`.
