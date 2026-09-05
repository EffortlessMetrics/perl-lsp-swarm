//! Corpus-wide evidence for the payload contract `token_subject` relies on (#9623).
//!
//! `ValidatedTokenStream`'s central rule is that a token's payload is
//! byte-identical to the source it spans, with one documented exemption (the
//! payload-free geometry-only `UnknownRest` recovery shape). If that were not
//! true of the real lexer for some token kind, the validator would reject
//! ordinary Perl — a false rejection far worse than the unchecked pairing it
//! replaces.
//!
//! That claim was originally established by a scratch sweep. A number quoted in
//! a doc comment is not evidence a later reader can re-run, so the sweep lives
//! here instead, where a lexer change that breaks the contract fails a test
//! rather than silently invalidating a comment.
//!
//! The sweep also pins the two structural facts the validator's rules are shaped
//! around, both of which are easy to get wrong from first principles:
//!
//! - tokens are ordered but **not contiguous** — trivia, POD, and heredoc bodies
//!   occupy the gaps, so ordering is non-overlap and never adjacency;
//! - the terminal EOF sits at `source.len()`.
//!
//! Tests return `Result` and use `ok_or`/`?` rather than `expect`/`panic`, per
//! the crate's integration-test lint policy.

use std::path::{Path, PathBuf};

use perl_parser_core::tokens::token_stream::{TokenKind, TokenStream};

/// Corpus directories, relative to the workspace root.
const CORPUS_DIRS: [&str; 4] = ["test_corpus", "fixtures", "testdata", "examples"];

/// Enough files that a contract violation in any ordinary construct would show
/// up. Guards against the sweep silently becoming a no-op if the corpus moves.
const MIN_FILES: usize = 200;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is <root>/crates/perl-parser-core.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn perl_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = CORPUS_DIRS.iter().map(|dir| root.join(dir)).collect();

    while let Some(path) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&path) else { continue };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
                continue;
            }
            // Compare the extension as an `OsStr`: converting through `to_str`
            // would silently drop a non-UTF-8 path instead of matching it.
            let is_perl =
                entry_path.extension().is_some_and(|ext| ext == "pl" || ext == "pm" || ext == "t");
            if is_perl {
                found.push(entry_path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_corpus_token_payload_is_the_source_it_spans() -> Result<(), Box<dyn std::error::Error>> {
    let root = workspace_root();
    let files = perl_files(&root);

    if files.is_empty() {
        // A vendored or packaged checkout carries no corpus. Skip rather than
        // fail: the contract is unchanged, the instrument is simply absent.
        return Ok(());
    }
    if files.len() < MIN_FILES {
        return Err(format!(
            "corpus sweep found only {} files under {CORPUS_DIRS:?}; expected at least \
             {MIN_FILES}. The sweep is the evidence for the payload contract, so a shrunken \
             corpus is an instrument failure, not a pass.",
            files.len()
        )
        .into());
    }

    let mut tokens_checked = 0usize;
    let mut files_read = 0usize;
    let mut unreadable: Vec<String> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    let mut saw_a_gap = false;

    for file in &files {
        // An unreadable or non-UTF-8 file must not silently shrink coverage:
        // the floor below counts files actually swept, not paths discovered.
        let source = match std::fs::read_to_string(file) {
            Ok(source) => source,
            Err(error) => {
                unreadable.push(format!("{}: {error}", file.display()));
                continue;
            }
        };
        files_read += 1;
        let display = file.strip_prefix(&root).unwrap_or(file).display();

        let mut stream = TokenStream::new(&source);
        let mut previous_end = 0usize;

        while let Ok(token) = stream.next() {
            tokens_checked += 1;
            let (start, end) = (token.start(), token.end());

            if token.kind() == TokenKind::Eof {
                if start != source.len() {
                    violations.push(format!(
                        "{display}: terminal EOF at {start}, source ends at {}",
                        source.len()
                    ));
                }
                break;
            }

            if start < previous_end {
                violations.push(format!(
                    "{display}: {:?} at {start}..{end} overlaps previous token ending at \
                     {previous_end}",
                    token.kind()
                ));
            }
            if start > previous_end {
                saw_a_gap = true;
            }
            previous_end = end;

            let Some(slice) = source.get(start..end) else {
                violations.push(format!(
                    "{display}: {:?} at {start}..{end} is not a valid slice of a {}-byte source",
                    token.kind(),
                    source.len()
                ));
                continue;
            };

            if !token.is_geometry_only() && *token.text != *slice {
                violations.push(format!(
                    "{display}: {:?} at {start}..{end} payload {:?} is not its source slice {:?}",
                    token.kind(),
                    &*token.text,
                    slice
                ));
            }

            if violations.len() > 20 {
                break;
            }
        }
    }

    if !violations.is_empty() {
        return Err(format!(
            "payload/geometry contract violated in {} place(s) across {files_read} swept files:\n  {}",
            violations.len(),
            violations.join("\n  ")
        )
        .into());
    }

    if files_read < MIN_FILES {
        return Err(format!(
            "only {files_read} of {} discovered files could be swept (needed {MIN_FILES}); \
             unreadable: {unreadable:?}. Coverage loss is an instrument failure, not a pass.",
            files.len()
        )
        .into());
    }

    // Negative control on the sweep itself: a corpus that produced no gaps
    // would mean this run never exercised trivia, and the non-contiguity fact
    // the ordering rule depends on would be untested here.
    if !saw_a_gap {
        return Err("the sweep observed no inter-token gaps, so it never exercised trivia; \
                    the non-adjacency premise is unproven by this run"
            .into());
    }

    assert!(
        tokens_checked > 10_000,
        "expected a substantial token population, checked only {tokens_checked}"
    );
    Ok(())
}
