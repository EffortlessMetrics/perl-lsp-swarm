# Context: #11097 — shared declaration-attribute source syntax

## Problem

Current AST declarations retain attributes as flattened `Vec<String>` values.
That representation loses separator identity, argument geometry, spelling,
duplicates, source order, and recovery state. Class and field consumers must
share one lower syntax proposition; their later semantic interpretations remain
separate.

## Authority and boundary

The semantic owner is `crates/perl-ast`. This issue adds a value contract only.
It does not add a `NodeKind`, change parser production, migrate flattened
fields, or interpret names and arguments. #10725 and #10730 are consumers; the
typed parser primitive belongs to #11093. Fixture source bodies remain owned by
#10698 and are referenced by identifier only.

## Canonical mapping

| Source fact | Contract field |
| --- | --- |
| `:` or reviewed whitespace continuation | `DeclarationAttributeSeparator` and its range |
| attribute spelling | `name` and `name_range` |
| full attribute geometry | `range` |
| no argument | `argument = None` |
| argument body and delimiters | `DeclarationAttributeArgumentSyntax` |
| exact, empty, recovered, unavailable | argument disposition |
| exact versus recovery-derived attribute | `DeclarationAttributeCompleteness` |

Order and duplicates are properties of the containing `Vec`; no set or
semantic registry is introduced.

## Non-authorities

This contract does not resolve packages, assign class/field meaning, evaluate
parenthesized text, infer generated members, validate framework profiles, or
change provider behavior. It records syntax at the strength actually retained.

## Proof boundary

Unit tests prove construction invariants, exact/recovered separation, absent
versus present argument states, delimiter geometry, order, duplicate retention,
and non-interpretive spelling. Parser production and live AST behavior are
deliberately not exercised because they belong to later issues.

## Rollback and handoff

Revert the module and its re-export plus this packet. The next consumer may
compose the value directly; it must not create class- or field-specific copies.
