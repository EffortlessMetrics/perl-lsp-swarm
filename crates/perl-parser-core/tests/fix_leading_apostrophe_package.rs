mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn test_leading_apostrophe_package_variable_parses() {
    // `$'b` is `$main::b` (legacy separator, empty first segment), as in
    // upstream t/comp/package.t: `$main'a = $'b;`.
    assert_clean_parse("$x = $'b;");
    assert_clean_parse("$'b = 1;");
    assert_clean_parse("$main'a = $'b;");
}

#[test]
fn test_bare_postmatch_variable_still_parses() {
    assert_clean_parse("print $';");
}
