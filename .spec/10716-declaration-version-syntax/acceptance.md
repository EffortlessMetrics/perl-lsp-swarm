# Acceptance: shared declaration VERSION syntax (#10716)

Executable authority: `crates/perl-ast/src/ast/declaration_version.rs`.
Proof: `crates/perl-ast/tests/declaration_version_syntax.rs`
(`cargo test -p perl-ast --test declaration_version_syntax --locked`).

| Row | Proposition | Proof |
| --- | --- | --- |
| DVS-001 | Decimal, v-string, and recovered readings of the same version never collapse into one value | `decimal_vstring_and_recovered_readings_never_collapse` |
| DVS-002 | The spelling is retained byte for byte; `1.230` and `1.23` are different values | `spelling_is_retained_byte_for_byte` |
| DVS-003 | The spelling is derived from the range, never supplied; moving the range moves the text | `spelling_is_derived_from_the_range_not_supplied` |
| DVS-004 | A range past the end of the source is rejected, recovery included | `range_past_the_end_of_source_is_rejected` |
| DVS-005 | An inverted range is rejected before any bounds or slicing arithmetic | `inverted_range_is_rejected_before_bounds_arithmetic` |
| DVS-006 | An exact form cannot cover zero bytes; only recovery may be zero-width | `exact_forms_require_a_spelling_but_recovery_may_be_zero_width` |
| DVS-007 | Recovery is never exact, and the disposition is derived from the form | `recovered_readings_are_never_exact` |
| DVS-008 | A present-but-unreadable version is not absence | `an_unknown_version_is_not_absence` |
| DVS-009 | Multi-byte source preserves byte geometry; a char-splitting range is rejected and a multi-byte spelling is measured in bytes | `multibyte_source_preserves_byte_geometry_and_rejects_split_characters` |
| DVS-010 | The `Display` projection is deterministic, form-tagged, and never normalized | `display_projection_is_deterministic_and_form_tagged` |
| DVS-011 | One value embeds in a package owner and a class owner with no conversion | `one_value_embeds_in_package_and_class_owners_without_conversion` |
| DVS-012 | Rejection diagnostics name the offending geometry, by full message not digit co-occurrence | `rejection_diagnostics_are_actionable` |
| DVS-013 | A caller cannot substitute a spelling for the one the range covers | `a_caller_cannot_substitute_a_spelling_for_the_covered_source` |
| DVS-014 | An exact form admits exactly what Perl admits; recovery is the only escape | `exact_forms_reject_cross_tag_and_malformed_spellings` |
| DVS-015 | The one-line projection survives control characters in recovered text | `display_projection_escapes_control_characters_in_recovered_text` |

## External oracle — Perl 5.38.2

DVS-014's accept and reject tables are not invented. Every row is the observed
verdict of the interpreter on the declaration header itself:

```
$ perl -v          # This is perl 5, version 38, subversion 2 (v5.38.2)
$ perl -e 'package A <spelling>; 1;'
```

| Spelling | Perl | Reason Perl gives when it rejects |
| --- | --- | --- |
| `0`, `1`, `10`, `0.0`, `1.0`, `1.23`, `0.001`, `5.036`, `10.5` | accept | — |
| `v1.2.3`, `v1.2.3.4`, `v0.0.0`, `v1.02.3`, `v1.22.333`, `v1.2.999`, `v1000.2.3` | accept | — |
| `00`, `01`, `v01.2.3` | reject | no leading zeros |
| `1_2`, `1.23_45`, `v1.2.3_4` | reject | no underscores |
| `v5`, `v1.2`, `v`, `vv1.2.3` | reject | dotted-decimal versions require at least three parts |
| `1.2.3`, `1.2.3.4`, `1.23.45` | reject | dotted-decimal versions must begin with `v` |
| `v1.2.1000`, `v1.1000.3`, `v1.2.3333`, `v1.2.0999` | reject | maximum 3 digits between decimals |
| `1.` | reject | fractional part required |
| `.5` | reject | 0 before decimal required |

A throwaway differential compared this crate's `DeclarationVersionForm::accepts`
against `perl -e 'package A <spelling>; 1;'` over a 44-spelling corpus covering
every row above plus `1.0.0`, `v10.20.30`, `v1.2.3.4.5`, `007`, `1.007`,
`1.123456`, `1.99999999`, `v999.999.999`, and `v1.0.0`:
**44 compared, 44 agree, 0 disagreements.** The differential harness is not
checked in — `perl-ast` is a Tier 1 leaf crate and adding a Perl subprocess to
its test target would introduce a dependency and a CI surface this claim does
not own. The corpus and verdicts are pinned in DVS-014 instead, and this table
is the record of where they came from.

Note the spellings that a reasonable reading of "v-string" gets wrong, all of
which the oracle caught rather than review or intuition:

- `v5` is **not** a legal declaration version — Perl requires at least three
  parts even with the `v`.
- a bare `1.2.3` is **not** one either — Perl requires the leading `v`.
- components *after the first* are capped at three digits, so `v1.2.1000` and
  even `v1.2.0999` are rejected, while `v1000.2.3` is fine: the cap is
  "between decimals", not on the whole spelling.

Successive revisions of this contract got each of these wrong before the
interpreter was consulted.

## Oracle notes

DVS-003 is non-circular by construction: every expected spelling is an
independently written literal, never `&source[start..end]` re-sliced with the
same offsets the test just passed in. Four different ranges over one source
yield four different literals, which is what proves the range is load-bearing
rather than a label carried alongside a string.

DVS-013 exists because an earlier revision took the spelling from the caller
and only checked that its *length* matched the range. That accepted
`raw = "9.99"` against `package Demo 1.23;` at `13..17` — same length, wrong
content — so the advertised source-fidelity invariant was unenforceable, and
DVS-003 in that revision was circular. The constructor now slices the source
itself, and DVS-013 pins the consequence: two different sources at the same
form and range are different values.

DVS-014 exists because an earlier revision derived exactness from the enum tag
alone, so `Decimal` over `v1.2.3` and `VString` over `garbage` were both
recorded as exact readings.

DVS-015's "one line" claim is asserted against a predicate for characters a
consumer would break on, **not** against `str::lines()`. That distinction is
load-bearing: Rust's `char::is_control` covers only the `Cc` category, so
`U+2028` LINE SEPARATOR (`Zl`) and `U+2029` PARAGRAPH SEPARATOR (`Zp`) passed
through literally — and because `str::lines()` does not split on them either,
a `lines().count() == 1` oracle reported success on the broken output. Log
viewers, JSON consumers and JavaScript tooling do treat them as breaks. The
oracle was replaced along with the escape set.

DVS-001 deliberately pairs `1.002003` (decimal) with `v1.2.3` (v-string) —
spellings a later semantic layer may well call equal. Their inequality here is
the claim that this type carries source form, not version meaning.

DVS-011 is a compile-time proposition as much as a runtime one: the same
`Option<DeclarationVersionSyntax>` is moved into two distinct owner structs
without a conversion step, which is what "owner-neutral" has to mean for
#10753 and #10762.

## Mutation controls

Each mutation was applied to the production module, run, and reverted; the
named rows failed and the suite returned to 15/15 green afterwards.

| Mutation | Rows that caught it |
| --- | --- |
| `RecoveredOrUnknown.disposition()` returns `Exact` | DVS-006, DVS-007, DVS-008 |
| source-length bounds check removed | DVS-004 |
| inverted-range check moved after the slicing step | DVS-005 |
| spelling normalized on construction (`trim_end_matches('0')`) | DVS-002 |
| whole source stored instead of the covered slice | DVS-001, DVS-003, DVS-006, DVS-010, DVS-013 |
| hand-written `PartialEq`/`Hash` comparing form and range but **not** the spelling | DVS-013 |
| `accepts()` always true | DVS-014 |
| decimal grammar strips a leading `v` | DVS-014 |
| leading-zero rule dropped (`is_leading_zero_free_digits` → `is_plain_digits`) | DVS-014 |
| v-string minimum component count lowered from three to one | DVS-014 |
| `Display` writes the raw spelling unescaped | DVS-015 |
| escape set narrowed to `char::is_control` only (drops U+2028/U+2029) | DVS-015 |
| v-string three-digit component cap dropped | DVS-014 |

Two of these are worth naming because they initially *survived* and forced a
change rather than confirming one. The `PartialEq`-ignoring-the-spelling mutant
passed all twelve rows of the first revision — it is the counterexample that
motivated DVS-013. The decimal-strips-`v` mutant passed until a `Decimal` over
`v5` fixture was added, because no existing fixture distinguished it.

## Out of scope

No parser population (#11089), no package/class node layout (#10753 / #10762),
no version comparison, ordering, normalization, feature activation, import or
directive semantics, and no provider or support claim. The grammar here decides
spelling *shape* only — whether Perl would accept the header — never what a
version means or how two of them order.
