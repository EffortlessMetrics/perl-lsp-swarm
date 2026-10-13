mod cpan_test_helpers;
use cpan_test_helpers::*;

#[test]
fn test_qualified_bareword_as_bare_call_argument_parses() {
    // Upstream t/comp/parser.t: `is CORE::print::foo, 43, '...'`.
    assert_clean_parse("is CORE::print::foo, 43;");
    assert_clean_parse("is Foo::bar::baz, 43;");
    assert_clean_parse("foo File::Spec, 1;");
    assert_clean_parse("is CORE::print, 43;");
}

#[test]
fn test_qualified_forms_that_already_parsed_still_parse() {
    assert_clean_parse("my $x = Foo::bar;");
    assert_clean_parse("print Foo::bar;");
    assert_clean_parse("foo File::Spec->catfile('a');");
}
