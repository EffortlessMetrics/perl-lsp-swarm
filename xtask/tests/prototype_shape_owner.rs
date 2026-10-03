//! Recurrence guard: prototype slot classification has one owner (#16810).
//!
//! Provider and compiler crates must consume `PrototypeShape` rather than
//! walking a prototype string's `$ @ % & _ ;` characters as a second semantic
//! parser. The canonical projector lives in
//! `crates/perl-parser-core/src/prototype_shape/`.

use std::{
    fs,
    path::{Path, PathBuf},
};

const OWNER_PREFIX: &str = "crates/perl-parser-core/src/prototype_shape/";
const SCAN_ROOTS: &[&str] = &[
    "crates/perl-lsp-rs-core/src",
    "crates/perl-semantic-analyzer/src",
    "crates/perl-parser-core/src",
];

fn repo_root() -> PathBuf {
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                files.push(path);
            }
        }
    }
    files
}

fn is_owner(path: &Path) -> bool {
    path.to_string_lossy().replace('\\', "/").contains(OWNER_PREFIX)
}

/// The pre-#16810 signature-help loop classified slots by iterating prototype
/// characters. Reintroducing that loop (or an equivalent `proto.chars()` match
/// on `$`/`@`/`%`/`&`) is a second semantic parser.
fn local_slot_parser(source: &str) -> bool {
    let compact: String = source.chars().filter(|ch| !ch.is_whitespace()).collect();
    compact.contains("forchinproto.chars()")
        || compact.contains("forchinprototype.chars()")
        || (compact.contains("strip_prefix(\"prototype(\")")
            && compact.contains("matchch")
            && compact.contains("'$'")
            && compact.contains("'@'")
            && compact.contains("'%'"))
}

#[test]
fn providers_do_not_reparse_prototype_strings_for_slots() {
    let root = repo_root();
    let mut hits = Vec::new();
    for scan in SCAN_ROOTS {
        for path in rust_files(&root.join(scan)) {
            if is_owner(&path) {
                continue;
            }
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            if local_slot_parser(&source) {
                hits.push(path.display().to_string());
            }
        }
    }
    assert!(
        hits.is_empty(),
        "provider/compiler-local prototype slot parsers must use PrototypeShape (#16810): {hits:?}"
    );
}

#[test]
fn owner_module_exists() {
    assert!(
        repo_root().join("crates/perl-parser-core/src/prototype_shape/mod.rs").is_file(),
        "canonical projector owner is missing"
    );
}

#[test]
fn the_scan_detects_the_retired_character_loop() {
    let planted = r#"
        let prototype = attr.strip_prefix("prototype(").and_then(|s| s.strip_suffix(")"));
        if let Some(proto) = prototype {
            for ch in proto.chars() {
                match ch {
                    '$' => {}
                    '@' => {}
                    '%' => {}
                    _ => {}
                }
            }
        }
    "#;
    assert!(
        local_slot_parser(planted),
        "recurrence scan must still see the retired signature-help loop"
    );
    assert!(
        !local_slot_parser("let shape = PrototypeShape::project(proto);"),
        "canonical consumer must not trip the scan"
    );
}
