//! Consolidated integration tests for typeglob, fat-arrow, and hash parser fixes (#5012 item 7).
//!
//! Former standalone targets under `tests/fix_{fat_arrow,hash,typeglob}_*.rs` are preserved
//! as submodules so assertions and issue references stay reviewable.

#[path = "../cpan_test_helpers/mod.rs"]
mod cpan_test_helpers;

mod fix_fat_arrow_array_ref_4168;
mod fix_fat_arrow_builtin_autoquote;
mod fix_fat_arrow_expr;
mod fix_fat_arrow_filehandle_tests;
mod fix_fat_arrow_ternary_branch;
mod fix_hash_brace_depth_in_blocks;
mod fix_hash_keyword_variable;
mod fix_hash_slice_qw_delimiter;
mod fix_hash_subscript_regex_op_keys;
mod fix_typeglob_arrow_tests;
mod fix_typeglob_punct_vars;
mod fix_typeglob_punct_vars_extended;
mod fix_typeglob_punctuation;
mod fix_typeglob_sub_assign_tests;
mod fix_typeglob_sub_assignment_tests;
