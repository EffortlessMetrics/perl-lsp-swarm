mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;

#[test]
fn control_flow_conditions_support_declarations_and_empty_while() {
    assert_clean_parse("if (our $x = 1 && $y) { 1; }");
    assert_clean_parse("if ($x) { 1; } elsif (my $y = 2 && $z) { 2; } else { 3; }");
    assert_clean_parse("unless (my $x = 1 && $y) { 1; } elsif (our $z = 2 && $w) { 2; }");
    assert_clean_parse("while (state $x = 1 && $y) { last; }");
    assert_clean_parse("until (local $x = 1 && $y) { last; }");
    assert_clean_parse("while () { last; }");
}
