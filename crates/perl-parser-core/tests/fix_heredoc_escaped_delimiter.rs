mod cpan_test_helpers;

use cpan_test_helpers::assert_clean_parse;

/// Perl keeps escape sequences literal in quoted heredoc delimiter names.
/// The terminator line must contain the bytes `E`, `\`, `n`, `O`, `F` — not an
/// embedded newline. Verified against Perl 5.38 on 2026-08-11.
#[test]
fn quoted_heredoc_delimiter_escape_stays_literal() {
    assert_clean_parse(
        r#"my $s = <<"E\nOF"
body
E\nOF
;"#,
    );
}
