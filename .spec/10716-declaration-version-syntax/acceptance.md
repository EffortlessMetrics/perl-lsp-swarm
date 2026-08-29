# Acceptance: shared declaration VERSION syntax (#10716)

Executable authority: `crates/perl-ast/src/ast/declaration_version.rs`.
Proof: `crates/perl-ast/tests/declaration_version_syntax.rs`
(`cargo test -p perl-ast --test declaration_version_syntax --locked`).

| Row | Proposition | Proof |
| --- | --- | --- |
| DVS-001 | Decimal, v-string, and recovered readings of the same version never collapse into one value | `decimal_vstring_and_recovered_readings_never_collapse` |
| DVS-002 | The raw spelling is retained byte for byte; `1.230` and `1.23` are different values | `spelling_is_retained_byte_for_byte` |
| DVS-003 | The exact byte range is retained and equals the source slice of the spelling | `range_is_exact_and_agrees_with_source_text` |
| DVS-004 | A range that does not cover the spelling is rejected, recovery included | `range_that_does_not_cover_the_spelling_is_rejected` |
| DVS-005 | An inverted range is rejected before any length arithmetic | `inverted_range_is_rejected_without_arithmetic_underflow` |
| DVS-006 | An exact form requires a spelling; only recovery may be zero-width | `exact_forms_require_a_spelling_but_recovery_may_be_zero_width` |
| DVS-007 | Recovery is never exact, and the disposition is derived from the form | `recovered_readings_are_never_exact` |
| DVS-008 | A present-but-unreadable version is not absence | `an_unknown_version_is_not_absence` |
| DVS-009 | Multi-byte source before the version preserves exact byte geometry | `multibyte_source_preserves_exact_byte_geometry` |
| DVS-010 | The `Display` projection is deterministic, form-tagged, and never normalized | `display_projection_is_deterministic_and_form_tagged` |
| DVS-011 | One value embeds in a package owner and a class owner with no conversion | `one_value_embeds_in_package_and_class_owners_without_conversion` |
| DVS-012 | Rejection diagnostics name the offending geometry | `rejection_diagnostics_are_actionable` |

## Oracle notes

DVS-003 is the source-fidelity oracle: for each fixture it asserts
`value.raw() == &source[value.range()]` against real header text. That is what
proves the spelling and the range were both retained rather than one being
reconstructed from the other.

DVS-001 deliberately pairs `1.002003` (decimal) with `v1.2.3` (v-string) —
spellings a later semantic layer may well call equal. Their inequality here is
the claim that this type carries source form, not version meaning.

DVS-011 is a compile-time proposition as much as a runtime one: the same
`Option<DeclarationVersionSyntax>` is moved into two distinct owner structs
without a conversion step, which is what "owner-neutral" has to mean for
#10753 and #10762.

## Mutation controls

Each mutation was applied to the production module, run, and reverted; the
named rows failed and the suite returned to 12/12 green afterwards.

| Mutation | Rows that caught it |
| --- | --- |
| `RecoveredOrUnknown.disposition()` returns `Exact` | DVS-006, DVS-007, DVS-008 |
| `raw.len() != range_len` check removed | DVS-004 |
| inverted-range check moved after the length arithmetic (wrapping subtraction) | DVS-005 |
| spelling normalized on construction (`trim_end_matches('0')`) | DVS-002 |
| hand-written `PartialEq` that compares raw and range but not form | DVS-001 |

## Out of scope

No parser population (#11089), no package/class node layout (#10753 / #10762),
no version comparison, ordering, normalization, feature activation, import or
directive semantics, and no provider or support claim.
