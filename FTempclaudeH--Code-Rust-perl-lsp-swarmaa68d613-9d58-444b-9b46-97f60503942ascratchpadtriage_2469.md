## Current state (2026-07-11)

**File**: `crates/perl-parser-core/src/engine/parser/expressions/postfix.rs:953-955`

Code matches issue description exactly:
```rust
txt.starts_with(|c: char| {
    c.is_ascii_lowercase() || c == '_'
})
```

Accepts identifiers starting with lowercase letters OR underscore. Comment says "lowercase identifier" (line 945), but code logic accepts both.

## Claim verification

✓ **Code allows underscore-starting identifiers** — confirmed; e.g., `_Foo` would be accepted.

✓ **Underscore-starting identifiers are valid in Perl** — confirmed via [perldoc: Barewords](https://perldoc.perl.org/perldata#Barewords): "Barewords can start with any letter or underscore"; sort comparators can be any valid function name.

✓ **Comment says 'lowercase identifier'** — confirmed at line 945; describes intent as matching "Perl's convention" for comparator functions.

⚠️ **CRITICAL: Existing test depends on underscore support** — File `crates/perl-parser-core/tests/unclosed_paren_identifier_tests.rs:441-443`:
```rust
#[test]
fn sort_custom_comparator_in_parens() {
    // (sort _released_order @perls)[0]
    assert_clean_parse(r#"my $first = (sort _released_order @perls)[0];"#);
}
```

This test uses `_released_order` (underscore-starting) as a comparator and expects clean parse. Implementing the previous scout's recommendation to "reject non-lowercase prefixes" would **break this test**.

## Scope & plan

**Two viable approaches:**

1. **Fix the comment** (safer): Update lines 945-950 to document why we accept both lowercase AND underscore-starting identifiers. Current Perl semantics support both; test confirms necessity.

2. **Tighten code + test** (restrictive): Change check to `c.is_ascii_lowercase()` only, update `unclosed_paren_identifier_tests.rs:441-443` test, and verify no CPAN corpus regresses. This enforces lowercase-only convention.

**Non-goals:** Do NOT implement previous scout recommendation ("reject non-lowercase") without addressing the test breakage.

## Next state

**needs-decision** — builder must choose:
- Approach 1: Update comment (low risk, clarifies current behavior)
- Approach 2: Tighten code (high risk if CPAN code relies on underscore-starting comparators; requires test update + verification)

Sources: [Perldoc: Barewords](https://perldoc.perl.org/perldata#Barewords) · Test: `crates/perl-parser-core/tests/unclosed_paren_identifier_tests.rs:441-443`
