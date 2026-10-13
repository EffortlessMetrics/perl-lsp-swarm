mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn test_bareword_call_with_coderef_arg_parses() {
    // Upstream t/comp/proto.t: `testing \&no_proto, undef;`
    assert_clean_parse("testing \\&no_proto, undef;");
    assert_clean_parse("foo \\&x;");
    assert_clean_parse("foo \\@a;");
    assert_clean_parse("foo \\\\$x;");
}

#[test]
fn test_bareword_call_other_args_still_parse() {
    assert_clean_parse("foo $x, undef;");
    assert_clean_parse("foo &x;");
    assert_clean_parse("foo %h;");
}
