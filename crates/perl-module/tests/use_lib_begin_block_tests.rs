use std::path::Path;

use perl_module::{
    UseLibAction, UseLibPath, extract_use_lib_operations, extract_use_lib_operations_with_offsets,
    no_lib_cancelled_paths_at_offset, resolve_use_lib_paths_from_source_at_offset,
};

#[test]
fn begin_block_leading_use_lib_is_active_for_following_use() {
    let source = "BEGIN {\n    use lib 'local/lib';\n    use Local::Thing;\n}\n";
    let offset = source.find("use Local::Thing;").unwrap_or(source.len());

    let paths =
        resolve_use_lib_paths_from_source_at_offset(source, offset, Path::new("/workspace"), None);

    assert_eq!(paths, vec!["local/lib".to_string()]);
}

#[test]
fn begin_block_comments_before_pragma_preserve_ordered_qw_paths() {
    let source = "BEGIN # compile-time include setup\n{\n    # Keep local modules first.\n    use lib qw(local/lib vendor/lib);\n    use Local::Thing;\n}\n";

    let operations = extract_use_lib_operations(source);

    assert_eq!(
        operations,
        vec![UseLibAction::Add(vec![
            UseLibPath { path: "local/lib".to_string(), from_findbin: false },
            UseLibPath { path: "vendor/lib".to_string(), from_findbin: false },
        ])]
    );
}

#[test]
fn begin_block_leading_no_lib_cancels_path_for_following_use() {
    let source = "BEGIN {\n    no lib 'local/lib';\n    use Local::Thing;\n}\n";
    let offset = source.find("use Local::Thing;").unwrap_or(source.len());

    let cancelled = no_lib_cancelled_paths_at_offset(source, offset, Path::new("/workspace"), None);

    assert_eq!(cancelled, vec!["local/lib".to_string()]);
}

#[test]
fn begin_block_unterminated_use_lib_remains_active_while_editing() {
    let source = "BEGIN {\n    use lib 'local/lib'\n    use Local::Thing;\n}\n";
    let offset = source.find("use Local::Thing;").unwrap_or(source.len());

    let paths =
        resolve_use_lib_paths_from_source_at_offset(source, offset, Path::new("/workspace"), None);

    assert_eq!(paths, vec!["local/lib".to_string()]);
}

#[test]
fn begin_block_use_lib_preserves_inner_statement_end_offset() {
    let source = "BEGIN { use lib 'x'; }\n";
    let operations = extract_use_lib_operations_with_offsets(source);

    assert_eq!(operations.len(), 1);
    let inner_statement_end = source.find(';').map_or(0, |offset| offset + 1);
    assert_eq!(operations[0].end_offset, inner_statement_end);
}

#[test]
fn begin_nested_block_does_not_unwrap_inner_pragma() {
    let source = "BEGIN { BEGIN { use lib 'x'; } }\n";

    assert!(extract_use_lib_operations(source).is_empty());
}

#[test]
fn begin_lookalikes_do_not_create_lib_operations() {
    let identifier = "BEGINNER { use lib 'fake'; }\n";
    let quoted = "BEGIN { \"use lib 'also_fake';\"; }\n";

    assert!(extract_use_lib_operations(identifier).is_empty());
    assert!(extract_use_lib_operations(quoted).is_empty());
}
