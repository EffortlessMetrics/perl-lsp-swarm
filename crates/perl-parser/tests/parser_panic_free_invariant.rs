//! Corpus-backed parser panic invariant.
//!
//! This test deliberately accepts parse errors. The invariant is narrower and
//! more important for an editor-facing parser: malformed input must produce a
//! result, never unwind the parser process.

use perl_parser::Parser;
use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").to_path_buf()
}
fn files_under(root: &Path, include: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];

    while let Some(path) = pending.pop() {
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let Ok(entries) = fs::read_dir(path) else {
                continue;
            };
            pending.extend(entries.flatten().map(|entry| entry.path()));
        } else if include(&path) {
            files.push(path);
        }
    }

    files.sort();
    files
}

fn parse_without_unwinding(source: &str) {
    let _ = Parser::new(source).parse();
}

#[test]
fn repository_and_fuzz_corpora_are_panic_free() {
    let root = workspace_root();
    let mut inputs = files_under(&root.join("test_corpus"), |path| {
        matches!(path.extension().and_then(|ext| ext.to_str()), Some("pl" | "pm" | "t"))
    });
    inputs.extend(files_under(&root.join("fuzz/corpus"), |_| true));
    inputs.sort();

    assert!(!inputs.is_empty(), "parser panic invariant discovered no corpus inputs");

    let mut panics = Vec::new();
    for path in inputs {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => panic!("failed to read {}: {error}", path.display()),
        };
        let source = String::from_utf8_lossy(&bytes);
        if panic::catch_unwind(AssertUnwindSafe(|| parse_without_unwinding(&source))).is_err() {
            panics.push(path.display().to_string());
        }
    }

    assert!(panics.is_empty(), "parser panicked for corpus inputs: {panics:?}");
}

#[test]
fn deterministic_malformed_bytes_are_panic_free() {
    let cases: &[&[u8]] = &[
        b"\0\xff\xfe\x80",
        b"(((({{{{[[[[",
        b"${\x00} =~ s{/{/g",
        b"use v5.40; sub { <<'EOF'\n",
        b"\xff\xff\xff\xff\xff\xff\xff\xff",
    ];

    for bytes in cases {
        let source = String::from_utf8_lossy(bytes);
        assert!(
            panic::catch_unwind(AssertUnwindSafe(|| parse_without_unwinding(&source))).is_ok(),
            "parser panicked for deterministic malformed input {bytes:?}"
        );
    }
}
