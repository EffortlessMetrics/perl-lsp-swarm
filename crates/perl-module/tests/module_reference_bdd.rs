use perl_module::reference::{
    ModuleReferenceKind, extract_module_reference, find_module_reference,
};
use perl_tdd_support::must_some;

#[test]
fn given_use_statement_when_cursor_is_inside_module_then_reference_is_extracted() {
    let line = "use Demo::Worker;";
    let cursor = line.find("Worker").unwrap_or(0) + 2;

    let reference = find_module_reference(line, cursor);
    assert!(reference.is_some());
    if let Some(reference) = reference {
        assert_eq!(reference.kind, ModuleReferenceKind::Use);
        assert_eq!(reference.module_name, "Demo::Worker");
        assert_eq!(extract_module_reference(line, cursor), Some("Demo::Worker".to_string()));
    }
}

#[test]
fn given_require_statement_when_cursor_is_inside_module_then_reference_is_extracted() {
    let line = "require Demo::Worker;";
    let cursor = line.find("Demo").unwrap_or(0) + 1;

    assert_eq!(extract_module_reference(line, cursor), Some("Demo::Worker".to_string()));
}

#[test]
fn given_legacy_separator_when_cursor_is_inside_module_then_name_is_canonicalized() {
    let line = "use Demo'Worker;";
    let cursor = line.find("Worker").unwrap_or(0) + 1;

    assert_eq!(extract_module_reference(line, cursor), Some("Demo::Worker".to_string()));
}

#[test]
fn given_parent_statement_when_cursor_is_inside_argument_then_no_reference_is_extracted() {
    let line = "use parent 'Demo::Worker';";
    let cursor = line.find("Worker").unwrap_or(0);

    assert_eq!(extract_module_reference(line, cursor), None);
}

#[test]
fn given_non_import_context_when_cursor_is_inside_module_like_text_then_no_reference_is_extracted()
{
    let line = "my $x = Demo::Worker->new();";
    let cursor = line.find("Worker").unwrap_or(0);

    assert_eq!(extract_module_reference(line, cursor), None);
}

// --- non-ASCII / multi-byte character boundary tests (regression for #4938) ---

#[test]
fn given_emoji_before_use_when_cursor_is_on_module_then_reference_is_extracted() {
    // 😀 is 4 bytes; byte-wise idx++ would panic when slicing line[idx..] mid-codepoint.
    let line = r#"my $x = "😀" ; use My::Module;"#;
    let cursor = must_some(line.find("My::Module")) + 2;
    assert_eq!(extract_module_reference(line, cursor), Some("My::Module".to_string()),);
}

#[test]
fn given_emoji_on_line_when_cursor_is_mid_codepoint_byte_then_no_panic() {
    // 😀 spans bytes 9–12; byte index 10 is a continuation byte — passing it as
    // cursor_pos must not panic regardless of whether it falls inside a char.
    let line = r#"my $x = "😀😀"; use My::Module;"#;
    let mid_emoji_byte = 10; // inside 😀's 4-byte encoding
    // The call must not panic; result may be None (cursor is not on a module).
    let _ = extract_module_reference(line, mid_emoji_byte);
}

#[test]
fn given_accented_char_before_use_when_cursor_is_on_module_then_reference_is_extracted() {
    // 'ö' is 2 bytes (U+00F6); ensures 2-byte chars are also handled.
    let line = "my $zö = 1; use My::Module;";
    let cursor = must_some(line.find("My::Module")) + 2;
    assert_eq!(extract_module_reference(line, cursor), Some("My::Module".to_string()),);
}

#[test]
fn given_cjk_chars_before_require_when_cursor_is_on_module_then_reference_is_extracted() {
    // CJK characters are 3 bytes each in UTF-8.
    let line = r#"my $x = "こんにちは"; require My::Module;"#;
    let cursor = must_some(line.find("My::Module")) + 2;
    assert_eq!(extract_module_reference(line, cursor), Some("My::Module".to_string()),);
}

#[test]
fn given_mixed_emoji_and_ascii_on_same_line_when_cursor_is_on_module_then_reference_is_extracted() {
    // Mirrors the reproduction from issue #4938: emojis + umlaut before module call.
    let line = r#"my $obj = "😀😀 zö " . My::Module->new(); use My::Module;"#;
    let cursor = must_some(line.rfind("use My::Module")) + "use ".len() + 2;
    assert_eq!(extract_module_reference(line, cursor), Some("My::Module".to_string()),);
}
