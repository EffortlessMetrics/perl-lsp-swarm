//! Rustdoc panic-family source observation, joined to the existing #13397 inventory.
//!
//! This observes documented Rust syntax, including ignored and compile-fail
//! examples. It does not claim rustdoc reachability or execution. No scalar
//! allowance or second registry is introduced. Unsupported documentation inputs
//! leave an explicit not_proven instrument rather than a complete zero.

use super::model::{
    Discovered, Entrypoint, FileRecord, Instrument, InstrumentStatus, TargetKind, Topology,
    Vocabulary,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::visit::Visit;

#[derive(Clone)]
struct DocLine {
    text: String,
    line: usize,
    column: usize,
}

struct Docs<'a> {
    source: &'a [&'a str],
    file: &'a FileRecord,
    inputs: Option<&'a InputContext<'a>>,
    lines: Vec<DocLine>,
    included: Vec<(FileRecord, Vec<DocLine>, String)>,
    errors: Vec<String>,
}

struct InputContext<'a> {
    root: &'a Path,
    package_root: &'a Path,
    source_path: &'a Path,
}

impl Docs<'_> {
    fn collect_attribute(&mut self, attr: &syn::Attribute) {
        if attr.path().is_ident("cfg_attr")
            && let syn::Meta::List(meta) = &attr.meta
            && contains_doc_payload(meta.tokens.clone())
        {
            self.errors.push("conditional doc payload is not statically observed".to_string());
            return;
        }
        if !attr.path().is_ident("doc") {
            return;
        }
        let syn::Meta::NameValue(meta) = &attr.meta else { return };
        if let syn::Expr::Macro(expression) = &meta.value {
            match self
                .inputs
                .ok_or_else(|| "include dependency context unavailable".to_string())
                .and_then(|inputs| include_document(inputs, self.file, expression))
            {
                Ok(included) => self.included.push(included),
                Err(detail) => self.errors.push(detail),
            }
            return;
        }
        let syn::Expr::Lit(lit) = &meta.value else {
            self.errors.push("computed doc attribute is not statically observed".to_string());
            return;
        };
        let syn::Lit::Str(value) = &lit.lit else {
            self.errors.push("non-string doc attribute is not statically observed".to_string());
            return;
        };
        let start = attr.span().start().line;
        let Some(first) = self.source.get(start.saturating_sub(1)) else {
            self.errors.push("doc attribute lacks a source location".to_string());
            return;
        };
        let first = first.trim_start();
        let block = first.starts_with("/**") || first.starts_with("/*!");
        if !block && !first.starts_with("///") && !first.starts_with("//!") {
            // Escapes in a literal #[doc = "..."] do not give independent
            // physical line coordinates. Do not invent source identities.
            self.errors.push("literal doc attribute lacks comment-line coordinates".to_string());
            return;
        }
        let content = value.value();
        for (offset, line) in content.lines().enumerate() {
            let physical = start + offset;
            let text =
                if block { line.trim_start().strip_prefix('*').unwrap_or(line) } else { line };
            let Some(raw) = self.source.get(physical.saturating_sub(1)) else {
                self.errors.push("doc line extends outside source".to_string());
                continue;
            };
            let Some(column) = raw.find(text) else {
                self.errors.push("doc line could not be bound to source bytes".to_string());
                continue;
            };
            self.lines.push(DocLine { text: text.to_string(), line: physical, column });
        }
    }
}

impl Docs<'_> {
    // Rustdoc comments/attributes inside macro definitions remain physical
    // source inputs. Observe literal payloads without expanding a macro;
    // computed payloads retain the same explicit NOT_PROVEN disposition.
    fn collect_token_attributes(&mut self, stream: proc_macro2::TokenStream) {
        let tokens: Vec<_> = stream.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            let proc_macro2::TokenTree::Group(group) = token else { continue };
            let inner = index > 1
                && matches!(&tokens[index - 1], proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '!')
                && matches!(&tokens[index - 2], proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#');
            let outer = index > 0
                && matches!(&tokens[index - 1], proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#');
            if group.delimiter() == proc_macro2::Delimiter::Bracket && (inner || outer) {
                let start = index - if inner { 2 } else { 1 };
                let attribute: proc_macro2::TokenStream =
                    tokens[start..=index].iter().cloned().collect();
                let parsed = if inner {
                    syn::Attribute::parse_inner.parse2(attribute)
                } else {
                    syn::Attribute::parse_outer.parse2(attribute)
                };
                match parsed {
                    Ok(attributes) => {
                        for attr in attributes {
                            self.collect_attribute(&attr);
                        }
                    }
                    Err(err) if contains_doc_payload(group.stream()) => {
                        self.errors
                            .push(format!("doc payload in macro token tree is not parsed: {err}"));
                    }
                    Err(_) => {}
                }
            } else {
                self.collect_token_attributes(group.stream());
            }
        }
    }
}

impl<'ast> Visit<'ast> for Docs<'_> {
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        self.collect_attribute(attr);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.collect_token_attributes(mac.tokens.clone());
    }
}

/// Close a literal include dependency over exact UTF-8 bytes. This does not
/// expand macros, follow symlinks, or permit cross-package/workspace inputs.
fn include_document(
    inputs: &InputContext<'_>,
    file: &FileRecord,
    expression: &syn::ExprMacro,
) -> Result<(FileRecord, Vec<DocLine>, String), String> {
    const MAX_INCLUDE_BYTES: u64 = 256 * 1024;
    if file.path.contains(':') {
        return Err("doc include consumer path is not portable".to_string());
    }
    if !expression.mac.path.is_ident("include_str") {
        return Err("computed doc macro is not statically observed".to_string());
    }
    let literal = syn::parse2::<syn::LitStr>(expression.mac.tokens.clone())
        .map_err(|_| "doc include_str requires exactly one literal relative path".to_string())?;
    let value = literal.value();
    let relative = Path::new(&value);
    if value.is_empty()
        || relative.is_absolute()
        || relative.components().any(|part| {
            matches!(part, std::path::Component::Prefix(_) | std::path::Component::RootDir)
        })
        || value.contains(':')
        || value.contains('\\')
    {
        return Err("doc include_str path must be literal and portable relative".to_string());
    }
    let root = inputs.root.canonicalize().map_err(|err| err.to_string())?;
    let package = inputs.package_root.canonicalize().map_err(|err| err.to_string())?;
    let source = inputs.source_path.canonicalize().map_err(|err| err.to_string())?;
    if !package.starts_with(&root) || !source.starts_with(&package) {
        return Err("doc include source escapes its metadata-owned package".to_string());
    }
    let mut path = source
        .parent()
        .ok_or_else(|| "doc include source parent missing".to_string())?
        .to_path_buf();
    for part in relative.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if path == package || !path.pop() {
                    return Err("doc include escapes metadata-owned package".to_string());
                }
            }
            std::path::Component::Normal(name) => {
                path.push(name);
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|err| format!("doc include {}: {err}", path.display()))?;
                if metadata.file_type().is_symlink() {
                    return Err("doc include symlink is unsupported".to_string());
                }
            }
            _ => return Err("doc include has an unsupported path component".to_string()),
        }
        if !path.starts_with(&package) {
            return Err("doc include escapes metadata-owned package".to_string());
        }
    }
    let canonical = path.canonicalize().map_err(|err| err.to_string())?;
    if !canonical.starts_with(&package) || !canonical.starts_with(&root) {
        return Err("doc include dependency escapes ownership".to_string());
    }
    let metadata = std::fs::metadata(&canonical).map_err(|err| err.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_INCLUDE_BYTES {
        return Err("doc include dependency is not a bounded regular file".to_string());
    }
    // take() enforces the bound even if the input changes after metadata().
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(&canonical)
        .map_err(|err| err.to_string())?
        .take(MAX_INCLUDE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| err.to_string())?;
    if bytes.len() as u64 > MAX_INCLUDE_BYTES {
        return Err("doc include exceeds byte bound".to_string());
    }
    let digest: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    let content =
        String::from_utf8(bytes).map_err(|err| format!("doc include is not UTF-8: {err}"))?;
    let mut included = file.clone();
    included.path = super::repo_relative_path(&canonical, &root)?;
    included.target_name = format!("rustdoc-include:{}", file.path);
    let lines = content
        .lines()
        .enumerate()
        .map(|(index, text)| DocLine { text: text.to_string(), line: index + 1, column: 0 })
        .collect();
    Ok((included, lines, digest))
}

fn contains_doc_payload(tokens: proc_macro2::TokenStream) -> bool {
    let tokens: Vec<_> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let proc_macro2::TokenTree::Ident(ident) = token
            && ident == "doc"
            && matches!(tokens.get(index + 1), Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == '=')
        {
            return true;
        }
        if let proc_macro2::TokenTree::Group(group) = token
            && contains_doc_payload(group.stream())
        {
            return true;
        }
    }
    false
}

fn not_proven(kind: &str, path: &str, detail: String) -> Instrument {
    Instrument {
        kind: kind.to_string(),
        subject: path.to_string(),
        status: InstrumentStatus::NotProven,
        detail,
    }
}

/// Observe all physical .rs inputs under each metadata-owned package, matching
/// the inherited test-source scanner's documented-source population. Nested
/// workspace packages keep their own owner; symlinks and unreadable inputs are
/// instrument failures. Build outputs and repository metadata are excluded.
fn source_paths(
    dir: &Path,
    package_root: &Path,
    package_roots: &BTreeSet<PathBuf>,
    out: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(dir).map_err(|err| err.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err(format!("symlink documentation input: {}", dir.display()));
    }
    if metadata.is_file() {
        if dir.extension().is_some_and(|ext| ext == "rs") {
            out.insert(dir.to_path_buf());
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!("unsupported documentation input: {}", dir.display()));
    }
    if dir != package_root && package_roots.contains(dir) {
        return Ok(());
    }
    for item in std::fs::read_dir(dir).map_err(|err| err.to_string())? {
        let item = item.map_err(|err| err.to_string())?;
        if matches!(item.file_name().to_str(), Some("target" | ".git" | "node_modules")) {
            continue;
        }
        source_paths(&item.path(), package_root, package_roots, out)?;
    }
    Ok(())
}

pub(crate) fn scan(root: &Path, topology: &Topology, vocabulary: &Vocabulary) -> Discovered {
    let mut out = Discovered {
        entrypoints: Vec::new(),
        sites: Vec::new(),
        declarations: Vec::new(),
        instruments: Vec::new(),
        covered_paths: BTreeSet::new(),
    };
    let package_roots: BTreeSet<PathBuf> = topology
        .packages
        .iter()
        .filter_map(|package| root.join(&package.manifest).parent().map(Path::to_path_buf))
        .collect();
    let mut owned = BTreeMap::new();
    for package in &topology.packages {
        let manifest = root.join(&package.manifest);
        let Some(package_root) = manifest.parent() else {
            out.instruments.push(not_proven(
                "doc_topology",
                &package.manifest,
                "package root missing".to_string(),
            ));
            continue;
        };
        let mut paths = BTreeSet::new();
        if let Err(detail) = source_paths(package_root, package_root, &package_roots, &mut paths) {
            out.instruments.push(not_proven("doc_topology", &package.manifest, detail));
        }
        for path in paths {
            owned.insert(path, (package.name.clone(), package_root.to_path_buf()));
        }
    }
    for (path, (package, package_root)) in owned {
        let relative = match super::repo_relative_path(&path, root) {
            Ok(relative) => relative,
            Err(detail) => {
                out.instruments.push(not_proven("doc_topology", &path.to_string_lossy(), detail));
                continue;
            }
        };
        let file = FileRecord {
            package,
            target_kind: TargetKind::Doctest,
            path: relative.clone(),
            target_name: "rustdoc-source-observation".to_string(),
            feature: None,
            required_features: Vec::new(),
            platform: None,
        };
        match std::fs::read_to_string(&path) {
            Ok(source) => {
                let inputs = InputContext { root, package_root: &package_root, source_path: &path };
                let mut scanned =
                    scan_source_with_inputs(&file, vocabulary, &source, Some(&inputs));
                if !scanned
                    .instruments
                    .iter()
                    .any(|item| item.status == InstrumentStatus::NotProven)
                {
                    // Documentation coverage cannot retire an unscanned unit-
                    // test identity in the same physical file.
                    out.covered_paths.insert(format!("rustdoc:{relative}"));
                }
                out.entrypoints.append(&mut scanned.entrypoints);
                out.sites.append(&mut scanned.sites);
                out.declarations.append(&mut scanned.declarations);
                out.instruments.append(&mut scanned.instruments);
                out.covered_paths.append(&mut scanned.covered_paths);
            }
            Err(err) => out.instruments.push(not_proven("doc_fence", &relative, err.to_string())),
        }
    }
    out
}

#[cfg(test)]
fn scan_source(file: &FileRecord, vocabulary: &Vocabulary, source: &str) -> Discovered {
    scan_source_with_inputs(file, vocabulary, source, None)
}

fn scan_source_with_inputs(
    file: &FileRecord,
    vocabulary: &Vocabulary,
    source: &str,
    inputs: Option<&InputContext<'_>>,
) -> Discovered {
    let mut out = Discovered {
        entrypoints: Vec::new(),
        sites: Vec::new(),
        declarations: Vec::new(),
        instruments: Vec::new(),
        covered_paths: BTreeSet::new(),
    };
    // A failure to parse ordinary source without any potential documentation
    // remains the existing source_parse observation. Do not turn it into an
    // unrelated doc-input check failure. Potential comment/attribute bodies
    // still require successful parsing and cannot become a complete zero.
    // Include inline block/line documentation. Marker-looking literals may
    // cause an extra parse, but a successful AST read will exclude them.
    let has_doc_comment =
        ["///", "//!", "/**", "/*!"].iter().any(|marker| source.contains(*marker));
    let compact: String = source.chars().filter(|ch| !ch.is_whitespace()).collect();
    if !has_doc_comment && !compact.contains("doc=") {
        if let Err(err) = syn::parse_file(source) {
            // This blocks retirement of a previously documented identity,
            // while retaining the ordinary source observation/check contract.
            out.instruments.push(not_proven("doc_source_parse", &file.path, err.to_string()));
        }
        return out;
    }
    let parsed = match syn::parse_file(source) {
        Ok(parsed) => parsed,
        Err(err) => {
            out.instruments.push(not_proven("doc_fence", &file.path, err.to_string()));
            return out;
        }
    };
    let lines: Vec<&str> = source.lines().collect();
    let mut docs = Docs {
        source: &lines,
        file,
        inputs,
        lines: Vec::new(),
        included: Vec::new(),
        errors: Vec::new(),
    };
    docs.visit_file(&parsed);
    for detail in docs.errors {
        out.instruments.push(not_proven("doc_fence", &file.path, detail));
    }
    docs.lines.sort_by_key(|line| line.line);
    observe_lines(file, vocabulary, docs.lines, &mut out);
    for (included, lines, digest) in docs.included {
        out.instruments.push(Instrument {
            kind: "doc_include".to_string(),
            subject: included.path.clone(),
            status: InstrumentStatus::Ok,
            detail: serde_json::json!({"sha256": digest, "consumer": file.path}).to_string(),
        });
        let start = out.instruments.len();
        observe_lines(&included, vocabulary, lines, &mut out);
        if !out.instruments[start..].iter().any(|item| item.status == InstrumentStatus::NotProven) {
            out.covered_paths.insert(format!("rustdoc-include:{}:{}", included.path, file.path));
        } else {
            out.instruments.push(not_proven(
                "doc_include_binding",
                &format!("rustdoc-include:{}:{}", included.path, file.path),
                "included document was not successfully observed for this consumer".to_string(),
            ));
        }
    }
    out
}

fn observe_lines(
    file: &FileRecord,
    vocabulary: &Vocabulary,
    lines: Vec<DocLine>,
    out: &mut Discovered,
) {
    let mut active: Option<(u8, usize, usize, bool)> = None;
    let mut body = Vec::<DocLine>::new();
    let mut prior = 0usize;
    let mut ordinal = 0usize;
    for mut line in lines {
        if active.is_some() && line.line > prior + 1 {
            out.instruments.push(not_proven(
                "doc_fence",
                &file.path,
                "fence crosses separate documentation blocks".to_string(),
            ));
            active = None;
            body.clear();
        }
        prior = line.line;
        let trimmed = line.text.trim_start();
        if active.is_none() && !trimmed.is_empty() && unsupported_markdown_container(&line.text) {
            out.instruments.push(not_proven(
                "doc_fence",
                &file.path,
                format!("unsupported Markdown code container at {}", line.line),
            ));
            continue;
        }
        if let Some((marker, width, info)) = fence(trimmed) {
            if let Some((open_marker, open_width, start, rust)) = active {
                if marker == open_marker && width >= open_width && info.trim().is_empty() {
                    if rust {
                        ordinal += 1;
                        observe_body(file, vocabulary, start, ordinal, &body, out);
                    }
                    active = None;
                    body.clear();
                    continue;
                }
            } else {
                let rust = match is_rust_fence(info) {
                    Ok(rust) => rust,
                    Err(detail) => {
                        out.instruments.push(not_proven(
                            "doc_fence",
                            &file.path,
                            format!("fence at {}: {detail}", line.line),
                        ));
                        false
                    }
                };
                active = Some((marker, width, line.line, rust));
                continue;
            }
        }
        if active.is_some_and(|(_, _, _, rust)| rust) {
            // rustdoc hidden lines are code. Escaped ## keeps one literal #.
            let whitespace = line.text.len() - line.text.trim_start().len();
            let trimmed = line.text.trim_start();
            if let Some(rest) = trimmed.strip_prefix("# ") {
                line.column += whitespace + 2;
                line.text = rest.to_string();
            } else if trimmed == "#" {
                line.text.clear();
            } else if let Some(rest) = trimmed.strip_prefix("##") {
                line.column += whitespace + 1;
                line.text = format!("#{rest}");
            }
            body.push(line);
        }
    }
    if active.is_some() {
        out.instruments.push(not_proven(
            "doc_fence",
            &file.path,
            "unclosed documentation fence".to_string(),
        ));
    }
}

fn fence(text: &str) -> Option<(u8, usize, &str)> {
    let marker = *text.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let width = text.bytes().take_while(|byte| *byte == marker).count();
    (width >= 3).then(|| (marker, width, &text[width..]))
}

fn unsupported_markdown_container(text: &str) -> bool {
    let trimmed = text.trim_start();
    let indentation = text.len() - trimmed.len();
    // A full CommonMark/rustdoc container parser is not provided here. In
    // particular, quote/list-prefixed fences and indented code must not be
    // authenticated as covered prose merely because fence() cannot see them.
    indentation >= 4
        || text.starts_with('\t')
        || trimmed.starts_with('>')
        || ((trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
            && (trimmed.contains("```") || trimmed.contains("~~~")))
}

/// Bounded ordered Rust 1.95 LangString selection for supported unquoted tags.
/// Class/quoted/comment-adjacency syntax remains explicitly unsupported;
/// context-dependent aliases/error codes are not given an invented profile.
fn is_rust_fence(info: &str) -> Result<bool, String> {
    let mut attributes = String::new();
    let mut rest = info;
    while let Some(open) = rest.find('(') {
        attributes.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else {
            rest = "";
            break;
        };
        let suffix = &after[close + 1..];
        if open > 0
            && !rest.as_bytes()[open - 1].is_ascii_whitespace()
            && !matches!(rest.as_bytes()[open - 1], b',')
            && suffix
                .as_bytes()
                .first()
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b',')
        {
            return Err("adjacent rustdoc fence comment grammar is unsupported".to_string());
        }
        rest = &after[close + 1..];
    }
    attributes.push_str(rest);
    // rustdoc class-only brace forms can be Rust without a literal rust
    // attribute. Until the class/custom grammar is implemented, reject the
    // observation explicitly instead of misclassifying it as foreign text.
    if attributes.contains('{') || attributes.contains('}') {
        return Err("rustdoc class/custom attribute grammar is unsupported".to_string());
    }
    if attributes.contains('"') || attributes.contains('\'') {
        return Err("quoted rustdoc language grammar is unsupported".to_string());
    }
    // The pinned TagIterator's parenthesis skipper stops at the first ')'.
    // A remaining unmatched closer sets is_error and excludes Rust.
    if attributes.contains(')') {
        return Ok(false);
    }
    let tokens: Vec<_> =
        attributes.split([',', ' ', '\t', '\r', '\n']).filter(|value| !value.is_empty()).collect();
    // Rust 1.95's LangString parser gives custom precedence over rust.
    if tokens.contains(&"custom") {
        return Ok(false);
    }
    if tokens.iter().any(|attribute| {
        attribute
            .strip_prefix("edition")
            .is_some_and(|year| !matches!(year, "2015" | "2018" | "2021" | "2024"))
    }) {
        // The pinned parser keeps malformed edition-prefixed forms in Rust
        // scope; they must never become an excluded foreign clean zero.
        return Err("unrecognized rustdoc edition selector".to_string());
    }
    if tokens
        .iter()
        .any(|attribute| matches!(*attribute, "rust2015" | "rust2018" | "rust2021" | "rust2024"))
    {
        return Err("rust-edition alias depends on unbound rustdoc ExtraInfo".to_string());
    }
    if tokens.iter().any(|attribute| {
        !attribute.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    }) {
        return Err("unsupported rustdoc tag punctuation".to_string());
    }
    // E#### tags have a caller-controlled error-code branch. Evaluate both
    // contexts; disagreement is NOT_PROVEN, not an assumed native setting.
    let classify = |allow_error_code_check: bool| {
        let mut seen_rust_tags = false;
        let mut seen_other_tags = false;
        for attribute in &tokens {
            if *attribute == "rust" {
                seen_rust_tags = true;
            } else if matches!(*attribute, "should_panic" | "no_run" | "ignore")
                || attribute.starts_with("ignore-")
            {
                seen_rust_tags = !seen_other_tags;
            } else if matches!(*attribute, "test_harness" | "compile_fail" | "standalone_crate") {
                seen_rust_tags = !seen_other_tags || seen_rust_tags;
            } else if attribute.starts_with("edition") {
                // Edition parsing changes neither seen flag in 1.95.
            } else if allow_error_code_check
                && attribute.len() == 5
                && attribute.starts_with('E')
                && attribute.as_bytes()[1..].iter().all(u8::is_ascii_digit)
            {
                seen_rust_tags = !seen_other_tags || seen_rust_tags;
            } else {
                seen_other_tags = true;
            }
        }
        !seen_other_tags || seen_rust_tags
    };
    let plain = classify(false);
    let checked = classify(true);
    if plain != checked {
        return Err("Rust selection depends on unbound error-code checking".to_string());
    }
    Ok(plain)
}

fn observe_body(
    file: &FileRecord,
    vocabulary: &Vocabulary,
    start: usize,
    ordinal: usize,
    body: &[DocLine],
    out: &mut Discovered,
) {
    let code = format!(
        "{{\n{}\n}}",
        body.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join("\n")
    );
    let mut observed = match super::discover::scan_documented_block(file, vocabulary, &code) {
        Ok(observed) => observed,
        Err(detail) => {
            out.instruments.push(not_proven(
                "doc_fence",
                &file.path,
                format!("fence at {start}: {detail}"),
            ));
            return;
        }
    };
    let entry = if let Some(consumer) = file.target_name.strip_prefix("rustdoc-include:") {
        format!("rustdoc-fence-{ordinal}:include:{consumer}")
    } else {
        format!("rustdoc-fence-{ordinal}")
    };
    out.entrypoints.push(Entrypoint {
        package: file.package.clone(),
        target_kind: TargetKind::Doctest,
        path: file.path.clone(),
        name: entry.clone(),
        feature: None,
        platform: None,
    });
    for site in &mut observed.sites {
        let Some(line) = site.line.checked_sub(2).and_then(|index| body.get(index)) else {
            out.instruments.push(not_proven(
                "doc_fence",
                &file.path,
                format!("fence at {start} has an unmapped site"),
            ));
            return;
        };
        site.line = line.line;
        site.column += line.column;
        site.entrypoint = format!("{entry}:{}", site.entrypoint);
        // The helper starts with no parent-source covering. Preserve only
        // example-local covering, rebinding its wrapper coordinate to source.
        if let Some(identity) = &site.covering_declaration {
            let prefix = format!("{}:", file.path);
            let rebound = identity.strip_prefix(&prefix).and_then(|rest| {
                let (wrapper_line, suffix) = rest.split_once(':')?;
                let index = wrapper_line.parse::<usize>().ok()?.checked_sub(2)?;
                let original = body.get(index)?;
                Some(format!("{}:{}:{suffix}", file.path, original.line))
            });
            let Some(rebound) = rebound else {
                out.instruments.push(not_proven(
                    "doc_fence",
                    &file.path,
                    format!("fence at {start} has an unmapped covering declaration"),
                ));
                return;
            };
            site.covering_declaration = Some(rebound);
        }
    }
    for declaration in &mut observed.declarations {
        let Some(line) = declaration.line.checked_sub(2).and_then(|index| body.get(index)) else {
            out.instruments.push(not_proven(
                "doc_fence",
                &file.path,
                format!("fence at {start} has an unmapped declaration"),
            ));
            return;
        };
        declaration.line = line.line;
        declaration.entrypoint = format!("{entry}:{}", declaration.entrypoint);
    }
    for mut instrument in observed.instruments {
        instrument.kind = "doc_fence".to_string();
        instrument.detail = format!("fence at {start}: {}", instrument.detail);
        out.instruments.push(instrument);
    }
    out.sites.append(&mut observed.sites);
    out.declarations.append(&mut observed.declarations);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> FileRecord {
        FileRecord {
            package: "demo".to_string(),
            target_kind: TargetKind::Doctest,
            path: "crates/demo/src/lib.rs".to_string(),
            target_name: "docs".to_string(),
            feature: None,
            required_features: Vec::new(),
            platform: None,
        }
    }
    fn vocabulary() -> Vocabulary {
        Vocabulary {
            lints: BTreeSet::from(["clippy::unwrap_used".to_string(), "clippy::panic".to_string()]),
            method_families: BTreeSet::from(["unwrap"]),
            macro_families: BTreeSet::from(["panic!"]),
            instruments: Vec::new(),
        }
    }

    #[test]
    fn inherited_fence_grammar_distinguishes_rust_and_foreign_attributes() {
        for info in [
            "",
            "rust,ignore",
            "rust no_run",
            "rust,text",
            "text,rust",
            "ignore (needs a, b)",
            "(only a comment)",
            "edition2024",
            "compile_fail,E0277",
            "ignore,text",
            "no_run,text",
            "should_panic,text",
            "rust,text,compile_fail",
        ] {
            assert_eq!(is_rust_fence(info), Ok(true), "{info}");
        }
        for info in [
            "text",
            "json",
            "Rust",
            "rustc",
            "custom,rust",
            "rust,custom",
            "text (why)",
            "ignore (outer (inner) text)",
            "text,ignore",
            "text,no_run",
            "text,should_panic",
            "text,compile_fail,E0277",
            "rust,text,ignore",
            "text,edition2024",
        ] {
            assert_eq!(is_rust_fence(info), Ok(false), "{info}");
        }
        for info in [
            "{.example}",
            "{class=example}",
            "rust,{.example}",
            "custom,{.example}",
            "editionword",
            "rust,editionword",
            "\"rust\"",
            "rust(no comment)text",
            "E0277",
            "E0277,text",
            "rust2015",
            "rust2018",
            "rust2021",
            "rust2024",
        ] {
            assert!(is_rust_fence(info).is_err(), "{info}");
        }
    }

    #[test]
    fn doc_debt_has_existing_identity_rows_and_exact_physical_locations() {
        let source = "/// ```rust,ignore\n/// let _ = value.unwrap();\n/// ```\n/// ```rust no_run\n/// panic!(\"known\");\n/// ```\n/// ```json\n/// panic!(\"foreign\");\n/// ```\npub fn documented() {}\n";
        let out = scan_source(&file(), &vocabulary(), source);
        assert!(out.instruments.is_empty());
        assert_eq!(out.sites.len(), 2);
        assert_eq!((out.sites[0].line, out.sites[0].column), (2, 18));
        assert_eq!(out.sites[1].line, 5);
        assert_eq!(out.sites[0].entrypoint, "rustdoc-fence-1:<rustdoc>");
        assert_eq!(out.sites[1].entrypoint, "rustdoc-fence-2:<rustdoc>");
        assert_eq!(out.sites[0].target_kind, TargetKind::Doctest);
    }

    #[test]
    fn ordered_fence_selectors_preserve_true_positive_and_foreign_controls() {
        for info in ["compile_fail,E0277", "ignore,text", "no_run,text", "rust,text,compile_fail"] {
            let source = format!("/// ```{info}\n/// value.unwrap();\n/// ```\npub fn f() {{}}\n");
            let out = scan_source(&file(), &vocabulary(), &source);
            assert!(out.instruments.is_empty(), "{info}: {:?}", out.instruments);
            assert_eq!(out.sites.len(), 1, "{info}");
        }
        for info in ["text,ignore", "text,no_run", "text,compile_fail,E0277", "rust,text,ignore"] {
            let source = format!("/// ```{info}\n/// value.unwrap();\n/// ```\npub fn f() {{}}\n");
            let out = scan_source(&file(), &vocabulary(), &source);
            assert!(out.instruments.is_empty(), "{info}: {:?}", out.instruments);
            assert!(out.sites.is_empty(), "{info}");
        }
        for info in ["E0277", "E0277,text", "rust2024"] {
            let source = format!("/// ```{info}\n/// value.unwrap();\n/// ```\npub fn f() {{}}\n");
            let out = scan_source(&file(), &vocabulary(), &source);
            assert!(
                out.instruments.iter().any(|item| item.status == InstrumentStatus::NotProven),
                "{info}"
            );
        }
    }

    #[test]
    fn hidden_lines_are_observed_but_strings_and_comments_are_not_sites() {
        let source = "//! ```\n//! # let _ = value.unwrap();\n//! let _ = \"panic!()\"; // ignored.unwrap()\n//! ```\npub fn documented() {}\n";
        let out = scan_source(&file(), &vocabulary(), source);
        assert!(out.instruments.is_empty());
        assert_eq!(out.sites.len(), 1);
        assert_eq!(out.sites[0].line, 2);
    }

    #[test]
    fn physical_doc_comments_inside_macro_tokens_are_observed_without_expansion() {
        let source = "macro_rules! documented { () => {\n/// ```rust\n/// value.unwrap();\n/// ```\npub fn f() {}\n}; }\n";
        let out = scan_source(&file(), &vocabulary(), source);
        assert!(out.instruments.is_empty(), "{:?}", out.instruments);
        assert_eq!(out.sites.len(), 1);
        assert_eq!(out.sites[0].line, 3);
        assert_eq!(out.sites[0].path, file().path);
    }

    #[test]
    fn block_comments_and_long_or_tilde_fences_keep_the_original_source_scope() {
        for source in [
            "/**\n * ````rust,text\n * let _ = value.unwrap();\n * ````\n */\npub fn documented() {}\n",
            "//! ~~~text,rust\n//! let _ = value.unwrap();\n//! ~~~\npub fn documented() {}\n",
        ] {
            let out = scan_source(&file(), &vocabulary(), source);
            assert!(out.instruments.is_empty(), "{:?}", out.instruments);
            assert_eq!(out.sites.len(), 1);
            assert_eq!(out.sites[0].family, "unwrap");
        }
    }

    #[test]
    fn unsupported_or_unclosed_input_is_not_a_complete_zero() {
        for source in [
            "/// ```rust\n/// value.unwrap();\npub fn f() {}\n",
            "/// ```rust\n/// let = ;\n/// ```\npub fn f() {}\n",
            "#[doc = include_str!(\"missing.md\")] pub fn f() {}\n",
            "macro_rules! f { () => { #[doc = concat!(\"```rust\\n\", stringify!($name))] pub fn f() {} }; }\n",
            "#[cfg_attr(feature = \"docs\", doc = include_str!(\"missing.md\"))] pub fn f() {}\n",
            "#[doc = \"```rust\\nvalue.unwrap();\\n```\"] pub fn f() {}\n",
            "/// ```{.example}\n/// let _ = value.unwrap();\n/// ```\npub fn f() {}\n",
            "/// ```{class=example}\n/// let _ = value.unwrap();\n/// ```\npub fn f() {}\n",
            "/// ```editionword\n/// let _ = value.unwrap();\n/// ```\npub fn f() {}\n",
            "/// ```\"rust\"\n/// let _ = value.unwrap();\n/// ```\npub fn f() {}\n",
            "/// ```rust(no comment)text\n/// let _ = value.unwrap();\n/// ```\npub fn f() {}\n",
            "/// > ```rust\n/// > let _ = value.unwrap();\n/// > ```\npub fn f() {}\n",
            "/// > Example:\n/// >\n/// >     let _ = value.unwrap();\npub fn f() {}\n",
            "///     let _ = value.unwrap();\npub fn f() {}\n",
            "/// - ```rust\n///   let _ = value.unwrap();\n///   ```\npub fn f() {}\n",
        ] {
            let out = scan_source(&file(), &vocabulary(), source);
            assert!(
                out.instruments
                    .iter()
                    .any(|item| item.kind == "doc_fence"
                        && item.status == InstrumentStatus::NotProven),
                "{source}"
            );
        }
    }

    #[test]
    fn doc_traversal_reports_missing_source_tree() {
        let mut paths = BTreeSet::new();
        let missing = PathBuf::from("/no-panic-debt-missing-source-tree");
        assert!(source_paths(&missing, &missing, &BTreeSet::new(), &mut paths).is_err());
        assert!(paths.is_empty());
    }

    #[test]
    fn malformed_ordinary_source_keeps_its_original_observation_class() {
        let out = scan_source(&file(), &vocabulary(), "fn not rust {{{");
        assert!(
            out.instruments.iter().all(|item| item.kind == "doc_source_parse"),
            "ordinary source failure became doc-input failure: {:?}",
            out.instruments
        );
        assert!(out.instruments.iter().any(|item| item.status == InstrumentStatus::NotProven));
        assert!(out.sites.is_empty());
    }

    #[test]
    fn literal_include_binds_dependency_bytes_and_physical_coordinates()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let package = temp.path().join("crates/demo");
        std::fs::create_dir_all(package.join("src"))?;
        let source_path = package.join("src/lib.rs");
        let source = "#![doc = include_str!(\"../README.md\")]\npub fn documented() {}\n";
        std::fs::write(&source_path, source)?;
        let document = "# Example\n\n```rust\nlet _ = value.unwrap();\n```\n";
        std::fs::write(package.join("README.md"), document)?;
        let inputs =
            InputContext { root: temp.path(), package_root: &package, source_path: &source_path };
        let out = scan_source_with_inputs(&file(), &vocabulary(), source, Some(&inputs));
        assert!(
            out.instruments.iter().all(|item| item.status == InstrumentStatus::Ok),
            "{:?}",
            out.instruments
        );
        assert_eq!(out.sites.len(), 1);
        assert_eq!(out.sites[0].path, "crates/demo/README.md");
        assert_eq!((out.sites[0].line, out.sites[0].column), (4, 14));
        assert!(
            out.sites[0].entrypoint.starts_with("rustdoc-fence-1:include:crates/demo/src/lib.rs:")
        );
        assert!(
            out.covered_paths
                .contains("rustdoc-include:crates/demo/README.md:crates/demo/src/lib.rs")
        );
        let input = out
            .instruments
            .iter()
            .find(|item| item.kind == "doc_include")
            .ok_or("missing include receipt")?;
        let receipt: serde_json::Value = serde_json::from_str(&input.detail)?;
        // Independent known-answer digest for the literal fixture bytes above.
        assert_eq!(
            receipt["sha256"],
            "72ecdfd357f4af6493fc9dd159aa3616b57944e3d7808aada457ec4c14a0f893"
        );
        std::fs::write(package.join("README.md"), document.replace("unwrap", "expect"))?;
        let changed = scan_source_with_inputs(&file(), &vocabulary(), source, Some(&inputs));
        let changed_input = changed
            .instruments
            .iter()
            .find(|item| item.kind == "doc_include")
            .ok_or("missing changed receipt")?;
        assert_ne!(input.detail, changed_input.detail);
        Ok(())
    }

    #[test]
    fn unsupported_include_dependencies_cannot_be_a_complete_zero()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let package = temp.path().join("crates/demo");
        std::fs::create_dir_all(package.join("src"))?;
        let source_path = package.join("src/lib.rs");
        std::fs::write(&source_path, "pub fn f() {}")?;
        std::fs::write(temp.path().join("outside.md"), "```rust\nvalue.unwrap();\n```\n")?;
        std::fs::write(package.join("invalid.md"), [0xff])?;
        std::fs::write(package.join("large.md"), vec![b'x'; 256 * 1024 + 1])?;
        let inputs =
            InputContext { root: temp.path(), package_root: &package, source_path: &source_path };
        for expression in [
            "include_str!(\"missing.md\")",
            "include_str!(\"../../../outside.md\")",
            "include_str!(\"/outside.md\")",
            "include_str!(\"C:/outside.md\")",
            "include_str!(concat!(\"../\", \"README.md\"))",
            "include_str!(env!(\"DOC_INPUT\"))",
            "include_str!(\"../README.md\", \"extra.md\")",
            "include_str!(\"../invalid.md\")",
            "include_str!(\"../large.md\")",
        ] {
            let source = format!("#[doc = {expression}] pub fn f() {{}}\n");
            let out = scan_source_with_inputs(&file(), &vocabulary(), &source, Some(&inputs));
            assert!(
                out.instruments
                    .iter()
                    .any(|item| item.kind == "doc_fence"
                        && item.status == InstrumentStatus::NotProven),
                "{expression}"
            );
            assert!(out.covered_paths.is_empty(), "{expression}");
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn include_symlinks_are_not_followed() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let package = temp.path().join("crates/demo");
        std::fs::create_dir_all(package.join("src"))?;
        let source_path = package.join("src/lib.rs");
        std::fs::write(&source_path, "pub fn f() {}")?;
        std::fs::write(package.join("real.md"), "```rust\nvalue.unwrap();\n```\n")?;
        std::os::unix::fs::symlink(package.join("real.md"), package.join("link.md"))?;
        let inputs =
            InputContext { root: temp.path(), package_root: &package, source_path: &source_path };
        let out = scan_source_with_inputs(
            &file(),
            &vocabulary(),
            "#[doc = include_str!(\"../link.md\")] pub fn f() {}",
            Some(&inputs),
        );
        assert!(out.instruments.iter().any(|item| item.status == InstrumentStatus::NotProven));
        assert!(out.covered_paths.is_empty());
        Ok(())
    }
}
