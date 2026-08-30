# Shared declaration VERSION syntax (#10716)

Status: current for this claim
Owner: perl-ast
Slot: C0-03 of the core-class source-truth train (#10687)

## Problem

`package NAME VERSION` and native `class NAME VERSION` both admit an optional
version in the declaration header. Today both parse it and throw it away, on
two separate code paths:

- `crates/perl-parser-core/src/engine/parser/declarations.rs` `parse_package`
  builds a `version` string from a `Number`/`VString` token and ends with
  `let _ = version;` — parsed, never attached. The concatenation into the
  package name was removed by #5265 because it polluted PL201 messages and
  package-to-file mapping.
- the same file's `parse_class` consumes the same token shapes with
  `self.tokens.next()?; // consume and discard version token`, keeping
  nothing at all.

So the raw spelling, the byte range, and whether the reading was exact are all
lost, twice, with two independently written recognizers. Without one shared
lower value the later parser cutovers (#11089) and the package/class
structural rails (#10753, #10762) would each re-derive a VERSION taxonomy.

### One correction about those two paths

Both recognizers also carry an identifier-plus-trailing-`.N` fallback branch
for v-strings. **That branch is unreachable for `package` and `class` today.**
`crates/perl-lexer/src/lib.rs:465` gates `try_vstring()` only on
`!self.after_sub`, and `after_sub` is set true exclusively by the `sub` and
`method` keywords (`lib.rs:1981`) — never after `class` or `package`. So both
declaration forms always receive one contiguous `VString` token whose text is
the exact source slice. The comment at `lib.rs:3892` records the companion
fact: under `after_sub` the `.` is lexed as a separate `Operator`, so the
fallback's `num_token.text.starts_with('.')` guard cannot hold even where the
branch is reachable.

This matters for #11089, not for this claim: a producer that lifts the
existing fallback logic verbatim would reconstruct `"v5" + "2" + "3"` and try
to record it against a 6-byte `v1.2.3` span. Under this contract that cannot
silently succeed — the spelling is derived from the source, so a reconstructed
string has nowhere to go.

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
- **Source fidelity is structural, not promised.**
  `DeclarationVersionSyntax::from_source(form, source, range)` is the only
  constructor and takes no caller-supplied spelling: the text is sliced out of
  the source. A caller therefore cannot pair a reconstructed, normalized, or
  simply wrong spelling with a real source range, because no API accepts one.
  Slicing a range the caller already computed is not a source rescan — the
  producer does no searching, matching, or re-lexing.
- **Disposition is derived from the form**, not stored beside it. An "exact
  recovered" value is unrepresentable rather than merely rejected.
- **An exact form admits exactly what Perl admits.** The constructor validates
  a closed spelling grammar per form, calibrated against Perl 5.38.2 rather
  than against intuition:
  - *decimal* — one integer part with no leading zero (`0`, `1`, `10`, but not
    `00`/`01`), then at most one fractional part which must have a digit if the
    dot is present. No underscores. `1.`, `.5`, `1_2`, `1.23_45` and `1.2.3`
    are all rejected by Perl and are rejected here.
  - *v-string* — a leading `v` and at least **three** dot-separated
    components. Perl rejects both `v5` (too few parts) and a bare `1.2.3` (no
    `v`), so both are rejected here. The no-leading-zero rule applies to the
    first component only: `v01.2.3` is rejected, `v1.02.3` is accepted.

  `RecoveredOrUnknown` is the only escape and admits anything. Without this
  check the exact/recovered distinction would be a caller's assertion rather
  than a property of the value. See `acceptance.md` for the oracle table and
  the 36-spelling differential against the interpreter.
- **Absence is owner-level `Option::None`.** A version that was present but
  unreadable is `Some(RecoveredOrUnknown)` and keeps whatever text and
  geometry the parser observed. Unknown is not absent.
- **No normalized value.** There is no numeric accessor, comparison, ordering,
  equivalence, activation, import, or directive semantics on this type, and no
  second "compatibility string" field beside the raw spelling.
- Checked construction rejects an inverted range, a range past the end of the
  source, a range that splits a multi-byte character, and a zero-width *exact*
  form. Rejection is a typed `Result`; no constructor panics or indexes.
- `Display` is one deterministic form-tagged projection
  (`<form>:<raw>@<start>..<end>`). It is a diagnostic/receipt rendering, not
  machine identity, and never renders a normalized interpretation. Because a
  recovered reading may cover arbitrary source, control characters and the
  escape character itself are escaped so the projection is genuinely one line;
  ordinary version spellings contain none of them and render unchanged.
- The three public enums are `#[non_exhaustive]`, matching this crate's
  convention for public enums (`GotoTargetForm` at `ast.rs:155`,
  `AstInvariantCode` at `invariants.rs:13`, and 12 further occurrences).
  Adding it now is free; after #10753/#10762/#11089 start matching it would be
  a breaking change.

## Deliberate omissions

- **No serde.** `perl-ast` carries no serde dependency today; "deterministic
  serialization" is satisfied by derived `Debug`/`PartialEq`/`Hash` and the
  deterministic `Display` above. Adding a dependency for an unbuilt consumer
  would be scope this claim does not own.
- **One constructor.** No per-form convenience wrappers; a single construction
  authority is also what makes the fidelity guarantee total.

## Consumers this unblocks

- #11089 — one typed declaration VERSION parser primitive (produces this type
  once for both header paths).
- #10753 — package declaration VERSION field.
- #10762 — canonical `ClassDeclaration` in shadow.

## Proof

`crates/perl-ast/tests/declaration_version_syntax.rs`, rows DVS-001..DVS-013 in
`acceptance.md`.
