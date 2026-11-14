//! Regression tests for issue #2712 — CPAN `unclosed_brace_eof` bucket on Test2::Compare::*.
//!
//! Historical failures (`.ci/cpan-corpus-baseline.json`, 2026-04-09) reported
//! `expected '}', found end of input` on all twelve `Test2/Compare/*` modules.
//! The shared shape is a conditional fat-comma pair in a method argument list,
//! e.g. `Test2/Compare/Array.pm` around the `push @deltas => ...->new(...)` call.

use super::cpan_test_helpers::*;

#[test]
fn test2_compare_ending_ternary_fat_comma_pair_in_arg_list() {
    assert_clean_parse(
        r#"
        push @deltas => $self->delta_class->new(
            dne      => 'check',
            verified => undef,
            $self->{+ENDING} eq 'implicit' ? (note => 'implicit end') : (),
        );
        "#,
    );
}

#[test]
fn test2_compare_unary_plus_hash_subscript_in_same_arg_list() {
    assert_clean_parse(
        r#"
        if ($self->{+ENDING}) {
            push @deltas => $self->delta_class->new(
                id  => [ARRAY => $state],
                got => $item,
                $self->{+ENDING} eq 'implicit' ? (note => 'implicit end') : (),
            );
        }
        "#,
    );
}
