//! Which source files are test-only, when the `#[cfg(test)]` is not in the file.
//!
//! `first_cfg_test_line_number` answers "where does this file's test scope
//! begin?" and that is enough for a file carrying an inline `#[cfg(test)] mod
//! tests { … }`. It is not enough when the whole file *is* the test module: the
//! attribute then sits on the parent's declaration,
//!
//! ```text
//! // crates/perl-lsp-rs/src/runtime/lifecycle/mod.rs
//! #[cfg(test)]
//! mod final_surface_census;
//! ```
//!
//! and the child file contains no `#[cfg(test)]` line at all. A file-local scan
//! reads every line of it as production.
//!
//! That is not hypothetical: it is what put 14 phantom production `expect` sites
//! into `check-unwraps-prod` against a baseline of 1, and kept #13838 open for
//! three weeks asking whether a module that has never been compiled into a
//! non-test build was production-reachable. Clippy got the same tree right,
//! because it runs after `cfg` expansion rather than over lines of text.
//!
//! This module resolves the declarations instead, so a checker can ask the
//! question the compiler answers.

use color_eyre::eyre::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{first_cfg_test_line_number, read_lines};

/// Files declared by `mod <name>;` anywhere at or after `path`'s first
/// `#[cfg(test)]` boundary.
///
/// Moved here from the panic-test command. It is deliberately over-inclusive:
/// it sweeps every `mod` declaration after the boundary, not only the ones the
/// attribute guards. For panic-test that is the safe direction — the worst case
/// is scanning a file that holds no test panics.
///
/// Over-inclusive is a choice about *which declarations* to follow. Where a
/// declaration leads is not a choice, so the hop resolves through
/// [`module_directory`] like every other hop in this module. The version that
/// came over from the panic-test command used `with_file_name`, which reads a
/// declaration in `foo.rs` as naming a file beside `foo.rs` instead of one
/// inside `foo/`. Twenty-seven test-only module files across twelve crates
/// resolve only under the 2018 rule — among them
/// `xtask/src/tasks/emacs_train_packet/tests.rs`, whose six panic sites the
/// inventory never saw.
///
/// **Do not use it to exclude a file from a production check.** There the same
/// over-inclusion is a false negative. Use [`test_only_source_files`].
pub(crate) fn external_test_module_files(path: &Path, lines: &[String]) -> Vec<PathBuf> {
    let Some(start_line) = first_cfg_test_line_number(path).ok().filter(|line| *line != usize::MAX)
    else {
        return Vec::new();
    };
    let Some(base) = module_directory(path) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    // `mod helper;` inside `mod tests { … }` names `tests/helper.rs`, not
    // `helper.rs`. Resolving it beside the module directory finds nothing, and
    // a child whose whole file is test-only carries no `#[cfg(test)]` of its
    // own, so `complete_panic_site_inventory` never reaches it by any other
    // route: its panic sites are simply absent from the inventory. Measured on
    // this tree, that is two sites in
    // `crates/perl-core-harness/src/invocation_trace/test_support.rs`, whose
    // parent declares it from inside `pub mod invocation_trace { … }`.
    //
    // Only the resolution is corrected. Which declarations get swept stays
    // deliberately over-inclusive, for the reason the doc comment gives.
    let mut inline: Vec<(usize, String)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if trimmed.starts_with('}') {
            while inline.last().is_some_and(|(open, _)| *open >= indent) {
                inline.pop();
            }
            continue;
        }

        // Strip the attributes so `#[cfg(test)] mod tests {` is read as the
        // block opener it is, exactly as `module_edges` reads it.
        let mut rest = trimmed;
        while let Some(end) = attribute_end(rest) {
            let Some(tail) = rest.get(end + 1..) else {
                break;
            };
            rest = tail.trim_start();
        }
        if rest.is_empty() {
            continue;
        }

        if let Some(name) = inline_module_name(rest) {
            inline.push((indent, name));
            continue;
        }

        // The nesting is tracked over the whole file so a block that opened
        // above the boundary is still known below it; only declarations at or
        // after the boundary are swept.
        if index + 1 >= start_line {
            let mut dir = base.clone();
            for (_, segment) in &inline {
                dir = dir.join(segment);
            }
            push_declared_module_files(&dir, rest, &mut files);
        }
    }
    files
}

fn push_declared_module_files(base: &Path, line: &str, files: &mut Vec<PathBuf>) {
    let Some(name) = declared_module_name(line.trim()) else {
        return;
    };
    // Both spellings of a child module. Rust allows only one to exist, so
    // probing for each is a disambiguation, not a guess. Paths stay as built
    // rather than canonicalized: the caller strips `repo_root` off them to
    // form the identity a registry row is keyed on.
    for candidate in [base.join(format!("{name}.rs")), base.join(&name).join("mod.rs")] {
        if candidate.is_file() {
            files.push(candidate);
        }
    }
}

/// One `mod <name>;` declaration, resolved to the file(s) it can name.
///
/// `gated` is true when an attribute directly above the declaration carries a
/// `cfg` predicate that cannot hold outside a test build — `cfg(test)` itself,
/// or an `all(…)` with such a predicate among its conjuncts. A predicate that
/// can hold in a production build, `cfg(any(test, feature = "…"))` among them,
/// leaves the edge ungated and the module in production scope.
///
/// Gating is safe here and would not be safe in a pre-filter over the file
/// list, because phase 3 of [`test_only_source_files`] lets any production
/// declaration reaching the same file win the tie. A file excluded before that
/// phase runs never reaches the rule that would have protected it.
#[derive(Debug)]
struct ModuleEdge {
    files: Vec<PathBuf>,
    gated: bool,
}

/// Every module declaration in `path`, with the attribute state that guards it.
///
/// An attribute guards the next item and nothing else, so each declaration is
/// paired with the attributes immediately above it — on the same line, which is
/// legal Rust, or on the lines above it, however far each attribute's brackets
/// reach. The attributes are read through [`guarded_item`], the one attribute
/// reader for production scope, and `crate::first_cfg_test_line_number` reads
/// the within-file boundary through it too, so the whole-file verdict and the
/// within-file boundary are two questions answered by one parse and cannot
/// disagree about a construct either one can read.
///
/// `crates/perl-lsp-rs/src/runtime/mod.rs:46` is why this matters: it carries
/// `#[cfg(all(test, feature = "workspace"))]` over line 47, and `mod workspace;`
/// on line 61 is ungated production code holding two `unsafe impl` blocks.
/// Sweeping forward from the first `#[cfg(test)]` swallows it.
fn module_edges(path: &Path, lines: &[String]) -> Vec<ModuleEdge> {
    let Some(module_dir) = module_directory(path) else {
        return Vec::new();
    };
    let Some(file_dir) = path.parent().map(Path::to_path_buf) else {
        return Vec::new();
    };

    let mut edges = Vec::new();
    let mut gated = false;
    let mut redirect: Option<String> = None;
    // Each entry is the indent that opened the block, its module name, and
    // whether the declaration that opened it was gated. A `mod` inside a
    // `#[cfg(test)] mod tests { … }` is compiled only under `cfg(test)` just
    // as surely as one whose own declaration carries the attribute, so the
    // gate has to survive entering the block rather than being reset by it.
    let mut inline: Vec<(usize, String, bool)> = Vec::new();

    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            index += 1;
            continue;
        }

        if trimmed.starts_with('}') {
            let indent = lines[index].len() - lines[index].trim_start().len();
            while inline.last().is_some_and(|(open, _, _)| *open >= indent) {
                inline.pop();
            }
            gated = false;
            redirect = None;
            index += 1;
            continue;
        }

        // Consume every attribute guarding the item through the one reader
        // production scope is decided by — the same reader
        // `crate::first_cfg_test_line_number` drives — so an attribute whose
        // brackets span several physical lines gates the declaration here
        // exactly as it opens test scope for the within-file checks.
        let item = guarded_item(lines, index);
        for (_, attr) in &item.attributes {
            gated |= attribute_is_a_test_gate(attr);
            if let Some(value) = path_attribute_target(attr) {
                redirect = Some(value.to_string());
            }
        }
        let rest = item.text;
        index = item.line;
        if rest.is_empty() {
            continue;
        }
        let indent = lines[item.line].len() - lines[item.line].trim_start().len();

        if let Some(name) = inline_module_name(rest) {
            inline.push((indent, name, gated));
            gated = false;
            redirect = None;
            index += 1;
            continue;
        }

        // `include!("literal.rs")` splices a file in at this point. It is not
        // a module declaration, so nothing above reads it -- and a production
        // file reachable only that way would be absent from the production
        // closure, which is the one direction phase 3 cannot recover from.
        // `perl-parser-core`'s parser is built this way. The path is relative
        // to the directory holding *this* file, and a computed path
        // (`concat!(env!("OUT_DIR"), ...)`) is skipped: it names generated
        // source outside the scanned tree, so it cannot be excluded anyway.
        if let Some(target) = rest
            .strip_prefix("include!(\"")
            .and_then(|rest| rest.split_once("\")"))
            .map(|(path, _)| path)
        {
            // Canonicalized for the same reason `module_files` canonicalizes:
            // the graph is keyed on canonical paths. An include spelled
            // `live/../shared.rs` would otherwise enter the production set
            // under a `PathBuf` that never equals the gated alias's canonical
            // spelling of the same file, so phase 3's `difference` would not
            // let production win the tie and would exclude a file the compiler
            // builds. One file, two spellings, opposite verdicts.
            if let Ok(included) = file_dir.join(target).canonicalize()
                && included.is_file()
            {
                // A splice is gated exactly as a declaration is: by the
                // attribute on its own line, and by any enclosing inline
                // module that was itself gated. `#[cfg(test)] include!(…)`
                // names source the compiler builds only under `cfg(test)`,
                // so reading it as a production edge holds every panic site
                // in the spliced file inside the production closure and
                // makes rule (3) hand it back at the end.
                let gated = gated || inline.iter().any(|(_, _, open_gated)| *open_gated);
                edges.push(ModuleEdge { files: vec![included], gated });
            }
            gated = false;
            redirect = None;
            index += 1;
            continue;
        }

        if let Some(name) = declared_module_name(rest) {
            // A `#[path]` on a declaration outside every inline module block is
            // relative to the directory holding this source file. Inside one it
            // is relative to the module directory, walked down the inline
            // chain. The module directory is also where an ordinary
            // declaration resolves.
            let base = if redirect.is_some() && inline.is_empty() {
                file_dir.clone()
            } else {
                let mut dir = module_dir.clone();
                for (_, segment, _) in &inline {
                    dir = dir.join(segment);
                }
                dir
            };
            let files = module_files(&base, &name, redirect.as_deref());
            if !files.is_empty() {
                // An enclosing gated inline module gates everything it
                // declares, however the declaration itself is spelled.
                let gated = gated || inline.iter().any(|(_, _, open_gated)| *open_gated);
                edges.push(ModuleEdge { files, gated });
            }
        }

        // Any item ends the attribute block, whether or not it was a `mod`.
        gated = false;
        redirect = None;
        index += 1;
    }
    edges
}

/// The attributes directly guarding an item, and the item they guard.
#[derive(Debug)]
struct GuardedItem<'a> {
    /// One `(line, text)` per attribute: `line` is the 0-based line the
    /// attribute starts on, `text` the attribute with each physical line it
    /// spans trimmed and joined by one space.
    attributes: Vec<(usize, String)>,
    /// 0-based line carrying the guarded item's own head.
    line: usize,
    /// The guarded item's head text, trimmed.
    text: &'a str,
}

/// Read the item at or after `start` together with every attribute guarding it.
///
/// This is the one attribute reader for production scope: [`module_edges`]
/// answers whole-file scope through it and
/// `crate::first_cfg_test_line_number` answers within-file scope through it,
/// so both verdicts come from one parse. Before it existed the two readers
/// parsed independently — one bracket scan bounded to a single physical line,
/// two regexes that needed `test` on the `#[cfg(all(` line — and a
/// `#[cfg(all(…))]` spelled across several lines, the live shape in
/// `crates/perl-corpus/src/loading/`, was production to both for different
/// reasons while any third spelling would have divided them again.
///
/// Blank and comment lines are skipped, each `#[…]` at the item's head is
/// consumed across as many physical lines as its brackets span, and the scan
/// stops at the first text that is neither. An attribute whose brackets never
/// close reads as no attribute at all, so malformed input ends the scan
/// instead of consuming the rest of the file.
fn guarded_item<'a>(lines: &'a [String], start: usize) -> GuardedItem<'a> {
    let mut attributes = Vec::new();
    let mut line = start;
    let mut col = lines[start].len() - lines[start].trim_start().len();
    while let Some(current) = lines.get(line) {
        let Some(rest) = current.get(col..) else {
            break;
        };
        let trimmed = rest.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            line += 1;
            col = 0;
            continue;
        }
        if !trimmed.starts_with("#[") {
            break;
        }
        let Some((end_line, end_col, text)) =
            attribute_extent(lines, line, col + (rest.len() - trimmed.len()))
        else {
            break;
        };
        attributes.push((line, text));
        line = end_line;
        col = end_col;
    }
    // The scan stopped at the first text that is neither attribute, blank, nor
    // comment: the item's own head — the remainder of the line the last
    // attribute ended on, or a line of its own — or the end of the file.
    let text =
        lines.get(line).and_then(|current| current.get(col..)).map(str::trim_start).unwrap_or("");
    GuardedItem { attributes, line, text }
}

/// The extent of the attribute whose `#[` sits at `lines[line][col..]`:
/// `(end_line, end_col, text)`, with `end_col` just past the closing bracket
/// and `text` each physical line trimmed and joined by one space. `None` when
/// the brackets never close — the same verdict `attribute_end` reaches within
/// one line, extended across lines for the multiline spellings rustfmt leaves
/// untouched, `#[cfg(all(\n test,\n))]` among them.
fn attribute_extent(
    lines: &[String],
    mut line: usize,
    mut col: usize,
) -> Option<(usize, usize, String)> {
    if !lines.get(line)?.get(col..)?.starts_with("#[") {
        return None;
    }
    let mut depth = 0usize;
    let mut text = String::new();
    loop {
        let rest = lines.get(line)?.get(col..)?;
        let mut closing = None;
        for (offset, character) in rest.char_indices() {
            match character {
                '[' => depth += 1,
                ']' => {
                    // As in `attribute_end`, a closing bracket with nothing
                    // open is malformed input: no attribute, not a panic.
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        closing = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        match closing {
            Some(offset) => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(rest.get(..=offset)?.trim());
                return Some((line, col + offset + 1, text));
            }
            None => {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(rest.trim());
                line += 1;
                col = 0;
            }
        }
    }
}

/// The 1-based line of the file's first `cfg(test)` boundary, or
/// [`usize::MAX`] when the file carries none.
///
/// The within-file half of production scope: every production check stops
/// reporting at this line. It is read through [`guarded_item`], the same
/// reader [`module_edges`] resolves whole-file scope through, so the boundary
/// a multiline `#[cfg(all(…))]` opens here is the same gate that declares the
/// module it guards test-only.
///
/// Both `#[cfg(test)]` and `#[cfg(all(test, …))]` count only when the item
/// they guard is a `mod` — block or declared, visibility-qualified spellings
/// (`pub(crate) mod`, `pub(super) mod`, `pub(in path) mod`) included — so a
/// test-gated attribute on any other item (a single-line `use` import, a
/// `const`, a `thread_local!`) does not cut the file in half.
/// `crates/perl-lsp-rs/src/runtime/language/symbols.rs` carried exactly that
/// shape: a `#[cfg(test)] use` near the top truncated every production check
/// that stops here, hiding the per-call `Regex::new(...)` calls it was meant
/// to gate (#16389). `cfg(any(test, …))` never bounds anything — it holds in
/// production builds whenever its other arm does.
pub(crate) fn first_cfg_test_boundary(lines: &[String]) -> usize {
    let mut index = 0usize;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('}') {
            index += 1;
            continue;
        }
        let item = guarded_item(lines, index);
        // A test gate bounds the file only when it guards a module: the
        // mod-lookahead the line-based reader used to spell as "skip blanks,
        // attributes, and comments, then require `mod`" is what
        // `module_head_name` answers about the guarded item's head. Comment and
        // attribute lines between the gate and the `mod` change nothing —
        // `guarded_item` skips them — and a commented-out `mod` line is a
        // comment, so it can never satisfy the check (#16389).
        //
        // The boundary asks whether the line *declares* a module; the nesting
        // predicates ask whether the line *opens* one whose scope runs on to
        // the next line, and they deliberately reject a head that ends in `}`.
        // Sharing them made a compact `#[cfg(test)] mod tests { fn it() {} }`
        // indistinguishable from no boundary at all, so every production check
        // downstream scanned through the test module (#16520).
        let guards_a_module = module_head_name(item.text).is_some();
        if guards_a_module {
            if let Some((line, _)) =
                item.attributes.iter().find(|(_, attr)| attribute_is_plain_cfg_test(attr))
            {
                return line + 1;
            }
            if let Some((line, _)) =
                item.attributes.iter().find(|(_, attr)| attribute_is_a_test_gate(attr))
            {
                return line + 1;
            }
        }
        index = item.line + 1;
    }
    usize::MAX
}

/// One inline test-gated module's extent, as the 1-based lines it covers.
///
/// `start_line` is where the gate opens test scope — the same answer
/// [`first_cfg_test_boundary`] gives — and `end_line` is the line carrying the
/// module's closing brace. Production resumes on the line after it.
///
/// The pair is what a start line alone cannot say. Every production check read
/// the start and treated the rest of the file as test scope, so a production
/// item written *after* `mod tests { … }` was never scanned at all:
///
/// ```text
/// pub fn leaked() { … }            // scanned
/// #[cfg(test)]
/// mod tests { … }                   // test scope
/// pub fn also_leaked() { … }        // never scanned
/// ```
///
/// Rust convention puts `mod tests` last, which is why the gap is usually
/// invisible — and why it is a *false negative*, the direction that surfaces
/// nothing when it is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CfgTestSpan {
    start_line: usize,
    end_line: usize,
}

impl CfgTestSpan {
    /// Whether the 1-based `line` falls inside this test-gated module.
    pub(crate) fn covers(&self, line: usize) -> bool {
        line >= self.start_line && line <= self.end_line
    }
}

/// Whether the 1-based `line` is inside any inline test-gated module.
///
/// This is the within-file production-scope question. It takes the file's spans
/// rather than re-reading the file, because every production check asks it once
/// per line and re-parsing per line would make each scan quadratic in the
/// file's length.
pub(crate) fn line_is_test_scope(spans: &[CfgTestSpan], line: usize) -> bool {
    spans.iter().any(|span| span.covers(line))
}

/// Every inline test-gated module in `lines`, in source order.
///
/// A file can hold more than one — a second `#[cfg(test)] mod` after production
/// code between the two — so the answer is a list, not a single extent. Answering
/// with only the first would fix the false negative and open a false positive in
/// its place: everything after the first module, including the second module,
/// would be read as production.
///
/// The walk is the same one [`first_cfg_test_boundary`] makes through
/// [`guarded_item`], so the two agree about which item a gate opens. It resumes
/// after each module's closing line rather than after the module head, which is
/// what lets it find a later gate at all.
pub(crate) fn cfg_test_spans(lines: &[String]) -> Vec<CfgTestSpan> {
    // The walk reads the projection, not the text. A test module in this
    // repository routinely carries a fixture whose *content* is Rust source, and
    // `code_only` is what keeps a `#[cfg(test)]` or a `}` inside a string from
    // answering a structural question about the file.
    let code = code_only(lines);
    let mut spans = Vec::new();
    let mut index = 0usize;
    while index < code.len() {
        let trimmed = code[index].trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('}') {
            index += 1;
            continue;
        }
        let item = guarded_item(&code, index);
        let start_line = item
            .attributes
            .iter()
            .find(|(_, attr)| attribute_is_plain_cfg_test(attr))
            .or_else(|| item.attributes.iter().find(|(_, attr)| attribute_is_a_test_gate(attr)))
            .map(|(line, _)| line + 1);
        if let Some(start_line) = start_line
            && module_head_name(item.text).is_some()
        {
            let end_line = gated_module_end_line(&code, item.line);
            spans.push(CfgTestSpan { start_line, end_line });
            // `end_line` is 1-based and the next line to examine is the one
            // after it, which is this 0-based cursor.
            index = end_line;
            continue;
        }
        index = item.line + 1;
    }
    spans
}

/// The 1-based line on which the test-gated module headed at `head_line` ends.
///
/// Braces are counted on the projection [`code_only`] produces, so a brace that
/// is only *text* — inside a string, inside a comment — does not open or close
/// anything. The count starts at the head's own line, which answers the three
/// shapes the module can take without a special case each:
///
/// - a compact `#[cfg(test)] mod tests { fn it() {} }` nets to zero on its head
///   line, so it ends there;
/// - a `mod foo;` declaration has no braces at all, so it ends there too — its
///   test scope is the child file, which [`test_only_source_files`] resolves;
/// - a block that runs on ends on the line that closes it.
///
/// A block that never closes — a truncated or hand-written fixture — ends at the
/// last line, so unresolved input keeps the answer the start line alone used to
/// give rather than a guess.
fn gated_module_end_line(code: &[String], head_line: usize) -> usize {
    let mut depth = 0i64;
    for (offset, line) in code.iter().enumerate().skip(head_line) {
        for character in line.chars() {
            match character {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        if depth <= 0 {
            return offset + 1;
        }
    }
    code.len()
}

/// `lines` with every comment body and string-literal body replaced by spaces.
///
/// Character positions and indentation survive, so the result still answers
/// structural questions — which line opens a block, which line closes one —
/// while text that only *looks* like structure cannot answer them.
///
/// This is not defensive. `crates/perl-lsp-rs/src/runtime/dispatch/text_document.rs`
/// holds tests for its own cfg-stripping helper, and their fixtures embed Rust
/// source in raw strings:
///
/// ```text
/// let adversarial = r#"
/// #[cfg(any(test, feature = "test-fallbacks"))]
/// fn gated() {
///     let fake = "unmatched { in a string";
///     /* outer { /* nested } */ still gated */
/// }
/// fn production() {
///     on_references(params, request_id);
/// }
/// "#;
/// ```
///
/// That `}` sits alone at column 0 and closes nothing. An extent reader that
/// believed the enclosing `mod tests { … }` ended there would hand the rest of
/// the module back to the production checks — measured on that one file, 19
/// `println!` sites, all of them test code.
///
/// Constructs that open on one line and close on another are the whole problem
/// here: raw strings, block comments, and strings continued with a trailing
/// backslash are carried in the returned state. Everything else closes within
/// its own line and is blanked there.
fn code_only(lines: &[String]) -> Vec<String> {
    let mut block_comment = false;
    let mut raw_hashes: Option<usize> = None;
    let mut continued_string = false;
    lines
        .iter()
        .map(|line| blanked_code(line, &mut block_comment, &mut raw_hashes, &mut continued_string))
        .collect()
}

/// Replace `[start, end)` of `chars` in `out` with spaces.
fn blank(out: &mut [char], start: usize, end: usize) {
    for slot in out.iter_mut().take(end).skip(start) {
        *slot = ' ';
    }
}

/// The first unescaped `"` at or after `start`, or `None` when the line ends
/// without one — the caller then carries the string to the next line.
fn unescaped_quote(chars: &[char], start: usize) -> Option<usize> {
    let mut escaped = false;
    for (index, character) in chars.iter().enumerate().skip(start) {
        if escaped {
            escaped = false;
        } else if *character == '\\' {
            escaped = true;
        } else if *character == '"' {
            return Some(index);
        }
    }
    None
}

/// The first occurrence of `needle` at or after `start`.
fn find_sequence(chars: &[char], start: usize, needle: &str) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    // A needle longer than the remaining line cannot match, and asking the
    // range for an out-of-bounds window is a panic rather than a `None`.
    if needle.is_empty() || start > chars.len() || chars.len() - start < needle.len() {
        return None;
    }
    (start..=chars.len() - needle.len())
        .find(|index| chars[*index..*index + needle.len()] == needle[..])
}

/// The line's code, with comments and string bodies blanked to spaces.
///
/// The three `&mut` parameters are the multi-line construct state, threaded
/// across lines by [`code_only`].
fn blanked_code(
    line: &str,
    block_comment: &mut bool,
    raw_hashes: &mut Option<usize>,
    continued_string: &mut bool,
) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = chars.clone();
    let mut index = 0usize;

    // Resume whatever opened on an earlier line. Each case either closes here
    // and falls through to the ordinary scan, or runs to end of line and leaves
    // the construct open for the next one.
    if *block_comment {
        match find_sequence(&chars, index, "*/") {
            Some(end) => {
                blank(&mut out, index, end + 2);
                index = end + 2;
                *block_comment = false;
            }
            None => {
                blank(&mut out, index, chars.len());
                return out.into_iter().collect();
            }
        }
    } else if let Some(hashes) = *raw_hashes {
        let terminator = raw_terminator(hashes);
        match find_sequence(&chars, index, &terminator) {
            Some(end) => {
                blank(&mut out, index, end + terminator.chars().count());
                index = end + terminator.chars().count();
                *raw_hashes = None;
            }
            None => {
                blank(&mut out, index, chars.len());
                return out.into_iter().collect();
            }
        }
    } else if *continued_string && let Some(end) = unescaped_quote(&chars, index) {
        blank(&mut out, index, end + 1);
        index = end + 1;
        *continued_string = false;
    } else if *continued_string {
        blank(&mut out, index, chars.len());
        return out.into_iter().collect();
    }

    while index < chars.len() {
        let rest: String = chars[index..].iter().collect();
        if rest.starts_with("//") {
            blank(&mut out, index, chars.len());
            break;
        }
        if rest.starts_with("/*") {
            blank(&mut out, index, index + 2);
            index += 2;
            match find_sequence(&chars, index, "*/") {
                Some(end) => {
                    blank(&mut out, index, end + 2);
                    index = end + 2;
                }
                None => {
                    blank(&mut out, index, chars.len());
                    *block_comment = true;
                    break;
                }
            }
            continue;
        }
        // `r"…"` and `r#"…"#`. The hash count is what makes the terminator
        // unambiguous, so it is counted rather than assumed.
        if rest.starts_with('r') {
            let hashes = rest
                .strip_prefix('r')
                .unwrap_or(rest.as_str())
                .chars()
                .take_while(|character| *character == '#')
                .count();
            if rest[1 + hashes..].starts_with('"') {
                let terminator = raw_terminator(hashes);
                blank(&mut out, index, index + 2 + hashes);
                index += 2 + hashes;
                match find_sequence(&chars, index, &terminator) {
                    Some(end) => {
                        let stop = end + terminator.chars().count();
                        blank(&mut out, index, stop);
                        index = stop;
                    }
                    None => {
                        blank(&mut out, index, chars.len());
                        *raw_hashes = Some(hashes);
                        break;
                    }
                }
                continue;
            }
        }
        if rest.starts_with('"') {
            blank(&mut out, index, index + 1);
            index += 1;
            match unescaped_quote(&chars, index) {
                Some(end) => {
                    blank(&mut out, index, end + 1);
                    index = end + 1;
                }
                None => {
                    blank(&mut out, index, chars.len());
                    *continued_string = true;
                    break;
                }
            }
            continue;
        }
        // A char literal may hold a brace (`'}'`) or a quote (`'"'`), and both
        // have to be consumed whole: a `'"'` left half-read turns its `"` into
        // the start of a string that never closes, which swallows the rest of
        // the file. `class_model.rs:816` and `:1978` both spell
        // `trim_matches('"')`, and both were found by the real tree rather than
        // by a fixture. A lifetime such as `'a` has no closing quote here and
        // stays code.
        if rest.starts_with('\'')
            && let Some(end) = char_literal_end(&chars, index)
        {
            blank(&mut out, index, end);
            index = end;
            continue;
        }
        index += 1;
    }
    out.into_iter().collect()
}

/// The terminator for a raw string carrying `hashes` `#` characters.
fn raw_terminator(hashes: usize) -> String {
    format!("\"{}", "#".repeat(hashes))
}

/// The index just past the char literal opened at `start`, or `None` when what
/// sits there is a lifetime rather than a one-character literal.
///
/// The two differ only in where the closing quote can be, and that is the whole
/// test: `'}'` and `'"'` close three characters in, `'a` has no closing quote on
/// the line at all.
fn char_literal_end(chars: &[char], start: usize) -> Option<usize> {
    let body = *chars.get(start + 1)?;
    let closing = if body == '\\' { start + 3 } else { start + 2 };
    (chars.get(closing) == Some(&'\'')).then_some(closing + 1)
}

/// Whether `attr` is `#[cfg(test)]` itself — the plain spelling of the test
/// gate — as opposed to a conjunction that merely requires `test`.
fn attribute_is_plain_cfg_test(attr: &str) -> bool {
    attr.strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
        .is_some_and(|predicate| predicate.trim() == "test")
}

/// The file `#[path = "…"]` redirects a declaration to, or `None` when `attr`
/// is some other attribute.
///
/// Reading only one spelling sends the declaration back to its plain module
/// name, which resolves to a different file or to none — and a gated
/// declaration that lands on the wrong file classifies that file as test-only.
///
/// Which spellings can actually reach here is a question about this repository,
/// not about Rust, because `cargo fmt --check` is a gate. Measured against
/// rustfmt: it rewrites `#[path="x.rs"]` to the spaced form and `#[ cfg(test) ]`
/// to the tight one, so neither survives the gate — but it leaves
/// `#[path = r"y.rs"]` exactly as written. **The raw literal is the one
/// residual spelling a committed file can carry**, so it is the one that
/// matters; the whitespace tolerance below is consistency, not necessity.
fn path_attribute_target(attr: &str) -> Option<&str> {
    let value = attr
        .strip_prefix("#[")?
        .trim_start()
        .strip_prefix("path")?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    string_literal_text(value)
}

/// The text inside the Rust string literal at the head of `value`.
///
/// `attribute_end` finds an attribute's extent by counting brackets without
/// reading string literals, so a path containing `]` would already have been
/// truncated before it got here. That is pathological for a file name and is
/// left alone deliberately: widening the bracket scan is a change to every
/// attribute, not to this one.
fn string_literal_text(value: &str) -> Option<&str> {
    if let Some(raw) = value.strip_prefix('r') {
        let after_hashes = raw.trim_start_matches('#');
        let hash_count = raw.len().checked_sub(after_hashes.len())?;
        let body = after_hashes.strip_prefix('"')?;
        let mut closing = String::with_capacity(hash_count + 1);
        closing.push('"');
        for _ in 0..hash_count {
            closing.push('#');
        }
        return body.find(&closing).and_then(|end| body.get(..end));
    }

    // A plain literal can escape its own quote, so the closing one is the first
    // unescaped `"` rather than the first `"`.
    let body = value.strip_prefix('"')?;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        match character {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return body.get(..index),
            _ => {}
        }
    }
    None
}

/// Whether `attr` is a `cfg` attribute whose predicate cannot hold outside a
/// test build.
fn attribute_is_a_test_gate(attr: &str) -> bool {
    attr.strip_prefix("#[cfg(")
        .and_then(|rest| rest.strip_suffix(")]"))
        .is_some_and(predicate_is_test_only)
}

/// Whether a `cfg` predicate can only hold when `test` is set.
///
/// `test` itself qualifies. So does `all(…)` with a qualifying conjunct: every
/// conjunct of an `all` must hold, so one that cannot hold outside a test build
/// makes the whole predicate unsatisfiable outside one.
///
/// `any(…)` never qualifies — it holds when a single arm does, and an arm like
/// `feature = "workspace"` holds in a production build. `not(…)` never
/// qualifies either, and `not(test)` is the opposite claim.
///
/// `#[cfg(all(test, feature = "workspace"))]` at
/// `crates/perl-lsp-rs/src/runtime/mod.rs:46` is the live instance. Reading it
/// as production put a module no production build compiles back into scope,
/// which is what #16251's own resolver got right and this one did not.
fn predicate_is_test_only(predicate: &str) -> bool {
    let predicate = predicate.trim();
    if predicate == "test" {
        return true;
    }
    let Some(inner) = predicate.strip_prefix("all(").and_then(|rest| rest.strip_suffix(')')) else {
        return false;
    };
    conjuncts(inner).iter().any(|arm| predicate_is_test_only(arm))
}

/// `inner` split on the commas that separate one predicate from the next.
///
/// Splits at parenthesis depth zero only, and skips commas inside a string
/// literal, so `all(test, feature = "a,b")` is two conjuncts rather than three.
fn conjuncts(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut start = 0usize;
    let mut previous = '\0';
    for (index, ch) in inner.char_indices() {
        if in_string {
            if ch == '"' && previous != '\\' {
                in_string = false;
            }
        } else {
            match ch {
                '"' => in_string = true,
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    if let Some(part) = inner.get(start..index) {
                        parts.push(part);
                    }
                    start = index + 1;
                }
                _ => {}
            }
        }
        previous = ch;
    }
    if let Some(part) = inner.get(start..) {
        parts.push(part);
    }
    parts
}

/// The end index of the attribute starting at the head of `rest`, if there is
/// one. Counts brackets, so `#[cfg(any(test, feature = "x"))]` is one
/// attribute and not a prefix of one.
fn attribute_end(rest: &str) -> Option<usize> {
    if !rest.starts_with("#[") {
        return None;
    }
    let mut depth = 0usize;
    for (index, ch) in rest.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                // The `#[` guard above means the first `[` precedes every `]`,
                // so `depth` is never zero here. Saying that with `checked_sub`
                // rather than a comment keeps the arithmetic total: a closing
                // bracket with nothing open is malformed input, and malformed
                // input yields "no attribute here", not a panic in a checker.
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The directory a file's `mod name;` declarations resolve against.
///
/// `lib.rs`, `main.rs` and `mod.rs` own their containing directory; any other
/// `foo.rs` owns `foo/`. That is the 2018-edition rule, and it is why the
/// legacy sweep's `with_file_name` cannot be reused here — not for the first
/// hop and not for the transitive ones either.
fn module_directory(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    match path.file_stem()?.to_str()? {
        "lib" | "main" | "mod" => Some(parent.to_path_buf()),
        stem => Some(parent.join(stem)),
    }
}

fn inline_module_name(trimmed: &str) -> Option<String> {
    // Trailing whitespace after the brace is rustfmt-incidental but legal in
    // hand-written fixtures; the mod check must not break on it (#16389).
    let rest = trimmed.trim_end().strip_suffix('{')?.trim_end();
    module_name_after_visibility(rest)
}

fn declared_module_name(trimmed: &str) -> Option<String> {
    let rest = trimmed.trim_end().strip_suffix(';')?;
    module_name_after_visibility(rest)
}

/// The module name in a head that declares a module anywhere on the line —
/// `mod foo { … }`, `pub(crate) mod foo;` — whatever follows the brace.
///
/// The within-file boundary reader's half of `mod` recognition, and
/// deliberately not [`inline_module_name`]: that predicate is also the
/// `inline` nesting stack's push test, so it must keep requiring the brace to
/// be the line's last token — a compact module pushed onto that stack opens
/// and closes on one line and would never be popped, misplacing every
/// declaration resolved after it. The name grammar itself is still read once,
/// by [`module_name_after_visibility`]; this only differs in where the head is
/// allowed to end.
fn module_head_name(trimmed: &str) -> Option<String> {
    let end = trimmed.find(['{', ';'])?;
    module_name_after_visibility(trimmed.get(..end)?)
}

/// The module name in `pub(crate) mod foo` and its spellings, or `None` when
/// the text is not a module declaration at all.
fn module_name_after_visibility(rest: &str) -> Option<String> {
    let rest = rest.trim().strip_prefix("pub").map_or(rest, |tail| tail);
    let rest = rest.trim_start();
    let rest = match (rest.starts_with('('), rest.find(')')) {
        (true, Some(close)) => rest.get(close + 1..)?.trim(),
        _ => rest,
    };
    let name = rest.trim().strip_prefix("mod ")?.trim();
    (!name.is_empty() && name.chars().all(|ch| ch == '_' || ch.is_ascii_alphanumeric()))
        .then(|| name.to_string())
}

/// The file(s) a declaration can name, keeping only those that exist.
fn module_files(base: &Path, name: &str, redirect: Option<&str>) -> Vec<PathBuf> {
    if let Some(relative) = redirect {
        let redirected = base.join(relative);
        return if redirected.is_file() {
            redirected.canonicalize().into_iter().collect()
        } else {
            Vec::new()
        };
    }
    // Both spellings of a child module. Rust allows only one to exist, so
    // probing for each is a disambiguation, not a guess.
    let mut files = Vec::new();
    let sibling = base.join(format!("{name}.rs"));
    let nested = base.join(name).join("mod.rs");
    if sibling.is_file() {
        files.extend(sibling.canonicalize());
    }
    if nested.is_file() {
        files.extend(nested.canonicalize());
    }
    files
}

/// Whether the compiler reaches this file without any `mod` declaration.
fn is_crate_root(path: &Path) -> bool {
    let is_bin_target = path.parent().and_then(Path::file_name).is_some_and(|dir| dir == "bin");
    let stem = path.file_stem().and_then(|stem| stem.to_str());
    is_bin_target || matches!(stem, Some("lib" | "main" | "build"))
}

/// Every file in `files` that is compiled only under `cfg(test)` because an
/// ancestor module declared it that way.
///
/// Three properties carry the exclusion, and the dangerous direction — dropping
/// a file the compiler really does build into production — is what each of them
/// is for:
///
/// 1. **Production reachability is computed first**, from the crate roots over
///    ungated declarations only. A file the compiler can reach without passing
///    a `#[cfg(test)]` is production, whatever else also names it.
/// 2. **A gated edge only seeds test scope from a production-reachable file.**
///    A declaration in a file nothing reaches establishes nothing.
/// 3. **Production reachability wins the tie.** One gated reference does not
///    make a physical file test-only when a production declaration or `#[path]`
///    alias also names it, so the intersection stays in scope.
///
/// Transitive, because a test-only module makes its whole subtree test-only,
/// and resolved through the same 2018-edition rule at every hop rather than by
/// looking beside the declaring file. The closure terminates: each pass can
/// only add files from the finite input set, and a file already in the set is
/// never expanded twice.
pub(crate) fn test_only_source_files(files: &[PathBuf]) -> Result<BTreeSet<PathBuf>> {
    // Reachability is a question about files, not about spellings. A
    // `#[path = "../shared.rs"]` in `live/mod.rs` resolves to
    // `live/../shared.rs`: the same file as `shared.rs` and a different
    // `PathBuf`. Compared as written, a production alias and a test alias for
    // one file never meet, and rule (3) below could not do its job. So the
    // graph is keyed canonically and mapped back to the caller's own paths at
    // the end.
    let mut canonical: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for path in files {
        if let Ok(real) = path.canonicalize() {
            canonical.entry(real).or_insert_with(|| path.clone());
        }
    }

    let mut edges: BTreeMap<PathBuf, Vec<ModuleEdge>> = BTreeMap::new();
    for (real, path) in &canonical {
        let lines = read_lines(path)?;
        edges.insert(real.clone(), module_edges(path, &lines));
    }

    // (1) What the compiler reaches without crossing a `#[cfg(test)]`.
    let mut production: BTreeSet<PathBuf> =
        canonical.keys().filter(|path| is_crate_root(path)).cloned().collect();
    let mut frontier: Vec<PathBuf> = production.iter().cloned().collect();
    while let Some(path) = frontier.pop() {
        for edge in edges.get(&path).into_iter().flatten() {
            if edge.gated {
                continue;
            }
            for child in &edge.files {
                if production.insert(child.clone()) {
                    frontier.push(child.clone());
                }
            }
        }
    }

    // (2) What a gated declaration in production code puts behind `cfg(test)`,
    // and everything those modules declare in turn.
    let mut test_only: BTreeSet<PathBuf> = BTreeSet::new();
    let mut frontier: Vec<PathBuf> = Vec::new();
    for (path, file_edges) in &edges {
        if !production.contains(path) {
            continue;
        }
        for edge in file_edges.iter().filter(|edge| edge.gated) {
            for child in &edge.files {
                if test_only.insert(child.clone()) {
                    frontier.push(child.clone());
                }
            }
        }
    }
    while let Some(path) = frontier.pop() {
        for edge in edges.get(&path).into_iter().flatten() {
            for child in &edge.files {
                if test_only.insert(child.clone()) {
                    frontier.push(child.clone());
                }
            }
        }
    }

    // (3) A file production also reaches is production.
    Ok(test_only.difference(&production).filter_map(|real| canonical.get(real).cloned()).collect())
}

#[cfg(test)]
mod tests {
    use color_eyre::eyre::ensure;

    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Same shape as the panic-test command's `TempRepo`: this crate builds
    /// throwaway trees from `std::env::temp_dir` rather than taking a
    /// `tempfile` dev-dependency.
    struct Tree {
        path: PathBuf,
    }

    impl Tree {
        fn new(label: &str) -> Result<Self> {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir()
                .join(format!("perl-ci-hygiene-test-scope-{label}-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }

        fn write(&self, rel: &str, contents: &str) -> Result<PathBuf> {
            let path = self.path.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, contents)?;
            Ok(path)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    // Every fixture is rooted at a `lib.rs`, because reachability from a crate
    // root is what decides production scope. A fixture that handed the
    // resolver a bare `mod.rs` would be asking a question the compiler never
    // asks.

    /// The panic-test sweep resolves its hop the same way every other hop in
    /// this module does. `census.rs` owns `census/`, so `mod rows;` inside it
    /// names `census/rows.rs` — never the `rows.rs` lying beside it, which is
    /// a different module belonging to the crate root.
    #[test]
    fn the_panic_sweep_resolves_a_child_under_the_module_directory() -> Result<()> {
        let tree = Tree::new("sweep-dir")?;
        let census = tree.write("census.rs", "#[cfg(test)]\nmod rows;\n")?;
        let test_child = tree.write("census/rows.rs", "fn f() { panic!(\"boom\"); }\n")?;
        let unrelated_sibling = tree.write("rows.rs", "fn g() {}\n")?;

        let found = external_test_module_files(&census, &read_lines(&census)?);
        ensure!(found.contains(&test_child), "census/rows.rs is the module census.rs declares");
        ensure!(
            !found.contains(&unrelated_sibling),
            "the sibling rows.rs belongs to the crate root, not to census"
        );
        Ok(())
    }

    /// The hop the sweep used to lose: a declaration *inside* an inline module
    /// resolves under that module's directory, not beside the declaring file.
    ///
    /// This is the panic inventory's one blind spot rather than a cosmetic
    /// miss. A wholly test-only child carries no `#[cfg(test)]` of its own, so
    /// `complete_panic_site_inventory`'s other route — walking every workspace
    /// file and reading from its own boundary — never reaches it either. On
    /// this tree it hid two live sites in
    /// `crates/perl-core-harness/src/invocation_trace/test_support.rs`, both
    /// now registered.
    #[test]
    fn the_panic_sweep_resolves_a_child_under_its_inline_module() -> Result<()> {
        let tree = Tree::new("sweep-inline")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod tests {\n    mod helper;\n}\n")?;
        let nested = tree.write("tests/helper.rs", "fn f() { panic!(\"boom\"); }\n")?;
        let unrelated_sibling = tree.write("helper.rs", "fn g() {}\n")?;

        let found = external_test_module_files(&root, &read_lines(&root)?);
        ensure!(
            found.contains(&nested),
            "`mod helper;` inside `mod tests` names tests/helper.rs; found {found:?}"
        );
        ensure!(
            !found.contains(&unrelated_sibling),
            "the helper.rs beside lib.rs is a different module nothing here declares"
        );
        Ok(())
    }

    /// A block that closed before the boundary must not keep gating what
    /// follows it. Without popping the nesting, `mod census;` at the crate
    /// root would resolve under `tests/`, which is the same defect pointed the
    /// other way.
    #[test]
    fn the_panic_sweep_leaves_a_closed_inline_module_behind() -> Result<()> {
        let tree = Tree::new("sweep-inline-closed")?;
        let root =
            tree.write("lib.rs", "mod tests {\n    mod helper;\n}\n#[cfg(test)]\nmod census;\n")?;
        tree.write("tests/helper.rs", "fn f() {}\n")?;
        let census = tree.write("census.rs", "fn f() { panic!(\"boom\"); }\n")?;

        let found = external_test_module_files(&root, &read_lines(&root)?);
        ensure!(
            found.contains(&census),
            "census.rs sits at the crate root, outside the closed block; found {found:?}"
        );
        Ok(())
    }

    #[test]
    fn child_declared_under_cfg_test_is_test_only() -> Result<()> {
        let tree = Tree::new("child")?;
        let root = tree.write("lib.rs", "mod real;\n#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;
        let real = tree.write("real.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[root, census.clone(), real.clone()])?;
        ensure!(found.contains(&census), "the cfg(test) child must be test-only");
        ensure!(!found.contains(&real), "an ungated sibling must stay in production scope");
        Ok(())
    }

    #[test]
    fn nested_mod_rs_spelling_is_resolved() -> Result<()> {
        let tree = Tree::new("nested")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod census;\n")?;
        let nested = tree.write("census/mod.rs", "fn f() {}\n")?;

        let found = test_only_source_files(&[root, nested.clone()])?;
        ensure!(found.contains(&nested), "the mod.rs spelling resolves; found {found:?}");
        Ok(())
    }

    #[test]
    fn test_only_subtree_is_transitive() -> Result<()> {
        let tree = Tree::new("transitive")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census/mod.rs", "mod deep;\n")?;
        let deep = tree.write("census/deep.rs", "fn f() { y.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, census.clone(), deep.clone()])?;
        ensure!(found.contains(&census));
        ensure!(found.contains(&deep), "a module of a test-only module is test-only");
        Ok(())
    }

    /// The finding that falsified the first version of this module.
    ///
    /// A test-only `census.rs` declaring `mod rows;` means `census/rows.rs`,
    /// not the sibling `rows.rs`. Resolving transitive edges beside the
    /// declaring file gets it wrong in both directions at once: it misses the
    /// real test child and it excludes a production file that merely shares
    /// the name. `protocol/final_surface_inventory.rs` -> `rows.rs` is the
    /// live instance in this repository.
    #[test]
    fn transitive_edges_resolve_under_the_module_directory_not_beside_the_file() -> Result<()> {
        let tree = Tree::new("transitive-dir")?;
        let root = tree.write("lib.rs", "mod rows;\n#[cfg(test)]\nmod census;\n")?;
        let census = tree.write("census.rs", "mod rows;\n")?;
        let test_child = tree.write("census/rows.rs", "fn f() { y.unwrap(); }\n")?;
        let production_sibling = tree.write("rows.rs", "fn g() { z.unwrap(); }\n")?;

        let found = test_only_source_files(&[
            root,
            census.clone(),
            test_child.clone(),
            production_sibling.clone(),
        ])?;
        ensure!(found.contains(&census));
        ensure!(found.contains(&test_child), "census/rows.rs is the module census.rs declares");
        ensure!(
            !found.contains(&production_sibling),
            "the sibling rows.rs is a different module and stays in production scope"
        );
        Ok(())
    }

    /// One gated reference does not make a physical file test-only.
    #[test]
    fn a_file_a_production_declaration_also_reaches_stays_in_scope() -> Result<()> {
        let tree = Tree::new("shared")?;
        let root = tree.write(
            "lib.rs",
            "mod live;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod under_test;\n",
        )?;
        let live = tree.write("live/mod.rs", "#[path = \"../shared.rs\"]\nmod shared;\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, live.clone(), shared.clone()])?;
        ensure!(
            !found.contains(&shared),
            "a production #[path] alias also names this file, so it is production"
        );
        ensure!(!found.contains(&live));
        Ok(())
    }

    /// The same fixture without the production alias: now the exclusion is
    /// correct, which is what makes the control above discriminating.
    #[test]
    fn the_same_file_is_test_only_once_no_production_declaration_names_it() -> Result<()> {
        let tree = Tree::new("shared-negative")?;
        let root = tree.write(
            "lib.rs",
            "mod live;\n#[cfg(test)]\n#[path = \"shared.rs\"]\nmod under_test;\n",
        )?;
        let live = tree.write("live/mod.rs", "fn g() {}\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, live, shared.clone()])?;
        ensure!(found.contains(&shared), "nothing in production reaches it now");
        Ok(())
    }

    /// `#[cfg(test)] mod tests;` on one physical line is legal Rust, and an
    /// attribute-only line test never sees it.
    #[test]
    fn an_attribute_and_its_declaration_on_one_line_are_read() -> Result<()> {
        let tree = Tree::new("same-line")?;
        let root = tree.write("lib.rs", "#[cfg(test)] mod assertions;\n")?;
        let assertions = tree.write("assertions.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, assertions.clone()])?;
        ensure!(found.contains(&assertions), "the redirected file is test-only; found {found:?}");
        Ok(())
    }

    /// The spelling the repository's format gate cannot rewrite away. rustfmt
    /// normalises `#[path="x.rs"]` and `#[ cfg(test) ]`, so neither reaches a
    /// committed file; it leaves a raw literal exactly as written, so this is
    /// the one form a scanner has to read for itself.
    #[test]
    fn a_raw_string_path_attribute_names_the_same_file() -> Result<()> {
        let tree = Tree::new("raw-path")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\n#[path = r\"shared.rs\"]\nmod under_test;\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, shared.clone()])?;
        ensure!(found.contains(&shared), "a raw literal redirects like a plain one");
        Ok(())
    }

    /// A hashed raw literal closes on `"#`, not on the first quote.
    #[test]
    fn a_hashed_raw_path_attribute_reads_to_its_own_terminator() -> Result<()> {
        let tree = Tree::new("hashed-raw-path")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\n#[path = r#\"shared.rs\"#]\nmod under_test;\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, shared.clone()])?;
        ensure!(found.contains(&shared), "r#\"…\"# names the same file");
        Ok(())
    }

    /// Rust does not require whitespace around the `=` in an attribute, so the
    /// parser must not either. Reading only the spaced spelling sends the
    /// declaration back to its plain module name — here `under_test.rs`, which
    /// does not exist — and the redirect target stays in production scope.
    #[test]
    fn a_path_attribute_without_spaces_names_the_same_file() -> Result<()> {
        let tree = Tree::new("unspaced-path")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\n#[path=\"shared.rs\"]\nmod under_test;\n")?;
        let shared = tree.write("shared.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, shared.clone()])?;
        ensure!(
            found.contains(&shared),
            "#[path=\"…\"] redirects the gated declaration just as #[path = \"…\"] does"
        );
        Ok(())
    }

    /// A `#[path]` outside every inline module block is relative to the
    /// directory holding the source file, not to the module directory.
    #[test]
    fn a_top_level_path_attribute_resolves_beside_its_source_file() -> Result<()> {
        let tree = Tree::new("top-level-path")?;
        let root = tree.write("lib.rs", "mod dispatch;\n")?;
        let dispatch =
            tree.write("dispatch.rs", "#[cfg(test)]\n#[path = \"cases/lifecycle.rs\"]\nmod c;\n")?;
        let beside = tree.write("cases/lifecycle.rs", "fn f() { x.unwrap(); }\n")?;
        let under_module_dir =
            tree.write("dispatch/cases/lifecycle.rs", "fn g() { z.unwrap(); }\n")?;

        let found =
            test_only_source_files(&[root, dispatch, beside.clone(), under_module_dir.clone()])?;
        ensure!(found.contains(&beside), "the redirect is relative to dispatch.rs's own directory");
        ensure!(
            !found.contains(&under_module_dir),
            "dispatch/cases/lifecycle.rs is not what that attribute names"
        );
        Ok(())
    }

    #[test]
    fn feature_gated_child_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("feature-gated")?;
        let root = tree.write("lib.rs", "#[cfg(any(test, feature = \"extra\"))]\nmod census;\n")?;
        let census = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, census.clone()])?;
        ensure!(
            !found.contains(&census),
            "a feature-gated module is compiled into production when the feature is on; found {found:?}"
        );
        Ok(())
    }

    /// `all(test, …)` cannot hold outside a test build, so the module it
    /// guards is test-only. Carried over from #16251's `is_cfg_test_module_file`,
    /// which read this correctly where this module did not, before that
    /// resolver was removed as duplicate authority. The live instance is
    /// `crates/perl-lsp-rs/src/runtime/mod.rs:46`.
    #[test]
    fn a_conjunction_requiring_test_gates_the_module_it_guards() -> Result<()> {
        let tree = Tree::new("all-test")?;
        let root = tree.write(
            "lib.rs",
            "#[cfg(all(test, feature = \"workspace\"))]\nmod scan_gate_observation;\n",
        )?;
        let child = tree.write("scan_gate_observation.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, child.clone()])?;
        ensure!(
            found.contains(&child),
            "all(test, …) requires test, so no production build compiles this; found {found:?}"
        );
        Ok(())
    }

    /// The opposite direction, so the conjunction rule cannot be satisfied by
    /// matching the word `test` anywhere in a predicate.
    #[test]
    fn a_disjunction_offering_test_does_not_gate_the_module() -> Result<()> {
        let tree = Tree::new("any-test")?;
        let root =
            tree.write("lib.rs", "#[cfg(any(test, feature = \"workspace\"))]\nmod census;\n")?;
        let child = tree.write("census.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, child.clone()])?;
        ensure!(
            !found.contains(&child),
            "any(test, feature) holds with the feature alone, so this is production; found {found:?}"
        );
        Ok(())
    }

    /// A negation naming `test` is the opposite claim, and a conjunct that
    /// merely contains one is not one.
    #[test]
    fn a_negated_or_nested_predicate_is_read_for_what_it_means() -> Result<()> {
        ensure!(predicate_is_test_only("test"), "the bare predicate gates");
        ensure!(
            predicate_is_test_only("all(feature = \"a\", all(test, feature = \"b\"))"),
            "a conjunction nested inside a conjunction still requires test"
        );
        ensure!(!predicate_is_test_only("not(test)"), "not(test) is production only");
        ensure!(
            !predicate_is_test_only("all(not(test), feature = \"a\")"),
            "a conjunct that negates test does not gate"
        );
        ensure!(
            !predicate_is_test_only("all(feature = \"tested\")"),
            "a feature whose name contains test is not the test predicate"
        );
        ensure!(
            !predicate_is_test_only("any(all(test, feature = \"a\"), feature = \"b\")"),
            "a disjunction is satisfiable by its other arm"
        );
        Ok(())
    }

    /// A comma inside a string literal does not start a new conjunct.
    #[test]
    fn a_comma_inside_a_feature_name_does_not_split_the_predicate() -> Result<()> {
        ensure!(
            predicate_is_test_only("all(test, feature = \"a,b\")"),
            "the literal carries the comma; the conjunction is still two arms"
        );
        ensure!(
            !predicate_is_test_only("all(feature = \"a,test\")"),
            "a comma inside a literal must not manufacture a bare test conjunct"
        );
        Ok(())
    }

    /// The near-miss control. The first version of this change reused the
    /// panic-test sweep, which takes every `mod` after a file's first
    /// `#[cfg(test)]`, and hid two real `unsafe impl` blocks in
    /// `runtime/workspace.rs`.
    #[test]
    fn an_ungated_sibling_after_a_cfg_test_boundary_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("boundary")?;
        let root = tree.write(
            "lib.rs",
            "#[cfg(all(test, feature = \"workspace\"))]\nmod harness;\n\nmod workspace;\n",
        )?;
        let harness = tree.write("harness.rs", "fn f() {}\n")?;
        let workspace = tree.write("workspace.rs", "unsafe impl Send for X {}\n")?;

        let found = test_only_source_files(&[root, harness, workspace.clone()])?;
        ensure!(
            !found.contains(&workspace),
            "an ungated declaration below a gated one is still production code"
        );
        Ok(())
    }

    #[test]
    fn only_the_declaration_the_attribute_guards_is_taken() -> Result<()> {
        let tree = Tree::new("guarded")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod gated;\nmod ungated;\n")?;
        let gated = tree.write("gated.rs", "fn f() {}\n")?;
        let ungated = tree.write("ungated.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[root, gated.clone(), ungated.clone()])?;
        ensure!(found.contains(&gated));
        ensure!(
            !found.contains(&ungated),
            "an attribute guards one item, not the rest of the file"
        );
        Ok(())
    }

    #[test]
    fn path_attribute_and_inline_module_nesting_are_resolved() -> Result<()> {
        let tree = Tree::new("path-inline")?;
        let root = tree.write(
            "lib.rs",
            "mod outer {\n    #[cfg(test)]\n    #[path = \"redirected.rs\"]\n    mod inner;\n}\n",
        )?;
        let redirected = tree.write("outer/redirected.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, redirected.clone()])?;
        ensure!(found.contains(&redirected), "the nested redirect resolves; found {found:?}");
        Ok(())
    }

    /// A `#[cfg(test)]` on an inline module block gates every file that block
    /// declares. The first version pushed the block onto the inline stack and
    /// reset `gated` in the same step, so `helper` below came out as an ungated
    /// edge and the production closure reached a file the compiler builds only
    /// under `cfg(test)`.
    #[test]
    fn a_gated_inline_module_gates_the_files_it_declares() -> Result<()> {
        let tree = Tree::new("gated-inline")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod tests {\n    mod helper;\n}\n")?;
        let helper = tree.write("tests/helper.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, helper.clone()])?;
        ensure!(
            found.contains(&helper),
            "a file declared inside a gated inline module is test-only; found {found:?}"
        );
        Ok(())
    }

    /// The companion: the gate must not leak back out of the block it opened.
    #[test]
    fn a_declaration_after_a_gated_inline_block_closes_is_not_gated() -> Result<()> {
        let tree = Tree::new("gate-scope")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\nmod tests {\n    mod helper;\n}\n\nmod live;\n")?;
        let helper = tree.write("tests/helper.rs", "fn f() {}\n")?;
        let live = tree.write("live.rs", "fn g() { y.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, helper.clone(), live.clone()])?;
        ensure!(found.contains(&helper));
        ensure!(!found.contains(&live), "the gate ended with the block that opened it");
        Ok(())
    }

    /// The exclusion path's one genuinely dangerous asymmetry: an *unread*
    /// production edge paired with a *read* gated one. `include!` is the form
    /// that actually occurs here -- `perl-parser-core`'s parser is assembled
    /// from thirteen of them -- so a literal `include!` is read as a production
    /// edge. Without that, `shared.rs` below is absent from the production
    /// closure and the gated alias takes a file the compiler builds into
    /// production straight out of scope.
    #[test]
    fn a_file_reached_only_through_an_include_stays_in_production_scope() -> Result<()> {
        let tree = Tree::new("include-prod")?;
        let root = tree.write(
            "lib.rs",
            "mod engine;\n#[cfg(test)]\n#[path = \"engine/shared.rs\"]\nmod under_test;\n",
        )?;
        let engine = tree.write("engine/mod.rs", "include!(\"shared.rs\");\n")?;
        let shared = tree.write("engine/shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;

        let found = test_only_source_files(&[root, engine, shared.clone()])?;
        ensure!(
            !found.contains(&shared),
            "an include! reaches it in production, so a gated alias cannot exclude it"
        );
        Ok(())
    }

    /// The same file named two ways: production reaches it through an
    /// `include!` spelled with a `..` hop, and a gated `#[path]` alias names
    /// it directly. Production has to win that tie, which it can only do if
    /// both spellings reduce to one identity.
    #[test]
    fn an_include_spelled_with_a_parent_hop_still_overrides_a_gated_alias() -> Result<()> {
        let tree = Tree::new("include-altspelling")?;
        let root = tree.write(
            "lib.rs",
            "mod engine;\n#[cfg(test)]\n#[path = \"engine/shared.rs\"]\nmod under_test;\n",
        )?;
        let engine = tree.write("engine/mod.rs", "include!(\"live/../shared.rs\");\n")?;
        let shared = tree.write("engine/shared.rs", "fn f() { x.expect(\"boom\"); }\n")?;
        tree.write("engine/live/placeholder.rs", "// keeps `live/` on disk\n")?;

        let found = test_only_source_files(&[root, engine, shared.clone()])?;
        ensure!(
            !found.contains(&shared),
            "`live/../shared.rs` and `shared.rs` are one file; the production include must override the gated alias"
        );
        Ok(())
    }

    /// The other direction of the `include!` rule, and the one the production
    /// edge above would otherwise swallow: a splice is *gated* exactly as a
    /// declaration is. Reading `#[cfg(test)] include!("cases.rs")` as
    /// production seeds the production closure with test-only source, and
    /// rule (3) then hands the whole spliced file back as in-scope forever.
    #[test]
    fn a_gated_include_splices_test_only_source() -> Result<()> {
        let tree = Tree::new("include-gated")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\ninclude!(\"cases.rs\");\n")?;
        let cases = tree.write("cases.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, cases.clone()])?;
        ensure!(
            found.contains(&cases),
            "a `#[cfg(test)]` include! reaches test source only; found {found:?}"
        );
        Ok(())
    }

    /// The same rule one level up. Here the splice's own line carries no
    /// attribute at all -- the gate belongs to the inline module holding it --
    /// so only the enclosing-block check can see it, which is the half a
    /// same-line test would miss.
    #[test]
    fn an_include_inside_a_gated_inline_module_is_gated_too() -> Result<()> {
        let tree = Tree::new("include-inline-gated")?;
        let root =
            tree.write("lib.rs", "#[cfg(test)]\nmod harness {\n    include!(\"cases.rs\");\n}\n")?;
        let cases = tree.write("cases.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, cases.clone()])?;
        ensure!(
            found.contains(&cases),
            "the enclosing `#[cfg(test)] mod` gates everything it splices; found {found:?}"
        );
        Ok(())
    }

    /// A computed `include!` names generated source outside the scanned tree,
    /// so it resolves to nothing rather than to a wrong path.
    #[test]
    fn a_computed_include_path_is_not_invented() -> Result<()> {
        let tree = Tree::new("include-computed")?;
        let root = tree.write(
            "lib.rs",
            "include!(concat!(env!(\"OUT_DIR\"), \"/gen.rs\"));\n#[cfg(test)]\nmod census;\n",
        )?;
        let census = tree.write("census.rs", "fn f() { x.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, census.clone()])?;
        ensure!(
            found.contains(&census),
            "an uninvented include! leaves the gate intact; found {found:?}"
        );
        Ok(())
    }

    #[test]
    fn a_non_mod_rs_file_owns_its_own_directory() -> Result<()> {
        let tree = Tree::new("owns-dir")?;
        let root = tree.write("lib.rs", "mod runtime;\n")?;
        let runtime = tree.write("runtime.rs", "#[cfg(test)]\nmod cases;\n")?;
        let cases = tree.write("runtime/cases.rs", "fn f() {}\n")?;
        let decoy = tree.write("cases.rs", "fn g() { w.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, runtime, cases.clone(), decoy.clone()])?;
        ensure!(found.contains(&cases));
        ensure!(!found.contains(&decoy), "the sibling cases.rs is a different module");
        Ok(())
    }

    #[test]
    fn a_file_with_no_declarations_yields_nothing() -> Result<()> {
        let tree = Tree::new("empty")?;
        let root = tree.write("lib.rs", "fn f() {}\n")?;

        let found = test_only_source_files(&[root])?;
        ensure!(found.is_empty(), "a file declaring nothing excludes nothing; found {found:?}");
        Ok(())
    }

    #[test]
    fn a_declaration_naming_no_file_is_not_invented() -> Result<()> {
        let tree = Tree::new("absent")?;
        let root = tree.write("lib.rs", "#[cfg(test)]\nmod nothing_here;\n")?;

        let found = test_only_source_files(&[root])?;
        ensure!(
            found.is_empty(),
            "a declaration naming no file resolves to nothing; found {found:?}"
        );
        Ok(())
    }

    /// A gated declaration in a file nothing reaches establishes nothing: the
    /// module it names stays in scope rather than being excluded on the word
    /// of an unreachable parent.
    #[test]
    fn a_gated_declaration_in_an_unreachable_file_excludes_nothing() -> Result<()> {
        let tree = Tree::new("orphan")?;
        let root = tree.write("lib.rs", "fn f() {}\n")?;
        let orphan = tree.write("orphan.rs", "#[cfg(test)]\nmod child;\n")?;
        let child = tree.write("orphan/child.rs", "fn g() { q.unwrap(); }\n")?;

        let found = test_only_source_files(&[root, orphan, child.clone()])?;
        ensure!(found.is_empty(), "unexpected exclusions: {found:?}");
        Ok(())
    }

    #[test]
    fn an_attribute_with_nested_brackets_is_one_attribute() -> Result<()> {
        let end = attribute_end("#[cfg(test)]");
        ensure!(end == Some(11), "a flat attribute ends at its own bracket, got {end:?}");
        let nested = "#[cfg(any(test, feature = \"x\"))]";
        let nested_end = attribute_end(&format!("{nested} mod m;")).map(|end| end + 1);
        ensure!(
            nested_end == Some(nested.len()),
            "the whole attribute, not a prefix ending at the first closing bracket; got {nested_end:?}"
        );
        let none = attribute_end("mod m;");
        ensure!(none.is_none(), "a line carrying no attribute ends nowhere, got {none:?}");
        Ok(())
    }

    /// A conjunction spelled across several physical lines — the live shape in
    /// `crates/perl-corpus/src/loading/` — is one attribute, not an unread
    /// fragment. This is the construct the two former readers disagreed about:
    /// the bracket scan stopped at the line break and the regexes needed
    /// `test` on the `#[cfg(all(` line, so both read the guarded module as
    /// production for different reasons.
    #[test]
    fn a_multiline_gated_declaration_is_test_only() -> Result<()> {
        let tree = Tree::new("multiline-gate")?;
        let root = tree.write(
            "lib.rs",
            "mod real;\n#[cfg(all(\n    test,\n    not(miri),\n))]\nmod census;\n",
        )?;
        let census = tree.write("census.rs", "fn f() { y.unwrap(); }\n")?;
        let real = tree.write("real.rs", "fn g() {}\n")?;

        let found = test_only_source_files(&[root, census.clone(), real.clone()])?;
        ensure!(found.contains(&census), "the multiline gate guards census.rs; found {found:?}");
        ensure!(!found.contains(&real), "the unguarded sibling stays in production scope");
        Ok(())
    }

    /// Within-file scope through the same reader: test scope opens at the
    /// attribute's own first line, not at `usize::MAX`, so a banned construct
    /// inside the guarded inline module is test code rather than a production
    /// finding waiting to happen.
    #[test]
    fn a_multiline_test_gate_opens_scope_at_its_own_line() -> Result<()> {
        let lines: Vec<String> = "fn prod() {}\n\n#[cfg(all(\n    test,\n    not(miri),\n))]\nmod tests {\n    fn it() { x.unwrap(); }\n}\n"
            .lines()
            .map(str::to_string)
            .collect();
        ensure!(
            first_cfg_test_boundary(&lines) == 3,
            "the attribute starts on line 3 and scope opens there, got {:?}",
            first_cfg_test_boundary(&lines),
        );
        Ok(())
    }

    /// The compact form: attribute, module, and body on one physical line.
    #[test]
    fn a_compact_test_module_opens_scope_on_its_own_line() -> Result<()> {
        let lines: Vec<String> =
            "#[cfg(test)] mod tests { fn it() {} }\n".lines().map(str::to_string).collect();
        ensure!(
            first_cfg_test_boundary(&lines) == 1,
            "the one-line module is test scope from line 1, got {:?}",
            first_cfg_test_boundary(&lines),
        );
        Ok(())
    }

    /// The compact body must not be enough to make any gated head a module.
    ///
    /// The boundary reader now accepts a head that ends in `}` rather than one
    /// that ends in `{`. Without this control, a reader that simply asked "is
    /// there a brace somewhere on this line" would bound the file at a
    /// test-gated `use`/`const` and truncate every production check above the
    /// real test scope — the same blind spot as the `#[cfg(test)] use` in
    /// `symbols.rs` this function's own docs cite (#16389).
    #[test]
    fn a_compact_non_module_gate_still_does_not_bound_the_file() -> Result<()> {
        for gated in ["#[cfg(test)] use std::fmt;", "#[cfg(test)] const N: usize = 1;"] {
            // The gated non-module sits above a *separately* gated module: the
            // boundary must be the module's gate on line 2, never the
            // non-module's on line 1.
            let lines: Vec<String> = [
                gated.to_string(),
                "#[cfg(test)]".to_string(),
                "mod tests {".to_string(),
                "    fn it() {}".to_string(),
                "}".to_string(),
            ]
            .into_iter()
            .collect();
            ensure!(
                first_cfg_test_boundary(&lines) == 2,
                "{gated:?} does not guard a module, so scope opens at the gate on line 2, got {:?}",
                first_cfg_test_boundary(&lines),
            );
        }
        Ok(())
    }

    /// A `mod` mentioned inside another item's block is not a module head.
    ///
    /// `module_head_name` reads the text *before* the first brace, so a `mod`
    /// that only appears once a function body has already opened cannot be
    /// mistaken for the file's own module declaration.
    #[test]
    fn a_mod_inside_another_item_is_not_a_module_head() -> Result<()> {
        let lines: Vec<String> = "#[cfg(test)] fn helper() { mod tests { fn it() {} } }\n"
            .lines()
            .map(str::to_string)
            .collect();
        ensure!(
            first_cfg_test_boundary(&lines) == usize::MAX,
            "a gated fn is not a module, got {:?}",
            first_cfg_test_boundary(&lines),
        );
        Ok(())
    }

    /// A compact module and its multi-line spelling must bound identically.
    ///
    /// The boundary is a line number and every caller treats it as "the rest of
    /// this file is test scope", so production written *after* the module is
    /// not scanned — for the multi-line spelling that has always been true, and
    /// recognising the compact form makes it true there too. That is the point
    /// of this test: the two spellings are one case, not two, so a future fix
    /// to the boundary semantics (#16523) cannot land for one and silently miss
    /// the other.
    #[test]
    fn a_compact_test_module_and_its_multiline_spelling_bound_alike() -> Result<()> {
        let compact = || {
            vec![
                "pub fn live() {}".to_string(),
                "#[cfg(test)] mod tests { fn it() {} }".to_string(),
            ]
        };
        let multiline = || {
            vec![
                "pub fn live() {}".to_string(),
                "#[cfg(test)]".to_string(),
                "mod tests {".to_string(),
                "    fn it() {}".to_string(),
                "}".to_string(),
            ]
        };
        ensure!(
            first_cfg_test_boundary(&compact()) == first_cfg_test_boundary(&multiline()),
            "the compact spelling bounded at {:?} but the multi-line spelling at {:?}",
            first_cfg_test_boundary(&compact()),
            first_cfg_test_boundary(&multiline()),
        );
        Ok(())
    }

    #[test]
    fn a_quote_inside_a_char_literal_does_not_open_a_string() -> Result<()> {
        // `class_model.rs:816` and `:1978` both spell `trim_matches('"')`. Read
        // one character at a time, the literal's `"` becomes a string opener
        // that never closes, and the projection blanks the whole rest of the
        // file — including the real `#[cfg(test)] mod tests` further down, which
        // is how a file with a test module at line 2182 reported zero spans.
        let source = concat!(
            "fn trim(value: &str) -> &str {\n",
            "    value.trim().trim_matches('\\'').trim_matches('\"')\n",
            "}\n",
            "pub fn prod() {\n",
            "    let _ = trim(\"x\");\n",
            "}\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn it() { assert!(true); }\n",
            "}\n",
        );
        ensure!(
            test_scope_lines(source) == vec![7, 8, 9, 10],
            "only the test module is test scope, got {:?}",
            test_scope_lines(source),
        );
        Ok(())
    }

    // ── inline test spans (#16523) ──────────────────────────────────────────

    /// The 1-based lines `lines` reads as inside an inline test-gated module.
    fn test_scope_lines(source: &str) -> Vec<usize> {
        let lines: Vec<String> = source.lines().map(str::to_string).collect();
        let spans = cfg_test_spans(&lines);
        (1..=lines.len()).filter(|line| line_is_test_scope(&spans, *line)).collect()
    }

    #[test]
    fn a_brace_inside_a_string_does_not_close_the_span() -> Result<()> {
        // The shape that made the first attempt at this wrong, copied from the
        // real tree: `text_document.rs` tests its own cfg-stripping helper, so
        // its test module embeds Rust source in a raw string. The `}` on its own
        // line at column 0 closes nothing, and an extent reader that believed it
        // did would hand 19 test-only `println!` sites back to the production
        // checks. Counted on the projection, the module's depth is still 1 there.
        let source = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    fn strips_gated_blocks() {\n",
            "        let adversarial = r#\"\n",
            "#[cfg(any(test, feature = \"test-fallbacks\"))]\n",
            "fn gated() {\n",
            "    let fake = \"unmatched { in a string\";\n",
            "    /* outer { /* nested } */ still gated */\n",
            "}\n",
            "fn production() {\n",
            "    on_references(params, request_id);\n",
            "}\n",
            "\"#;\n",
            "    }\n",
            "    fn after() { println!(\"test-only\"); }\n",
            "}\n",
            "pub fn prod() { println!(\"production\"); }\n",
        );
        ensure!(
            test_scope_lines(source) == vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            "the raw string's braces must not end the module early; `prod` on line 17 is production, got {:?}",
            test_scope_lines(source),
        );
        Ok(())
    }

    #[test]
    fn a_multiline_test_module_ends_at_its_closing_brace() -> Result<()> {
        ensure!(
            test_scope_lines(
                "pub fn before() {}\n#[cfg(test)]\nmod tests {\n    fn it() {}\n}\npub fn after() {}\n"
            ) == vec![2, 3, 4, 5],
            "test scope should be lines 2-5, so `after` on line 6 is production",
        );
        Ok(())
    }

    #[test]
    fn a_compact_test_module_ends_on_its_own_line() -> Result<()> {
        ensure!(
            test_scope_lines(
                "pub fn before() {}\n#[cfg(test)] mod tests { fn it() {} }\npub fn after() {}\n"
            ) == vec![2],
            "test scope should be line 2 only, so `after` on line 3 is production",
        );
        Ok(())
    }

    #[test]
    fn both_spellings_hand_production_back_after_the_module() -> Result<()> {
        // The equivalence #16521 established for the boundary, carried through to
        // the extent. The end lines differ — 2 and 5 — because the two files spell
        // the same module over different numbers of lines. What must not differ is
        // the answer to the production question each caller actually asks, and
        // that is what this pins: line-for-line, the line after each module is
        // production. A reader that resolved only the start would fail here, and
        // so would one that resolved the end from the head's own line for the
        // multi-line spelling.
        let compact = "#[cfg(test)] mod tests { fn it() {} }\npub fn prod() {}\n";
        let multiline = "#[cfg(test)]\nmod tests {\n    fn it() {}\n}\npub fn prod() {}\n";
        let compact_lines: Vec<String> = compact.lines().map(str::to_string).collect();
        let multiline_lines: Vec<String> = multiline.lines().map(str::to_string).collect();
        ensure!(
            !line_is_test_scope(&cfg_test_spans(&compact_lines), 2),
            "the compact module should hand line 2 back as production",
        );
        ensure!(
            !line_is_test_scope(&cfg_test_spans(&multiline_lines), 5),
            "the multi-line module should hand line 5 back as production",
        );
        // And the module's own lines stay test scope in both.
        ensure!(
            line_is_test_scope(&cfg_test_spans(&compact_lines), 1),
            "the compact module's own line is test scope",
        );
        ensure!(
            (1..=4).all(|line| line_is_test_scope(&cfg_test_spans(&multiline_lines), line)),
            "every line of the multi-line module is test scope",
        );
        Ok(())
    }

    #[test]
    fn a_second_test_module_is_a_second_span() -> Result<()> {
        ensure!(
            test_scope_lines(
                "#[cfg(test)]\nmod a {\n}\npub fn between() {}\n#[cfg(test)]\nmod b {\n}\n"
            ) == vec![1, 2, 3, 5, 6, 7],
            "both modules are test scope and the production between them is not",
        );
        Ok(())
    }

    #[test]
    fn a_gated_declaration_ends_at_its_own_line() -> Result<()> {
        // `mod foo;` has no body here, so its span is the declaration line. The
        // child file's scope is `test_only_source_files`' question, not this one —
        // and production after the declaration is production.
        ensure!(
            test_scope_lines("#[cfg(test)]\nmod helper;\npub fn after() {}\n") == vec![1, 2],
            "only the gate and the declaration are test scope here",
        );
        Ok(())
    }

    #[test]
    fn a_gated_block_that_never_closes_ends_at_the_last_line() -> Result<()> {
        // Truncated or hand-written input resolves to the old whole-the-rest
        // answer rather than to a guess about where the block should have ended.
        let source = "#[cfg(test)]\nmod tests {\n    fn it() {}\n";
        ensure!(
            test_scope_lines(source) == vec![1, 2, 3],
            "an unterminated block should cover every line it does have",
        );
        Ok(())
    }

    #[test]
    fn a_test_gate_on_a_non_module_item_covers_nothing() -> Result<()> {
        // The #16389 rule carries over: a `#[cfg(test)] use` near the top is not
        // a module, so it neither opens a boundary nor a span. Widening the span
        // reader to ignore the item kind would re-open that blind spot in the
        // extent reader while the boundary reader still had it covered.
        ensure!(
            test_scope_lines(
                "#[cfg(test)]\nuse std::cell::Cell;\npub fn prod() { println!(\"x\"); }\n"
            )
            .is_empty(),
            "a gated `use` is not a test-scope span",
        );
        Ok(())
    }

    #[test]
    fn a_nested_close_does_not_end_the_span_early() -> Result<()> {
        // Indent is what decides, not the first `}`. A nested block inside the
        // module closes at a deeper indent and the module keeps its scope.
        ensure!(
            test_scope_lines(
                "#[cfg(test)]\nmod tests {\n    mod inner {\n    }\n}\npub fn after() {}\n"
            ) == vec![1, 2, 3, 4, 5],
            "the inner block's close must not end the outer module's span",
        );
        Ok(())
    }
}
