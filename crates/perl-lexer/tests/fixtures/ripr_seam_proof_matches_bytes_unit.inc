use super::*;

#[test]
fn matches_bytes_empty_pattern_returns_false() {
    let lexer = PerlLexer::new("hello");
    assert!(!lexer.matches_bytes(b""), "empty pattern must not match anything");
}

#[test]
fn matches_bytes_matching_pattern_returns_true() {
    let lexer = PerlLexer::new("hello");
    assert!(lexer.matches_bytes(b"hello"), "should match at start of input");
}

#[test]
fn matches_bytes_non_matching_pattern_returns_false() {
    let lexer = PerlLexer::new("hello");
    assert!(!lexer.matches_bytes(b"world"), "wrong pattern must not match");
}

#[test]
fn matches_bytes_prefix_match_returns_true() {
    let lexer = PerlLexer::new("->foo");
    assert!(lexer.matches_bytes(b"->"), "arrow prefix must match");
}
