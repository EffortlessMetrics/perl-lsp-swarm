//! Consolidated error-recovery (unclosed/unexpected) historical `fix_*.rs` tests.
//!
//! Advances #5012 item 7 (thematic suite consolidation).

#[path = "../cpan_test_helpers/mod.rs"]
mod cpan_test_helpers;

mod fix_unclosed_brace_2392;
mod fix_unclosed_brace_eof_2712;
mod fix_unclosed_brace_semicolon_remaining;
mod fix_unclosed_bracket_2398;
mod fix_unclosed_paren_bitshift;
mod fix_unclosed_paren_decl_as_expr;
mod fix_unclosed_paren_patterns;
mod fix_unclosed_paren_qualified_call;
mod fix_unclosed_paren_sort_coderef;
mod fix_unexpected_assign_expr;
mod fix_unexpected_comma_expr;
mod fix_unexpected_eq_expr;
mod fix_unexpected_rbrace_expr_2404;
mod fix_unexpected_rbrace_hash_key;
mod fix_unexpected_rbrace_proto_plus_2835;
mod fix_unexpected_slash_expr;
mod fix_unexpected_token_in_expr_2731;
mod fix_unexpected_word_op_and_writeexcel;
