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

## Oracle notes

DVS-003 is non-circular by construction: every expected spelling is an
independently written literal, never `&source[start..end]` re-sliced with the
same offsets the test just passed in. Four different ranges over one source
yield four different literals, which is what proves the range is load-bearing
rather than a label carried alongside a string.

DVS-013 exists because an earlier revision of this contract took the spelling
from the caller and only checked that its *length* matched the range. That
accepted `raw = "9.99"` against `package Demo 1.23;` at `13..17` — same
length, wrong content — so the advertised source-fidelity invariant was
unenforceable, and DVS-003 in that revision was circular. The constructor now
slices the source itself, and DVS-013 pins the consequence: two different
sources at the same form and range are different values, so neither the range
nor a caller string can stand in for the text.

DVS-001 deliberately pairs `1.002003` (decimal) with `v1.2.3` (v-string) —
spellings a later semantic layer may well call equal. Their inequality here is
the claim that this type carries source form, not version meaning.

DVS-011 is a compile-time proposition as much as a runtime one: the same
`Option<DeclarationVersionSyntax>` is moved into two distinct owner structs
without a conversion step, which is what "owner-neutral" has to mean for
#10753 and #10762.

## Mutation controls

Each mutation was applied to the production module, run, and reverted; the
named rows failed and the suite returned to 13/13 green afterwards.

| Mutation | Rows that caught it |
| --- | --- |
| `RecoveredOrUnknown.disposition()` returns `Exact` | DVS-006, DVS-007, DVS-008 |
| source-length bounds check removed | DVS-004 |
| inverted-range check moved after the slicing step | DVS-005 |
| spelling normalized on construction (`trim_end_matches('0')`) | DVS-002 |
| whole source stored instead of the covered slice | DVS-001, DVS-003, DVS-006, DVS-010, DVS-013 |
| hand-written `PartialEq`/`Hash` comparing form and range but **not** the spelling | DVS-013 |

The last row is the counterexample raised in review against the earlier
revision: under it, two versions with different spellings at the same range
compared equal, and every test then in the suite still passed. It is now
caught.

## Out of scope

No parser population (#11089), no package/class node layout (#10753 / #10762),
no version comparison, ordering, normalization, feature activation, import or
directive semantics, and no provider or support claim.
