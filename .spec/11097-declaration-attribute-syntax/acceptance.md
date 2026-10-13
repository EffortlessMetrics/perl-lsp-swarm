# Acceptance: #11097 — shared declaration-attribute source syntax

## §Behavior

- One owner-neutral value preserves separator, name, argument, ranges, order,
  duplicates, spelling, and recovery.
- `None` distinguishes absent arguments from present empty, exact, recovered,
  and unavailable arguments.
- Construction rejects contradictory exact/recovered states and invalid range
  or delimiter geometry.

## §Contracts

- `DeclarationAttributeSyntax` is the shared lower syntax identity.
- `DeclarationAttributeArgumentSyntax` preserves argument geometry and local
  disposition.
- No parser, node layout, consumer, semantic, provider, or support contract
  changes in this PR.

## §API-Shape

The public API is `DeclarationAttributeSyntax`,
`DeclarationAttributeArgumentSyntax`, `DeclarationAttributeSeparator`,
`DeclarationAttributeDelimiter`, `DeclarationAttributeArgumentDisposition`,
`DeclarationAttributeCompleteness`, and the checked construction error.

## §Test-Grid

| Case | Required result |
| --- | --- |
| repeated `reader` entries | both remain in source order |
| custom spelling | retained without semantic normalization |
| colon separator | separator identity and range retained |
| whitespace continuation | distinct separator identity retained |
| no argument | `None` |
| empty/exact argument | distinct dispositions with delimiters |
| recovered/unavailable argument | explicit recovery disposition |
| exact attribute containing recovery | rejected |
| exact argument without delimiters | rejected |
| invalid range or delimiter order | rejected |

## §Blast-Radius

Only `crates/perl-ast/src/declaration.rs`, its `lib.rs` module/re-exports, and
this three-file specification packet change. There is no parser or runtime
behavior change. Reversion is a single bounded rollback.

## Claim boundary

This PR establishes a reusable, source-preserving value contract for later
class and field parser work. It does not establish parser production,
canonical AST node emission, semantic interpretation, or provider behavior.

## Non-goals

No `NodeKind`, source scanning, expression evaluation, framework policy,
generated members, compatibility retirement, or consumer migration.
