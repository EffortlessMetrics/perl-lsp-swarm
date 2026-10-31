//! Extract fenced `json` blocks from Markdown and locate trailing commas (#16940).
//!
//! Documentation examples are copied verbatim by users, so a syntactically
//! invalid example is a real defect: it cannot be pasted into a config file.
//! #16359 removed six `perl.limits` keys and left two `docs/reference/CONFIG.md`
//! examples with a trailing comma before `}`, which `serde_json` rejects.
//!
//! # Why not "every fenced `json` block must be strict JSON"
//!
//! Many fenced `json` blocks in `docs/` are legitimately partial: bare fragments
//! such as `"includePaths": [ ... ]`, elision markers like `{ ... }`, and blocks
//! annotated with `//` comments (`docs/reference/COMMANDS_REFERENCE.md`).
//! Demanding strict parsing for all of them would fail documentation that is
//! correct as written, and "fixing" those blocks would invent content.
//!
//! A trailing comma is different. It is never legitimate in a `json` block, and
//! it is detectable without resolving elisions, comments, or partial wrappers,
//! so the check applies to every fenced `json` block in `docs/` with no false
//! positives. That is the defect class this module owns.

use crate::{NC, RED, display_path, is_text_file, walk_entries};
use color_eyre::eyre::{Result, eyre};
use std::fs;
use std::path::{Path, PathBuf};

/// A fenced ```json block extracted from a Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JsonBlock {
    /// 1-based line number of the block's opening fence within its document.
    pub(crate) fence_line: usize,
    /// The block's body with the opening and closing fences removed.
    pub(crate) body: String,
}

/// A comma that immediately precedes a closing `}` or `]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingComma {
    /// 1-based line number of the offending comma within its document.
    pub(crate) line: usize,
    /// 1-based column of the offending comma within its document.
    pub(crate) column: usize,
}

/// A trailing comma found in a document, paired with the document it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrailingCommaSite {
    /// Repository-relative path of the Markdown document.
    pub(crate) document: String,
    /// Location of the offending comma within that document.
    pub(crate) comma: TrailingComma,
}

/// Reject trailing commas in every fenced ```json example under `docs_dir`.
///
/// Returns `0` when the subtree is clean and `1` when at least one offending
/// comma is reported, so this composes as a gate exactly like `check-doc-paths`.
pub(crate) fn check_doc_json(repo_root: &Path, docs_dir: Option<&str>) -> Result<i32> {
    let docs_dir = docs_dir.unwrap_or("docs");
    let docs_path = resolve_docs_path(repo_root, docs_dir);

    if !docs_path.is_dir() {
        return Err(eyre!("Docs directory not found: {}", docs_path.display()));
    }

    let sites = trailing_commas_in_docs(repo_root, &docs_path);
    if sites.is_empty() {
        println!(
            "? No trailing commas in fenced json examples under {}",
            display_path(repo_root, &docs_path)
        );
        return Ok(0);
    }

    println!("{RED}? Found trailing commas in fenced json examples{NC}");
    for site in &sites {
        println!(
            "{}:{}:{}: trailing comma before a closing brace or bracket",
            site.document, site.comma.line, site.comma.column
        );
    }
    println!();
    println!("Fix: remove the trailing comma so the example parses as JSON");
    Ok(1)
}

/// Resolve `docs_dir` against `repo_root` unless it is already absolute.
fn resolve_docs_path(repo_root: &Path, docs_dir: &str) -> PathBuf {
    if Path::new(docs_dir).is_absolute() {
        PathBuf::from(docs_dir)
    } else {
        repo_root.join(docs_dir)
    }
}

/// Extract every fenced ```json block from `markdown`, in document order.
///
/// A fence opens on a line whose first info word is `json` and closes on the
/// next bare ``` line. Body text is returned verbatim so callers can parse or
/// scan it without re-deriving fence state.
pub(crate) fn fenced_json_blocks(markdown: &str) -> Vec<JsonBlock> {
    let mut blocks = Vec::new();
    let mut open: Option<usize> = None;
    let mut body = String::new();

    for (index, line) in markdown.lines().enumerate() {
        match open {
            None => {
                if is_json_fence(line) {
                    open = Some(index + 1);
                    body.clear();
                }
            }
            Some(fence_line) => {
                if is_closing_fence(line) {
                    blocks.push(JsonBlock { fence_line, body: std::mem::take(&mut body) });
                    open = None;
                } else {
                    body.push_str(line);
                    body.push('\n');
                }
            }
        }
    }

    // An unterminated fence is a malformed document, not a block. Dropping it
    // keeps this extractor total, so an unclosed fence cannot report a site.
    blocks
}

/// Locate trailing commas in every fenced ```json block of every Markdown file
/// beneath `docs_path`, returning one [`TrailingCommaSite`] per offending comma.
///
/// Non-Markdown and non-text files are skipped, matching the walk used by
/// `check-doc-paths`.
pub(crate) fn trailing_commas_in_docs(
    repo_root: &Path,
    docs_path: &Path,
) -> Vec<TrailingCommaSite> {
    let mut sites = Vec::new();

    for entry in walk_entries(docs_path) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        // Positive include test, matching `is_rust_source_file`: a negative
        // skip test would admit extensionless files into the walk.
        let is_markdown = path.extension().is_some_and(|ext| ext == "md");
        if !(is_markdown && is_text_file(path)) {
            continue;
        }
        let Ok(markdown) = fs::read_to_string(path) else {
            continue;
        };
        let document = display_path(repo_root, path);
        for block in fenced_json_blocks(&markdown) {
            for comma in trailing_commas(&block.body) {
                sites.push(TrailingCommaSite {
                    document: document.clone(),
                    comma: TrailingComma {
                        line: block.fence_line + comma.line,
                        column: comma.column,
                    },
                });
            }
        }
    }

    sites
}

/// Locate every comma that immediately precedes a closing `}` or `]` in `body`.
///
/// Line and column in the returned sites are 1-based and relative to `body`, so
/// a caller can offset them by the enclosing block's fence line.
///
/// String literals and `//` line comments are skipped, so a comma inside a
/// string value and a comma inside a comment are both correctly ignored.
/// Elision markers such as `{ ... }` carry no comma and never report.
pub(crate) fn trailing_commas(body: &str) -> Vec<TrailingComma> {
    let chars: Vec<char> = body.chars().collect();
    let mut sites = Vec::new();
    let mut last_significant: Option<usize> = None;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];

        if in_line_comment {
            if current == '\n' {
                in_line_comment = false;
            }
            index += 1;
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == '"' {
                in_string = false;
                // The closing quote is the last significant character, so a
                // comma inside the literal cannot be read as a trailing one.
                last_significant = Some(index);
            }
            index += 1;
            continue;
        }

        match current {
            // Whitespace is skipped rather than recorded: otherwise the newline
            // between `"key": 1,` and `}` would mask the comma entirely, which
            // is exactly the multi-line shape this check exists to catch.
            ' ' | '\t' | '\r' | '\n' => {}
            '"' => {
                in_string = true;
                last_significant = Some(index);
            }
            '/' if chars.get(index + 1) == Some(&'/') => in_line_comment = true,
            '}' | ']' => {
                if let Some(position) = last_significant.filter(|at| chars[*at] == ',') {
                    sites.push(TrailingComma {
                        line: line_of(&chars, position),
                        column: column_of(&chars, position),
                    });
                }
                last_significant = Some(index);
            }
            _ => last_significant = Some(index),
        }

        index += 1;
    }

    sites
}

/// Returns `true` when `line` opens a fenced block tagged `json`.
///
/// The info string is matched on its first word, so annotations such as
/// ```json title=response are recognised while `jsonc` and `javascript` are not.
fn is_json_fence(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return false;
    };
    rest.split_whitespace().next() == Some("json")
}

/// Returns `true` when `line` is a bare closing fence.
fn is_closing_fence(line: &str) -> bool {
    line.trim() == "```"
}

/// 1-based line number of the character at `index`.
fn line_of(chars: &[char], index: usize) -> usize {
    chars[..index].iter().filter(|c| **c == '\n').count() + 1
}

/// 1-based column of the character at `index`.
fn column_of(chars: &[char], index: usize) -> usize {
    let line_start = chars[..index].iter().rposition(|c| *c == '\n').map_or(0, |at| at + 1);
    chars[line_start..index].iter().filter(|c| **c != '\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::{fenced_json_blocks, trailing_commas};

    #[test]
    fn extracts_only_json_fences_in_document_order() {
        let markdown = concat!(
            "# Title\n",
            "```json\n",
            "{\"a\": 1}\n",
            "```\n",
            "```rust\n",
            "let x = 1;\n",
            "```\n",
            "```json title=response\n",
            "{\"b\": 2}\n",
            "```\n",
        );
        let blocks = fenced_json_blocks(markdown);
        assert_eq!(blocks.len(), 2, "only json fences are extracted: {blocks:?}");
        assert_eq!(blocks[0].body.trim(), "{\"a\": 1}");
        assert_eq!(blocks[1].body.trim(), "{\"b\": 2}");
        assert_eq!(blocks[0].fence_line, 2, "fence_line is 1-based");
    }

    #[test]
    fn ignores_non_json_info_strings() {
        for info in ["jsonc", "javascript", "JSON", ""] {
            let markdown = format!("```{info}\n{{\"a\": 1,}}\n```\n");
            assert!(
                fenced_json_blocks(&markdown).is_empty(),
                "info string {info:?} must not open a json block"
            );
        }
    }

    #[test]
    fn detects_multi_line_trailing_comma() {
        // The exact shape #16359 left behind in docs/reference/CONFIG.md.
        let body =
            "{\n  \"perl\": {\n    \"limits\": {\n      \"referencesCap\": 1000,\n    }\n  }\n}\n";
        let sites = trailing_commas(body);
        assert_eq!(sites.len(), 1, "expected one trailing comma, got {sites:?}");
        assert_eq!(sites[0].line, 4);
        assert_eq!(sites[0].column, 28, "column points at the comma itself");
    }

    #[test]
    fn detects_single_line_trailing_comma_in_both_closers() {
        assert_eq!(trailing_commas("{\"a\": 1,}").len(), 1);
        assert_eq!(trailing_commas("[\"a\",]").len(), 1);
        assert_eq!(trailing_commas("{\"a\": {\"b\": 2,},}").len(), 2, "nested closers");
    }

    #[test]
    fn accepts_valid_json_shapes() {
        for body in [
            "{\n  \"a\": 1,\n  \"b\": 2\n}\n",
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}\n",
            "{}\n",
            "[]\n",
        ] {
            assert!(trailing_commas(body).is_empty(), "false positive on {body:?}");
        }
    }

    #[test]
    fn ignores_commas_inside_strings_and_comments() {
        // A comma inside a string value, then a valid close.
        assert!(trailing_commas("{\"a\": \"x,\"}").is_empty());
        // An escaped quote inside a string, then a valid close.
        assert!(trailing_commas("{\"a\": \"he said \\\",\\\" ok\"}").is_empty());
        // A comma inside a line comment, then a valid close.
        assert!(trailing_commas("{\n  // trailing, in a comment\n  \"a\": 1\n}").is_empty());
    }

    #[test]
    fn tolerates_the_partial_json_shapes_docs_actually_uses() {
        // Elision markers and bare fragments appear throughout docs/ and are
        // legitimate as written; this check must stay silent about them.
        for body in [
            "{\n  \"workspace\": { ... },\n  \"limits\": { ... }\n}\n",
            "\"includePaths\": [\n  \"${workspaceFolder}/lib\",\n  \"${workspaceFolder}/local/lib/perl5\"\n]\n",
            "{\n  // Client request format\n  \"jsonrpc\": \"2.0\"\n}\n",
        ] {
            assert!(trailing_commas(body).is_empty(), "false positive on {body:?}");
        }
    }

    #[test]
    fn unterminated_fence_yields_no_block() {
        assert!(fenced_json_blocks("```json\n{\"a\": 1,}\n").is_empty());
    }
}

/// Repository-level enforcement. These are the tests that make the check
/// fail-closed on the real tree rather than only proving the scanner works.
#[cfg(test)]
mod repository {
    use super::{fenced_json_blocks, trailing_commas_in_docs};
    use color_eyre::eyre::{Result, eyre};
    use regex::Regex;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Canonical owner of the `perl.limits` key set.
    const LIMITS_SOURCE: &str = "crates/perl-lsp-rs-core/src/runtime/limits/mod.rs";
    /// The config reference whose examples readers copy verbatim.
    const CONFIG_DOC: &str = "docs/reference/CONFIG.md";

    /// Workspace root: `CARGO_MANIFEST_DIR` is `<repo>/crates/perl-ci-hygiene`.
    fn repo_root() -> Result<PathBuf> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_dir
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .ok_or_else(|| eyre!("cannot locate repo root from {}", manifest_dir.display()))
    }

    /// Skip when the canonical tree is absent (packaged crate, foreign layout)
    /// so these assertions never fail for a reason unrelated to their subject.
    fn repo_root_or_skip() -> Result<Option<PathBuf>> {
        let root = repo_root()?;
        Ok((root.join("Cargo.toml").is_file()).then_some(root))
    }

    /// The `perl.limits` keys the config reader actually consumes.
    ///
    /// Read back out of `LspLimits::update_from_value` rather than kept as a
    /// second copy here: #16359 exists precisely because a duplicated key list
    /// drifted away from the reader. Adding a key to the reader widens this set
    /// with no edit to this test; documenting a key the reader ignores fails it.
    fn supported_limit_keys(root: &Path) -> Result<BTreeSet<String>> {
        let source = fs::read_to_string(root.join(LIMITS_SOURCE))?;
        let start = source
            .find("fn update_from_value")
            .ok_or_else(|| eyre!("{LIMITS_SOURCE} no longer defines update_from_value"))?;
        let body = &source[start..];
        // `update_from_value` is the last item in the file's impl block; bound
        // the scan so a later `limits.get(..)` in a different function cannot
        // silently widen the supported set.
        let body = match body.find("\nimpl ") {
            Some(next_impl) => &body[..next_impl],
            None => body,
        };
        let key = Regex::new(r#"limits\.get\("([A-Za-z0-9_]+)"\)"#)?;
        Ok(key
            .captures_iter(body)
            .filter_map(|c| c.get(1).map(|m| m.as_str().to_owned()))
            .collect())
    }

    /// Every key used inside a `"limits"` object in the config examples.
    fn documented_limit_keys(markdown: &str) -> Result<BTreeSet<String>> {
        let mut keys = BTreeSet::new();
        for block in fenced_json_blocks(markdown) {
            let value: serde_json::Value = serde_json::from_str(&block.body).map_err(|e| {
                eyre!("{CONFIG_DOC} has a fenced json block that does not parse: {e}")
            })?;
            let Some(limits) = value.get("perl").and_then(|p| p.get("limits")) else {
                continue;
            };
            let Some(object) = limits.as_object() else {
                return Err(eyre!(
                    "{CONFIG_DOC} has a `perl.limits` example that is not an object"
                ));
            };
            keys.extend(object.keys().cloned());
        }
        Ok(keys)
    }

    /// #16940: no fenced `json` example anywhere in `docs/` may carry a
    /// trailing comma. This is the ratchet that rejects the reintroduced defect
    /// -- a `cargo test -p perl-ci-hygiene` failure, not a review convention.
    #[test]
    fn real_repo_docs_carry_no_trailing_comma_in_json_examples() -> Result<()> {
        let Some(root) = repo_root_or_skip()? else {
            return Ok(());
        };
        let sites = trailing_commas_in_docs(&root, &root.join("docs"));
        assert!(
            sites.is_empty(),
            "docs/ json examples must not contain trailing commas (#16940): {sites:#?}"
        );
        Ok(())
    }

    /// #16940: the config reference's examples must parse as strict JSON and
    /// name only keys the config reader still consumes.
    #[test]
    fn real_repo_config_examples_parse_and_name_supported_keys() -> Result<()> {
        let Some(root) = repo_root_or_skip()? else {
            return Ok(());
        };
        let markdown = fs::read_to_string(root.join(CONFIG_DOC))?;
        let documented = documented_limit_keys(&markdown)?;
        let supported = supported_limit_keys(&root)?;

        assert!(
            !documented.is_empty(),
            "{CONFIG_DOC} no longer exercises `perl.limits`; this test would pass vacuously"
        );
        let unsupported: Vec<_> = documented.difference(&supported).cloned().collect::<Vec<_>>();
        assert!(
            unsupported.is_empty(),
            "{CONFIG_DOC} documents perl.limits keys the reader does not consume \
             (removed or never read): {unsupported:?}; supported: {supported:?}"
        );
        Ok(())
    }
}
