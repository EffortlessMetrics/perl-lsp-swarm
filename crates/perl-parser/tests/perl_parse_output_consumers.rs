use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const ALLOWED_CURRENT_CONSUMERS: &[&str] = &["crates/perl-parser/src/bin/perl-parse.rs"];
const SKIPPED_DIRECTORIES: &[&str] = &[".git", "target", "node_modules", "archive"];
const TEXT_EXTENSIONS: &[&str] = &[
    "json", "md", "ps1", "rs", "sh", "toml", "ts", "txt", "yaml", "yml",
];

#[test]
fn legacy_output_consumer_inventory_is_explicit() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repository_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("perl-parser is not under <repository>/crates"))?;

    let mut patterns = Vec::new();
    for format in ["json", "sexp", "s-expression", "debug"] {
        patterns.push(format!("perl-parse -f {format}"));
        patterns.push(format!("perl-parse --format {format}"));
    }
    patterns.push(["perl-parse ", "script.pl"].concat());

    let mut observed = BTreeSet::new();
    let mut pending = vec![repository_root.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let entry_path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                let is_skipped = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| SKIPPED_DIRECTORIES.contains(&name));
                if !is_skipped {
                    pending.push(entry_path);
                }
                continue;
            }
            if !file_type.is_file() || !is_candidate_text_file(&entry_path) {
                continue;
            }

            let Ok(contents) = fs::read_to_string(&entry_path) else {
                continue;
            };
            if patterns.iter().any(|pattern| contents.contains(pattern)) {
                let relative = entry_path.strip_prefix(repository_root)?;
                observed.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let expected: BTreeSet<String> = ALLOWED_CURRENT_CONSUMERS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    assert_eq!(
        observed, expected,
        "a perl-parse legacy output consumer was added or removed; classify its subject, migration target, compatibility need, owner, and removal condition"
    );
    Ok(())
}

fn is_candidate_text_file(path: &Path) -> bool {
    if path.file_name().and_then(|name| name.to_str()) == Some("justfile") {
        return true;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension))
}
