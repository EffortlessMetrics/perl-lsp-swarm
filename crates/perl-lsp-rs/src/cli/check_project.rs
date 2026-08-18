use std::collections::HashMap;
use std::path::Path;

/// A single parse error or advisory rendered with line/column context.
///
/// `message` is the `ParseError` Display string; `context` is the human-readable
/// `--> line N, column M` + source-line + caret rendering produced by
/// `format_parse_error_context` (#5519). Read errors (no `ParseError`) have an
/// empty context.
struct RenderedError {
    message: String,
    context: Vec<String>,
}

impl RenderedError {
    /// Build from a `ParseError`, rendering line/column context from `source`.
    fn from_parse(source: &str, err: &perl_parser::ParseError) -> Self {
        let message = format!("{err}");
        let context = super::format_parse_error_context(source, err);
        Self { message, context }
    }

    /// Build from a plain message (e.g. a read error) with no context.
    fn plain(message: String) -> Self {
        Self { message, context: Vec::new() }
    }
}

/// Blocking parse errors and advisory diagnostics recorded for one scanned file.
///
/// The two lists are kept apart because only `blocking` decides whether the file
/// parsed cleanly. Advisories are raised on source real `perl` accepts, so
/// counting them as failures would report valid Perl as unparsable. `--check`
/// draws the same line in `cli.rs::run_check`.
struct FileFindings {
    path: String,
    blocking: Vec<RenderedError>,
    advisory: Vec<RenderedError>,
}

/// A path the directory walk could not read.
struct UnreadablePath {
    path: String,
    reason: String,
}

pub(super) fn run_check_project(dir: &str) -> i32 {
    let root = Path::new(dir);
    let metadata = match root.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("{dir}: directory not found");
            return 1;
        }
        Err(error) => {
            eprintln!("{dir}: cannot access directory: {error}");
            return 1;
        }
    };
    if !metadata.is_dir() {
        eprintln!("{dir}: not a directory");
        return 1;
    }

    let mut results = ProjectCheckResults::default();
    // Skip vendored / build directories so the parsability verdict is driven by
    // the user's own code, not third-party deps. Symlink loops are still
    // detected and reported by `walkdir`'s built-in visited-inode guard.
    // (#5519 Slice B)
    let walker = walkdir::WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| !is_vendored_dir(entry));

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                // Dropping walk errors here would shorten `Files scanned` with no
                // trace, leaving the report to state a confident percentage over
                // whatever subset happened to be readable.
                results.unreadable.push(UnreadablePath {
                    path: error
                        .path()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| dir.to_string()),
                    reason: walk_error_reason(&error),
                });
                continue;
            }
        };

        let path = entry.path();
        if !is_supported_perl_file(path) {
            continue;
        }

        results.total += 1;
        let path_str = path.display().to_string();
        process_file(path, path_str, &mut results);
    }

    emit_report(dir, &results)
}

/// Render a `walkdir` error without the redundant path prefix it already carries.
fn walk_error_reason(error: &walkdir::Error) -> String {
    if error.loop_ancestor().is_some() {
        return "symbolic link loop".to_string();
    }
    match error.io_error() {
        Some(io_error) => io_error.to_string(),
        None => error.to_string(),
    }
}

#[derive(Default)]
struct ProjectCheckResults {
    total: usize,
    clean: usize,
    file_findings: Vec<FileFindings>,
    unreadable: Vec<UnreadablePath>,
    category_counts: HashMap<String, usize>,
}

fn is_supported_perl_file(path: &Path) -> bool {
    const EXTENSIONS: &[&str] = &["pm", "pl", "t"];
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| EXTENSIONS.contains(&e))
            .unwrap_or(false)
}

/// Directory names that hold vendored or generated code rather than the user's
/// own project source. Skipping them prevents a parsability verdict from being
/// driven by third-party code the user did not write and cannot fix (#5519
/// Slice B). The explicitly-requested root directory (depth 0) is never
/// skipped, even if its name matches a vendored pattern (e.g. `--check-project
/// vendor`).
fn is_vendored_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    // The root entry (depth 0) is the directory the user explicitly asked to
    // scan. Never skip it even if its basename matches a vendored name.
    if entry.depth() == 0 {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    is_vendored_dir_name(name)
}

/// Pure name check extracted from [`is_vendored_dir`] for unit testing.
fn is_vendored_dir_name(name: &str) -> bool {
    const VENDORED_DIRS: &[&str] = &[
        "local",        // Carton / `cpanm -l`
        "blib",         // `make test` build output
        "vendor",       // vendored deps
        "node_modules", // JS ecosystem
        ".git",         // VCS metadata
        "target",       // Rust build output (e.g. this project's own target/)
        ".build",       // Module::Build output
        "auto",         // XS build artifacts under lib/
    ];
    VENDORED_DIRS.contains(&name)
}

fn process_file(path: &Path, path_str: String, results: &mut ProjectCheckResults) {
    let source = match crate::util::read_text_file_with_encoding(path) {
        Ok(s) => s,
        Err(e) => {
            record_file_error(path_str, format!("read error: {e}"), results);
            return;
        }
    };

    let mut parser = perl_parser::Parser::new(&source);
    // `parse()` returns `Ok` whenever the parser recovered, so the recovered
    // diagnostics are read separately. Only the blocking ones decide the
    // verdict — advisories appear on files real `perl` accepts. `--check`
    // partitions the same way in `cli.rs::run_check`.
    let parse_result = parser.parse();
    let (blocking_errors, advisory_errors): (Vec<_>, Vec<_>) =
        parser.errors().iter().partition(|err| err.blocks_clean_parse());

    let mut blocking: Vec<RenderedError> = Vec::new();
    let advisory: Vec<RenderedError> =
        advisory_errors.iter().map(|err| RenderedError::from_parse(&source, err)).collect();

    if let Err(ref e) = parse_result {
        let rendered = RenderedError::from_parse(&source, e);
        record_category(&rendered.message, results);
        blocking.push(rendered);
    }

    for err in &blocking_errors {
        let rendered = RenderedError::from_parse(&source, err);
        record_category(&rendered.message, results);
        blocking.push(rendered);
    }

    if blocking.is_empty() {
        results.clean += 1;
    }

    if !blocking.is_empty() || !advisory.is_empty() {
        results.file_findings.push(FileFindings { path: path_str, blocking, advisory });
    }
}

fn record_file_error(path: String, message: String, results: &mut ProjectCheckResults) {
    record_category(&message, results);
    results.file_findings.push(FileFindings {
        path,
        blocking: vec![RenderedError::plain(message)],
        advisory: Vec::new(),
    });
}

fn record_category(message: &str, results: &mut ProjectCheckResults) {
    let category = categorize_error(message);
    results.category_counts.entry(category).and_modify(|c| *c += 1).or_insert(1);
}

/// Print one error under its file path, followed by the line/column context
/// rendering when available (#5519). Previously this printed a bare byte offset
/// (`at position 4821`) that the user could not act on; `--check` already solved
/// this with `format_parse_error_context`, and `--check-project` now shares that
/// rendering.
fn emit_rendered_error(path: &str, err: &RenderedError) {
    println!("  {path}: {}", err.message);
    for line in &err.context {
        println!("{line}");
    }
}

fn emit_report(dir: &str, results: &ProjectCheckResults) -> i32 {
    println!("Perl Project Parsability Report");
    println!("===============================");
    println!();
    println!("Directory: {dir}");
    println!("Files scanned: {}", results.total);

    if results.total == 0 {
        println!();
        emit_unreadable_section(&results.unreadable);
        println!("No Perl files (.pm, .pl, .t) found.");
        return 0;
    }

    let pct = (results.clean as f64 / results.total as f64) * 100.0;
    println!("Clean parses: {}/{} ({pct:.1}%)", results.clean, results.total);
    println!();

    let blocking_files: Vec<_> =
        results.file_findings.iter().filter(|file| !file.blocking.is_empty()).collect();
    if !blocking_files.is_empty() {
        println!("Parse errors:");
        for file in blocking_files {
            for err in &file.blocking {
                emit_rendered_error(file.path.as_str(), err);
            }
        }
        println!();
    }

    let advisory_files: Vec<_> =
        results.file_findings.iter().filter(|file| !file.advisory.is_empty()).collect();
    if !advisory_files.is_empty() {
        println!("Advisories (do not affect the parsability verdict):");
        for file in advisory_files {
            for err in &file.advisory {
                emit_rendered_error(file.path.as_str(), err);
            }
        }
        println!();
    }

    emit_unreadable_section(&results.unreadable);
    emit_category_section(&results.category_counts);

    let scope = if results.unreadable.is_empty() { "" } else { ", scanned files only" };
    if pct >= 80.0 {
        println!("Assessment: PASS ({pct:.1}% parsable{scope})");
        0
    } else {
        println!("Assessment: FAIL ({pct:.1}% parsable, threshold 80%{scope})");
        1
    }
}

/// Report paths the walk could not read so a short scan is never silent.
fn emit_unreadable_section(unreadable: &[UnreadablePath]) {
    if unreadable.is_empty() {
        return;
    }

    println!("Paths not scanned: {}", unreadable.len());
    for entry in unreadable {
        println!("  {}: {}", entry.path, entry.reason);
    }
    println!();
    println!("The parsability figures above cover scanned files only.");
    println!();
}

fn emit_category_section(category_counts: &HashMap<String, usize>) {
    if category_counts.is_empty() {
        return;
    }

    let mut cats: Vec<_> = category_counts.iter().collect();
    cats.sort_by_key(|(_, count)| std::cmp::Reverse(**count));
    println!("Top issue categories:");
    for (cat, count) in &cats {
        println!("  {cat}: {count}");
    }

    let suggested_fixes: Vec<_> = cats
        .iter()
        .filter_map(|(cat, _)| remediation_hint_for_category(cat).map(|hint| (cat.as_str(), hint)))
        .take(3)
        .collect();

    if !suggested_fixes.is_empty() {
        println!();
        println!("Suggested next steps:");
        for (category, suggestion) in suggested_fixes {
            println!("  {category}: {suggestion}");
        }
    }
    println!();
}

fn categorize_error(msg: &str) -> String {
    // The parser emits several surface strings for end-of-input failures:
    //   "Unexpected end of input", "Unclosed block: expected '}' but reached
    //   end of input", "expected ';' but found end of input",
    //   "Unexpected end of file", "... found EOF".  Group them all so users
    //   receive the unclosed-block remediation hint instead of "Other".
    let lower = msg.to_ascii_lowercase();
    if lower.contains("end of input")
        || lower.contains("end of file")
        || lower.contains("reached end")
        || lower.contains("found eof")
        || lower.contains("unexpected eof")
    {
        "Unexpected EOF".to_string()
    } else if msg.contains("expected") && msg.contains("found") {
        "Unexpected token".to_string()
    } else if msg.contains("Invalid syntax") {
        "Syntax error".to_string()
    } else if msg.contains("Lexer error") {
        "Lexer error".to_string()
    } else if msg.contains("recursion") || msg.contains("Recursion") {
        "Recursion limit".to_string()
    } else if msg.contains("read error") {
        "IO error".to_string()
    } else {
        "Other".to_string()
    }
}

fn remediation_hint_for_category(category: &str) -> Option<&'static str> {
    match category {
        "Unexpected EOF" => Some(
            "Check for unclosed blocks, quotes, or heredocs near the end of each failing file.",
        ),
        "Unexpected token" => Some(
            "Run `perl -c <file>` to compare parser output and inspect the token shown in the error.",
        ),
        "Syntax error" => {
            Some("Review recently edited lines for malformed declarations or expressions.")
        }
        "Lexer error" => {
            Some("Look for invalid bytes, malformed UTF-8, or unterminated strings/regex literals.")
        }
        "Recursion limit" => Some(
            "Minimize deeply nested constructs and isolate the smallest snippet that reproduces the issue.",
        ),
        "IO error" => {
            Some("Check file permissions and symbolic links, then rerun with readable paths.")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    // Test assertions favor `expect()` with a descriptive message over
    // silent unwraps; the workspace-wide deny is a production-code rule.
    #![allow(clippy::expect_used)]
    use super::{
        RenderedError, categorize_error, is_vendored_dir_name, remediation_hint_for_category,
    };

    #[test]
    fn rendered_error_from_parse_produces_line_column_not_byte_offset() {
        // #5519: --check-project previously printed a bare byte offset
        // (`at position N`) that the user could not act on. The fix reuses
        // `format_parse_error_context` so each error renders `--> line N,
        // column M` plus the source line and caret — matching `--check`.
        //
        // The unclosed brace on line 2 must produce a context pointing at line 2.
        let source = "my $x = 1;\nif ($x) {\n";
        let mut parser = perl_parser::Parser::new(source);
        let result = parser.parse();
        // An unclosed block yields a blocking error (either as the returned
        // Err or in the recovered diagnostics).
        let err: perl_parser::ParseError = result
            .err()
            .or_else(|| parser.errors().iter().next().cloned())
            .expect("source with an unclosed block must produce a parse error");

        let rendered = RenderedError::from_parse(source, &err);

        // The message still contains the parser's text (which may mention a
        // byte position internally), but the *context* lines must render a
        // human-readable `--> line N, column M`.
        let context_joined = rendered.context.join("\n");
        assert!(
            context_joined.contains("--> line"),
            "context must contain a line annotation, got: {context_joined:?}"
        );
        // The context must NOT be empty — every ParseError yields a context.
        assert!(
            !rendered.context.is_empty(),
            "from_parse must produce non-empty context for a parse error"
        );
    }

    #[test]
    fn categorize_error_maps_known_cases() {
        assert_eq!(categorize_error("Unexpected end of input while parsing"), "Unexpected EOF");
        assert_eq!(categorize_error("expected ; but found }"), "Unexpected token");
        assert_eq!(categorize_error("Invalid syntax near token"), "Syntax error");
        assert_eq!(categorize_error("Lexer error: invalid byte"), "Lexer error");
        assert_eq!(categorize_error("Recursion depth exceeded"), "Recursion limit");
        assert_eq!(categorize_error("read error: permission denied"), "IO error");
        assert_eq!(categorize_error("something new"), "Other");
    }

    #[test]
    fn categorize_error_groups_end_of_input_variants() {
        // #1991: parser emits several end-of-input surface strings that
        // previously fell through to "Other", suppressing the unclosed-block
        // remediation hint.
        assert_eq!(
            categorize_error("Unclosed block: expected '}' but reached end of input"),
            "Unexpected EOF"
        );
        assert_eq!(categorize_error("expected ';' but found end of input"), "Unexpected EOF");
        assert_eq!(categorize_error("Unexpected end of file"), "Unexpected EOF");
        assert_eq!(categorize_error("expected name but found EOF"), "Unexpected EOF");
        // Case-insensitivity: a stray lowercase "eof" substring must not
        // capture unrelated words (e.g. "does"), but genuine EOF messages of
        // any case must match.
        assert_eq!(categorize_error("parser reached end of input prematurely"), "Unexpected EOF");
        // Non-EOF "found" message stays in the token category.
        assert_eq!(categorize_error("expected ';' but found '}'"), "Unexpected token");
    }

    #[test]
    fn remediation_hints_cover_major_error_categories() {
        assert!(remediation_hint_for_category("Unexpected EOF").is_some());
        assert!(remediation_hint_for_category("Unexpected token").is_some());
        assert!(remediation_hint_for_category("Syntax error").is_some());
        assert!(remediation_hint_for_category("Lexer error").is_some());
        assert!(remediation_hint_for_category("Recursion limit").is_some());
        assert!(remediation_hint_for_category("IO error").is_some());
    }

    #[test]
    fn remediation_hints_skip_unknown_categories() {
        assert!(remediation_hint_for_category("Other").is_none());
    }

    #[test]
    fn vendored_dir_names_are_recognized() {
        // #5519 Slice B: the project checker must skip vendored / build
        // directories so a FAIL verdict is not driven by third-party code.
        for &name in
            &["local", "blib", "vendor", "node_modules", ".git", "target", ".build", "auto"]
        {
            assert!(is_vendored_dir_name(name), "`{name}` should be recognized as a vendored dir");
        }
        // User project directories must NOT be skipped.
        for &name in &["lib", "bin", "t", "script", "src", "app"] {
            assert!(
                !is_vendored_dir_name(name),
                "`{name}` should NOT be treated as a vendored dir"
            );
        }
    }
}
