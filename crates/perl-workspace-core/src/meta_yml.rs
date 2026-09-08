//! Bounded `META.yml` parsing and the shared metadata parse-state model
//! (#8458).
//!
//! `META.yml` is the YAML-flavored sibling of `META.json` ([crate::dist]
//! owns the JSON path). This module adds the missing YAML metadata source
//! with an explicit state for supplied bytes: malformed, unsupported, or
//! parsed. The workspace builder owns filesystem observation: an absent file
//! emits no file record or metadata facts; an unreadable file additionally
//! emits an identified `read_failure` limitation. Parser outcomes do not claim
//! to observe either filesystem state. Malformed metadata can never become an
//! empty successful fact set.
//!
//! # Safety contract
//!
//! The parser is a **bounded YAML subset** written in std-only Rust; there is
//! no Perl/CPAN YAML subprocess and no new dependency:
//!
//! * block mappings and block sequences (indentation-based);
//! * flow collections (`[a, b]`, `{k: v}`);
//! * inline plain, single-quoted, and double-quoted scalars (double-quoted escapes
//!   support newline, tab, carriage return, quote and backslash; others refuse);
//! * `#` comments, column-zero `---` / `...` document markers (single document only).
//!
//! Indented scalar-only lines are outside the block mapping/sequence subset;
//! they refuse rather than being consumed as document markers.
//!
//! Everything outside the subset is an **explicit non-success state**, never a
//! silent fallback: anchors/aliases, YAML tags, merge keys, block scalars
//! (`|` / `>`), and multiple documents are reported as findings and refuse
//! the parse. Duplicate keys are detected rather than last-value-accepted,
//! in block mappings and flow mappings alike. Resource budgets (input bytes,
//! nesting depth, node count) fail closed: every mapping entry, sequence
//! item, collection, and flow element charges the node budget, so flat
//! documents cannot dodge it.
//!
//! Scalars deliberately keep their **source spelling** (versions like `1.5`
//! stay the string `"1.5"`). Unquoted nulls are distinct from quoted strings,
//! including `"null"`, `"~"`, and `""`; only recognized v1.4/v2 fields are normalized
//! into comparison-ready facts via the same shape [crate::dist] uses for
//! `META.json`, so the two sources stay comparable.
//!
//! # What this module does NOT do (#8458 non-goals)
//!
//! No full spec-conformance verdict (that is #7176), no `META.json` ↔
//! `META.yml` reconciliation, no Kwalitee metric, and no `Makefile.PL` /
//! `Build.PL` / `dist.ini` extraction.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::dist::{
    DistMetadataFacts, DistMetadataSource, META_V1_PHASED_REQUIRES, Prereq, RELATIONS,
};
use crate::id::{Digest, FileId};

/// Budgets bounding the parser's work. Exceeding one is a typed finding, not
/// a panic or a silent truncation.
const MAX_INPUT_BYTES: usize = 1 << 20; // 1 MiB — META.yml files are small
const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 10_000;

/// The observed `meta-spec` version, kept distinct from the parse state: a
/// file can parse cleanly while declaring an unrecognized spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaSpecVersion {
    /// `meta-spec: version: 1.x` (the classic META.yml spec family).
    V1,
    /// `meta-spec: version: 2`.
    V2,
}

/// Terminal parse state for one `META.yml` input.
///
/// `Parsed` is reachable only with a whole-file successful parse; every other
/// state carries findings and yields no facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaYmlParseState {
    /// YAML syntax outside the supported subset failed (indentation, stray
    /// tokens, empty document, malformed quoting).
    Malformed,
    /// Well-formed-looking YAML that uses an explicitly unsupported feature
    /// (anchors/aliases, tags, merge keys, block scalars, multiple
    /// documents), breaks a resource budget, or contains duplicate keys or
    /// malformed encoding.
    Unsupported,
    /// The whole document parsed under the bounded subset.
    Parsed,
}

/// The class of one parse finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetaYmlFindingKind {
    /// The same mapping key appeared twice; last-value-wins is forbidden.
    DuplicateKey,
    /// More than one YAML document was present.
    MultipleDocuments,
    /// An anchor, alias, merge key, or explicit tag was used.
    AnchorAliasOrTag,
    /// A block scalar (`|` / `>`) was used.
    BlockScalar,
    /// A nesting, node-count, or byte budget was exceeded.
    ResourceLimit,
    /// Control characters that YAML forbids inside the stream.
    MalformedEncoding,
    /// Anything else that makes the YAML unparseable (including an empty
    /// document).
    MalformedSyntax,
    /// An escape outside the supported double-quoted subset.
    UnsupportedEscape,
}

/// One bounded diagnostic. Line numbers are 1-based where known; they are
/// best-effort pointers, not source ranges (range spans are #7176 surface).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaYmlFinding {
    /// The refusal class.
    pub kind: MetaYmlFindingKind,
    /// 1-based source line when known.
    pub line: Option<usize>,
    /// Human-readable explanation of the refusal.
    pub detail: String,
}

impl MetaYmlFinding {
    fn new(kind: MetaYmlFindingKind, line: Option<usize>, detail: impl Into<String>) -> Self {
        Self { kind, line, detail: detail.into() }
    }
}

/// Outcome of one bounded `META.yml` parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaYmlOutcome {
    /// Terminal parse state; facts exist only in `Parsed`.
    pub state: MetaYmlParseState,
    /// The recognized spec version declared by `meta-spec`, if any. Kept
    /// separate from `state`: an unknown spec does not make a clean YAML
    /// document malformed, and a recognized spec does not make a broken one
    /// parse.
    pub spec_version: Option<MetaSpecVersion>,
    /// Normalized facts; `Some` only when `state == Parsed`.
    pub facts: Option<DistMetadataFacts>,
    /// Bounded diagnostics (parse refusals, unknown spec, unsupported
    /// features).
    pub findings: Vec<MetaYmlFinding>,
    /// Deterministic content fingerprint (`fnv64:<hex>`), independent of the
    /// host path.
    pub source_digest: String,
    /// Static limitations of this parser, stable across runs.
    pub limitations: Vec<String>,
}

/// Static limitations reported with every outcome.
pub const META_YML_LIMITATIONS: &[&str] = &[
    "anchors, aliases, merge keys, explicit tags, block scalars, and multi-document streams are refused, not resolved",
    "scalar values keep their source spelling; no YAML type resolution is performed",
    "double-quoted escapes outside newline, tab, carriage return, quote and backslash are refused",
    "no META spec-conformance verdict is produced (#7176 owns spec validation)",
];

/// Parse a `META.yml` under the bounded YAML subset.
#[must_use]
pub fn parse_meta_yml(file_id: FileId, content: &str) -> MetaYmlOutcome {
    let source_digest = Digest::of(content).as_str().to_string();
    let mut findings = Vec::new();

    if content.len() > MAX_INPUT_BYTES {
        findings.push(MetaYmlFinding::new(
            MetaYmlFindingKind::ResourceLimit,
            None,
            format!(
                "input is {} bytes; the bounded parser refuses above {MAX_INPUT_BYTES}",
                content.len()
            ),
        ));
        return outcome(MetaYmlParseState::Unsupported, None, None, findings, source_digest);
    }
    if content.trim().is_empty() {
        findings.push(MetaYmlFinding::new(
            MetaYmlFindingKind::MalformedSyntax,
            None,
            "document is empty",
        ));
        return outcome(MetaYmlParseState::Malformed, None, None, findings, source_digest);
    }

    for (index, line) in content.lines().enumerate() {
        if has_forbidden_control(line) {
            findings.push(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedEncoding,
                Some(index + 1),
                "raw line contains a forbidden control character",
            ));
            return outcome(MetaYmlParseState::Malformed, None, None, findings, source_digest);
        }
    }
    let mut doc = Doc::new(content);
    if let Err(finding) = scan_stream_safety(&mut doc) {
        let state = failure_state(&finding);
        findings.push(finding);
        return outcome(state, None, None, findings, source_digest);
    }

    let mut parser = Parser { lines: doc.body, pos: 0, nodes: 0, depth: 0 };
    let value = match parser.parse_block(0) {
        Ok(value) => value,
        Err(finding) => {
            let state = failure_state(&finding);
            findings.push(finding);
            return outcome(state, None, None, findings, source_digest);
        }
    };
    // Trailing content that is not blank after the first top-level node is a
    // second document fragment.
    if parser.skip_blank() != parser.lines.len() {
        findings.push(MetaYmlFinding::new(
            MetaYmlFindingKind::MultipleDocuments,
            Some(parser.lines[parser.pos].number),
            "content follows the first document; only single-document META.yml is supported",
        ));
        return outcome(MetaYmlParseState::Unsupported, None, None, findings, source_digest);
    }

    let Yaml::Map(root) = value else {
        findings.push(MetaYmlFinding::new(
            MetaYmlFindingKind::MalformedSyntax,
            None,
            "top level of a META.yml must be a mapping",
        ));
        return outcome(MetaYmlParseState::Malformed, None, None, findings, source_digest);
    };

    let spec_version = recognized_spec_version(&root, &mut findings);
    let facts = DistMetadataFacts {
        file_id,
        source: DistMetadataSource::MetaYml,
        name: root_string(&root, "name"),
        version: root_string(&root, "version"),
        summary: root_string(&root, "abstract"),
        licenses: root_licenses(&root),
        prereqs: root_prereqs(&root),
    };
    if findings.len() > 64 {
        // The finding list itself is bounded; a pathological input must not
        // grow it without limit.
        findings.truncate(64);
        findings.push(MetaYmlFinding::new(
            MetaYmlFindingKind::ResourceLimit,
            None,
            "finding list truncated at 64 entries",
        ));
    }
    outcome(MetaYmlParseState::Parsed, spec_version, Some(facts), findings, source_digest)
}

fn failure_state(finding: &MetaYmlFinding) -> MetaYmlParseState {
    match finding.kind {
        MetaYmlFindingKind::ResourceLimit
        | MetaYmlFindingKind::DuplicateKey
        | MetaYmlFindingKind::AnchorAliasOrTag
        | MetaYmlFindingKind::BlockScalar
        | MetaYmlFindingKind::UnsupportedEscape
        | MetaYmlFindingKind::MultipleDocuments => MetaYmlParseState::Unsupported,
        _ => MetaYmlParseState::Malformed,
    }
}

fn outcome(
    state: MetaYmlParseState,
    spec_version: Option<MetaSpecVersion>,
    facts: Option<DistMetadataFacts>,
    findings: Vec<MetaYmlFinding>,
    source_digest: String,
) -> MetaYmlOutcome {
    MetaYmlOutcome {
        state,
        spec_version,
        facts,
        findings,
        source_digest,
        limitations: META_YML_LIMITATIONS.iter().map(|s| (*s).to_string()).collect(),
    }
}

// ── Stream safety ─────────────────────────────────────────────────────────────

/// Pre-parse scan for encoding, document count, and feature refusals.
/// Produces the body line list with the leading document marker stripped.
fn scan_stream_safety(doc: &mut Doc) -> Result<(), MetaYmlFinding> {
    let mut body: Vec<Line> = Vec::new();
    let mut seen_first_marker = false;
    let mut seen_document_end = false;

    for line in &doc.raw {
        if has_forbidden_control(&line.text) {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedEncoding,
                Some(line.number),
                "line contains a control character YAML forbids",
            ));
        }
        let trimmed = line.text.trim();
        // Markers start at column zero and end at ASCII separation. An
        // indented marker belongs to content; a suffix like `...name` is a
        // plain scalar rather than a document marker (YAML 1.2.2, 9.1.2).
        let marker_suffix = |marker| {
            line.text
                .strip_prefix(marker)
                .filter(|rest| rest.chars().next().is_none_or(|c| matches!(c, ' ' | '\t')))
        };
        if let Some(rest) = marker_suffix("...") {
            if !rest.trim().is_empty() {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    "document end marker cannot carry inline content",
                ));
            }
            seen_document_end = true;
            continue;
        }
        if seen_document_end && !trimmed.is_empty() {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MultipleDocuments,
                Some(line.number),
                "content appears after the first document's explicit end marker",
            ));
        }
        if let Some(rest) = marker_suffix("---") {
            if !body.is_empty() || seen_first_marker {
                // A second start marker starts another document even when
                // no content follows it. Only `...` ends the first document.
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MultipleDocuments,
                    Some(line.number),
                    "a second document starts after the first; single-document META.yml only",
                ));
            }
            seen_first_marker = true;
            // Space and tab separators both admit inline document content.
            if !rest.trim().is_empty() {
                body.push(Line { number: line.number, text: rest.trim_start().to_string().into() });
            }
            continue;
        }
        body.push(line.clone());
    }

    for line in &body {
        let trimmed = line.text.trim_start();
        for marker in ["&", "*", "!"] {
            if token_present(trimmed, marker) {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::AnchorAliasOrTag,
                    Some(line.number),
                    format!("anchors/aliases/tags ('{marker}…') are refused, not resolved"),
                ));
            }
        }
        // A block-scalar indicator is either the whole line or the value of a
        // `key: |` / `key: >` mapping entry (or a bare `- |` sequence item).
        let scalar_or_map = trimmed.strip_prefix('-').map(str::trim_start).unwrap_or(trimmed);
        let value_part = split_key(scalar_or_map, line.number, false)?
            .map(|(_, rest)| rest)
            .unwrap_or_else(|| scalar_or_map.to_string());
        let value_trimmed = value_part.trim_start();
        if value_trimmed.starts_with('|') || value_trimmed.starts_with('>') {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::BlockScalar,
                Some(line.number),
                "block scalars ('|' / '>') are not supported",
            ));
        }
    }

    doc.body = body;
    Ok(())
}

/// Shared quote and escape state for punctuation scanners.
#[derive(Default)]
struct QuoteState {
    single: bool,
    double: bool,
    escaped: bool,
    single_end: bool,
    plain: bool,
    flow_depth: usize,
}

impl QuoteState {
    /// Consume one character; true means it is unquoted punctuation/content.
    /// Quotes start only at scalar admission: line start, a separated mapping
    /// colon/sequence dash, or a delimiter of an actual flow collection.
    /// Punctuation embedded in a block plain scalar never opens a new scalar.
    fn outside(&mut self, c: char, next: Option<char>) -> bool {
        if self.double {
            if self.escaped {
                self.escaped = false;
            } else if c == '\\' {
                self.escaped = true;
            } else if c == '"' {
                self.double = false;
            }
            return false;
        }
        if self.single {
            if c == '\'' {
                self.single_end = !self.single_end;
                return false;
            }
            if !self.single_end {
                return false;
            }
            self.single = false;
            self.single_end = false;
        }
        match c {
            '\'' if !self.plain => {
                self.single = true;
                self.plain = true;
                false
            }
            '"' if !self.plain => {
                self.double = true;
                self.plain = true;
                false
            }
            ':' if next.is_none_or(char::is_whitespace) => {
                self.plain = false;
                true
            }
            '[' | '{' if !self.plain => {
                self.flow_depth += 1;
                true
            }
            ']' | '}' if self.flow_depth > 0 => {
                self.flow_depth -= 1;
                self.plain = true;
                true
            }
            ',' if self.flow_depth > 0 => {
                self.plain = false;
                true
            }
            '-' if !self.plain && next.is_none_or(char::is_whitespace) => true,
            c if c.is_whitespace() => true,
            _ => {
                self.plain = true;
                true
            }
        }
    }
}

/// Detect unquoted indicators only where the shared scanner admits a scalar.
fn token_present(line: &str, marker: &str) -> bool {
    let mut quotes = QuoteState::default();
    for (i, c) in line.char_indices() {
        let scalar_start = !quotes.plain && !quotes.single && !quotes.double;
        if quotes.outside(c, line[i + c.len_utf8()..].chars().next())
            && scalar_start
            && line[i..].starts_with(marker)
        {
            return true;
        }
    }
    false
}

fn has_forbidden_control(text: &str) -> bool {
    text.chars().any(|c| c.is_control() && c != '\t' && c != '\n' && c != '\r')
}

// ── Line model ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Line<'a> {
    number: usize,
    /// Content with the line comment stripped, trailing whitespace trimmed.
    /// Leading indentation is preserved.
    text: std::borrow::Cow<'a, str>,
}

#[derive(Debug)]
struct Doc<'a> {
    raw: Vec<Line<'a>>,
    body: Vec<Line<'a>>,
}

impl<'a> Doc<'a> {
    fn new(content: &'a str) -> Self {
        let mut raw = Vec::new();
        for (idx, raw_line) in content.lines().enumerate() {
            let text = strip_comment(raw_line);
            if text.trim().is_empty() {
                continue;
            }
            raw.push(Line { number: idx + 1, text });
        }
        Self { raw, body: Vec::new() }
    }
}

/// Strip a `#` comment that is outside quotes and not part of a scalar.
fn strip_comment(line: &str) -> std::borrow::Cow<'_, str> {
    let mut quotes = QuoteState::default();
    let mut previous = None;
    for (i, c) in line.char_indices() {
        if quotes.outside(c, line[i + c.len_utf8()..].chars().next())
            && c == '#'
            && previous.is_none_or(|p: char| p.is_whitespace())
        {
            return std::borrow::Cow::Borrowed(line[..i].trim_end());
        }
        previous = Some(c);
    }
    std::borrow::Cow::Borrowed(line.trim_end())
}

// ── Block parser ─────────────────────────────────────────────────────────────

/// A YAML value in the bounded subset. Scalars keep their source spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Yaml {
    /// An unquoted YAML null, kept distinct from quoted strings like "null".
    Null,
    Scalar(String),
    Seq(Vec<Yaml>),
    /// Insertion-ordered mapping; duplicate keys are refused at insert time.
    Map(Vec<(String, Yaml)>),
}

struct Parser<'a> {
    lines: Vec<Line<'a>>,
    pos: usize,
    nodes: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    /// Parse a block node whose lines are indented at least `indent` spaces.
    fn parse_block(&mut self, indent: usize) -> Result<Yaml, MetaYmlFinding> {
        self.charge_node()?;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::ResourceLimit,
                self.peek().map(|l| l.number),
                format!("nesting deeper than {MAX_DEPTH} levels"),
            ));
        }
        let result = self.parse_block_inner(indent);
        self.depth -= 1;
        result
    }

    fn parse_block_inner(&mut self, indent: usize) -> Result<Yaml, MetaYmlFinding> {
        let Some(line) = self.peek() else {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                None,
                "expected content, found end of document",
            ));
        };
        let line_indent = indentation(&line.text);
        if line_indent == usize::MAX {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                Some(line.number),
                "tabs may not be used for indentation",
            ));
        }
        if line_indent < indent {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                Some(line.number),
                format!("expected indentation >= {indent}, found {line_indent}"),
            ));
        }
        if line.text.trim_start().starts_with("- ") || line.text.trim() == "-" {
            self.parse_seq(line_indent)
        } else {
            self.parse_map(line_indent)
        }
    }

    fn parse_seq(&mut self, indent: usize) -> Result<Yaml, MetaYmlFinding> {
        let mut items = Vec::new();
        while let Some(line) = self.peek().cloned() {
            let line_indent = indentation(&line.text);
            if line_indent == usize::MAX {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    "tabs may not be used for indentation",
                ));
            }
            if line_indent < indent {
                break;
            }
            if line_indent > indent {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    "inconsistent indentation inside a sequence",
                ));
            }
            let trimmed = line.text.trim_start().to_string();
            if !(trimmed.starts_with("- ") || trimmed == "-") {
                break;
            }
            self.pos += 1;
            let rest = trimmed.strip_prefix("-").unwrap_or("").trim_start().to_string();
            if rest.is_empty() {
                // Nested block owned by this item.
                let nested = self.parse_block(indent + 1)?;
                items.push(nested);
            } else if let Some((key, value)) = self.try_split_map_entry(&rest, line.number)? {
                // `- key: value` — an inline mapping item; later `key: value`
                // lines at the deeper indent belong to the same item.
                let mut entries = vec![(key, value)];
                let mut seen_keys: HashSet<String> = HashSet::new();
                seen_keys.insert(entries[0].0.clone());
                let item_indent = indent + (trimmed.len() - rest.len()) + 1;
                while let Some(next_indent) = self.peek().map(|l| indentation(&l.text)) {
                    if next_indent <= indent || next_indent < item_indent.saturating_sub(1) {
                        break;
                    }
                    let entry = self.parse_map(next_indent)?;
                    let Yaml::Map(m) = entry else { break };
                    // Duplicate detection must span the inline first entry and
                    // every continuation entry of the same item; the set keeps
                    // that check linear in the item size.
                    for (k, v) in m {
                        if !seen_keys.insert(k.clone()) {
                            return Err(MetaYmlFinding::new(
                                MetaYmlFindingKind::DuplicateKey,
                                Some(line.number),
                                format!("duplicate key `{k}` in a sequence mapping item"),
                            ));
                        }
                        entries.push((k, v));
                    }
                }
                self.charge_nodes(entries.len())?;
                items.push(Yaml::Map(entries));
            } else {
                items.push(self.parse_scalar_or_flow(&rest, line.number, false)?);
            }
        }
        Ok(Yaml::Seq(items))
    }

    fn parse_map(&mut self, indent: usize) -> Result<Yaml, MetaYmlFinding> {
        let mut entries: Vec<(String, Yaml)> = Vec::new();
        // Set-based duplicate detection: a linear scan per insert made wide
        // maps O(n^2) (60k entries cost seconds).
        let mut seen_keys: HashSet<String> = HashSet::new();
        while let Some(line) = self.peek().cloned() {
            let line_indent = indentation(&line.text);
            if line_indent == usize::MAX {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    "tabs may not be used for indentation",
                ));
            }
            if line_indent < indent {
                break;
            }
            if line_indent > indent {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    "inconsistent indentation inside a mapping",
                ));
            }
            let trimmed = line.text.trim_start().to_string();
            if trimmed.starts_with("- ") || trimmed == "-" {
                break;
            }
            let Some((key, rest)) = split_key(&trimmed, line.number, false)? else {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    format!("expected `key: value`, found: {trimmed:.40}"),
                ));
            };
            self.pos += 1;
            let value = if rest.is_empty() {
                // Value is the nested block (or null when the next
                // line is a sibling). Every entry charges a node — block
                // values through their own parse_block, flow values through
                // their elements — so flat maps cannot dodge the budget.
                match self.peek() {
                    Some(next) if indentation(&next.text) > indent => {
                        self.parse_block(indent + 1)?
                    }
                    _ => {
                        self.charge_node_at(line.number)?;
                        Yaml::Null
                    }
                }
            } else {
                self.parse_scalar_or_flow(&rest, line.number, false)?
            };
            if !seen_keys.insert(key.clone()) {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::DuplicateKey,
                    Some(line.number),
                    format!("duplicate key `{key}`; last-value-wins is refused"),
                ));
            }
            entries.push((key, value));
        }
        Ok(Yaml::Map(entries))
    }

    fn charge_node(&mut self) -> Result<(), MetaYmlFinding> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::ResourceLimit,
                self.peek().map(|l| l.number),
                format!("more than {MAX_NODES} YAML nodes"),
            ));
        }
        Ok(())
    }

    fn charge_nodes(&mut self, extra: usize) -> Result<(), MetaYmlFinding> {
        for _ in 0..extra {
            self.charge_node()?;
        }
        Ok(())
    }

    /// Charge one node against the node budget when the source line is known
    /// directly (mapping entries, sequence scalar items, flow elements) so a
    /// refusal points at the entry that crossed the budget.
    fn charge_node_at(&mut self, line: usize) -> Result<(), MetaYmlFinding> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::ResourceLimit,
                Some(line),
                format!("more than {MAX_NODES} YAML nodes"),
            ));
        }
        Ok(())
    }

    /// Parse an inline scalar or flow-collection value. Scalars and every
    /// flow node charge the budget, so flat documents fail closed.
    fn parse_scalar_or_flow(
        &mut self,
        text: &str,
        line: usize,
        in_flow: bool,
    ) -> Result<Yaml, MetaYmlFinding> {
        let trimmed = text.trim();
        if trimmed.starts_with('[') {
            return self.flow_seq(trimmed, line);
        }
        if trimmed.starts_with('{') {
            return self.flow_map(trimmed, line);
        }
        self.charge_node_at(line)?;
        parse_scalar_value(trimmed, line, in_flow)
    }

    /// Parse a flow sequence `[a, b, [c]]` with the same budgets: the
    /// collection and each element charge one node.
    fn flow_seq(&mut self, text: &str, line: usize) -> Result<Yaml, MetaYmlFinding> {
        let inner = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')).ok_or_else(|| {
            MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                Some(line),
                "unterminated flow sequence",
            )
        })?;
        self.charge_node_at(line)?;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::ResourceLimit,
                Some(line),
                format!("nesting deeper than {MAX_DEPTH} levels"),
            ));
        }
        let mut items = Vec::new();
        let result = (|| {
            for part in split_flow(inner, line)? {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                self.charge_node_at(line)?;
                if part.starts_with('[') {
                    items.push(self.flow_seq(part, line)?);
                } else if part.starts_with('{') {
                    items.push(self.flow_map(part, line)?);
                } else {
                    items.push(parse_scalar_value(part, line, true)?);
                }
            }
            Ok(Yaml::Seq(items))
        })();
        self.depth -= 1;
        result
    }

    /// Parse a flow map `{a: 1, b: 2}` with the same budgets as block
    /// mappings: per-node charging and a typed duplicate-key refusal instead
    /// of silent last-value-wins.
    fn flow_map(&mut self, text: &str, line: usize) -> Result<Yaml, MetaYmlFinding> {
        let inner = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')).ok_or_else(|| {
            MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                Some(line),
                "unterminated flow mapping",
            )
        })?;
        self.charge_node_at(line)?;
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.depth -= 1;
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::ResourceLimit,
                Some(line),
                format!("nesting deeper than {MAX_DEPTH} levels"),
            ));
        }
        let mut entries = Vec::new();
        let mut seen_keys: HashSet<String> = HashSet::new();
        let result = (|| {
            for part in split_flow(inner, line)? {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                let Some((key, value)) = split_key(part, line, true)? else {
                    return Err(MetaYmlFinding::new(
                        MetaYmlFindingKind::MalformedSyntax,
                        Some(line),
                        format!("flow mapping entry without ':': {part:.40}"),
                    ));
                };
                self.charge_node_at(line)?;
                if !seen_keys.insert(key.clone()) {
                    return Err(MetaYmlFinding::new(
                        MetaYmlFindingKind::DuplicateKey,
                        Some(line),
                        format!(
                            "duplicate key `{key}` in a flow mapping; last-value-wins is refused"
                        ),
                    ));
                }
                entries.push((key, self.parse_scalar_or_flow(&value, line, true)?));
            }
            Ok(Yaml::Map(entries))
        })();
        self.depth -= 1;
        result
    }

    fn try_split_map_entry(
        &mut self,
        text: &str,
        line: usize,
    ) -> Result<Option<(String, Yaml)>, MetaYmlFinding> {
        match split_key(text, line, false)? {
            Some((key, rest)) => Ok(Some((key, self.parse_scalar_or_flow(&rest, line, false)?))),
            None => Ok(None),
        }
    }

    fn peek(&self) -> Option<&Line<'a>> {
        self.lines.get(self.pos)
    }

    /// Advance past blank lines (already filtered) — kept for the trailing
    /// multi-document check.
    fn skip_blank(&mut self) -> usize {
        while self.peek().is_some() && self.peek().is_none_or(|l| l.text.trim().is_empty()) {
            self.pos += 1;
        }
        self.pos
    }
}

/// Indentation width in spaces (tabs are refused: YAML forbids them).
fn indentation(text: &str) -> usize {
    let mut width = 0;
    for ch in text.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => return usize::MAX, // handled as a syntax error by callers
            _ => break,
        }
    }
    width
}

/// Split `key: value` at the top level of a line (outside quotes/brackets).
fn split_key(
    text: &str,
    line: usize,
    in_flow: bool,
) -> Result<Option<(String, String)>, MetaYmlFinding> {
    let mut quotes = QuoteState::default();
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        if !quotes.outside(c, text[i + c.len_utf8()..].chars().next()) {
            continue;
        }
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => {
                let after = &text[i + 1..];
                if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
                    let key = unquote(text[..i].trim(), line, in_flow)?;
                    // Merge keys are refused at the shared key seam so block
                    // mappings, flow mappings, and sequence items all reject
                    // them instead of publishing a fake `<<` entry.
                    if key == "<<" {
                        return Err(MetaYmlFinding::new(
                            MetaYmlFindingKind::AnchorAliasOrTag,
                            Some(line),
                            "merge keys ('<<') are refused",
                        ));
                    }
                    return Ok(Some((key, after.trim_start().to_string())));
                }
            }
            _ => {}
        }
    }
    Ok(None)
}

fn unquote(text: &str, line: usize, in_flow: bool) -> Result<String, MetaYmlFinding> {
    let trimmed = text.trim();
    let malformed = || {
        MetaYmlFinding::new(
            MetaYmlFindingKind::MalformedSyntax,
            Some(line),
            "unterminated or malformed quoted scalar",
        )
    };
    if trimmed.starts_with('\'') {
        let inner =
            trimmed.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).ok_or_else(malformed)?;
        let mut chars = inner.chars();
        let mut out = String::new();
        while let Some(c) = chars.next() {
            if c == '\'' && chars.next() != Some('\'') {
                return Err(malformed());
            }
            out.push(c);
        }
        return Ok(out);
    }
    if trimmed.starts_with('"') {
        let inner =
            trimmed.strip_prefix('"').and_then(|s| s.strip_suffix('"')).ok_or_else(malformed)?;
        return decode_double_quoted(inner, line);
    }
    // YAML 1.2.2 section 7.3.3: indicators cannot begin plain scalars,
    // except `?`, `:`, or `-` followed by a non-space character.
    // Check at the shared decoder so block/flow values and mapping keys all
    // use this admission boundary; quoted strings above retain punctuation.
    let mut chars = trimmed.chars();
    let first = chars.next();
    let forbidden_start = first.is_some_and(|c| ",[]{}#&*!|>%@`".contains(c))
        || (first.is_some_and(|c| "?:-".contains(c))
            && chars.next().is_none_or(|c| matches!(c, ' ' | '\t')));
    let separated_colon = trimmed.char_indices().any(|(index, c)| {
        c == ':' && trimmed[index + 1..].chars().next().is_none_or(|c| matches!(c, ' ' | '\t'))
    });
    // Flow plain scalars exclude collection punctuation anywhere, unlike
    // block plain scalars. Quoted values returned before this check.
    let forbidden_flow = in_flow && trimmed.chars().any(|c| "[]{},".contains(c));
    if forbidden_start || separated_colon || forbidden_flow {
        return Err(MetaYmlFinding::new(
            MetaYmlFindingKind::MalformedSyntax,
            Some(line),
            "reserved indicator, separated colon, or flow delimiter in an unquoted scalar",
        ));
    }
    Ok(trimmed.to_string())
}

/// Resolve null before decoding quotes; quoted null-like text remains a
/// string. Other scalars retain their spelling rather than numeric coercion.
fn parse_scalar_value(text: &str, line: usize, in_flow: bool) -> Result<Yaml, MetaYmlFinding> {
    let trimmed = text.trim();
    if matches!(trimmed, "" | "~" | "null" | "Null" | "NULL") {
        return Ok(Yaml::Null);
    }
    Ok(Yaml::Scalar(unquote(trimmed, line, in_flow)?))
}

fn decode_double_quoted(text: &str, line: usize) -> Result<String, MetaYmlFinding> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(_) => {
                    return Err(MetaYmlFinding::new(
                        MetaYmlFindingKind::UnsupportedEscape,
                        Some(line),
                        "escape outside the supported double-quoted subset",
                    ));
                }
                None => {
                    return Err(MetaYmlFinding::new(
                        MetaYmlFindingKind::MalformedSyntax,
                        Some(line),
                        "trailing escape in quoted scalar",
                    ));
                }
            }
        } else if c == '"' {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                Some(line),
                "unescaped quote inside scalar",
            ));
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

impl Yaml {
    fn as_scalar(&self) -> Option<&str> {
        match self {
            Yaml::Scalar(s) => Some(s),
            _ => None,
        }
    }
}

/// Split a flow collection body on top-level commas.
fn split_flow(text: &str, line: usize) -> Result<Vec<String>, MetaYmlFinding> {
    let malformed = || {
        MetaYmlFinding::new(
            MetaYmlFindingKind::MalformedSyntax,
            Some(line),
            "flow collection has an empty entry or unbalanced delimiters",
        )
    };
    let mut parts = Vec::new();
    let mut delimiters = Vec::new();
    // The supplied text is already inside a flow collection.
    let mut quotes = QuoteState { flow_depth: 1, ..QuoteState::default() };
    let mut start = 0;
    for (i, c) in text.char_indices() {
        if !quotes.outside(c, text[i + c.len_utf8()..].chars().next()) {
            continue;
        }
        match c {
            '[' | '{' => {
                if delimiters.len() >= MAX_DEPTH {
                    return Err(MetaYmlFinding::new(
                        MetaYmlFindingKind::ResourceLimit,
                        Some(line),
                        "flow delimiters exceed the nesting budget",
                    ));
                }
                delimiters.push(c);
            }
            ']' | '}' => {
                let expected = if c == ']' { '[' } else { '{' };
                if delimiters.pop() != Some(expected) {
                    return Err(malformed());
                }
            }
            ',' if delimiters.is_empty() => {
                if text[start..i].trim().is_empty() {
                    return Err(malformed());
                }
                parts.push(text[start..i].to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !delimiters.is_empty() {
        return Err(malformed());
    }
    // Empty collections and one trailing comma are valid; a leading or
    // repeated comma already failed while scanning the separator.
    if !text[start..].trim().is_empty() {
        parts.push(text[start..].to_string());
    }
    Ok(parts)
}

// ── Fact normalization (mirrors dist::parse_meta_json) ──────────────────────

fn root_string(root: &[(String, Yaml)], key: &str) -> Option<String> {
    let value = root.iter().find(|(k, _)| k == key).map(|(_, v)| v)?;
    value.as_scalar().map(str::to_string)
}

fn root_licenses(root: &[(String, Yaml)]) -> Vec<String> {
    let Some((_, value)) = root.iter().find(|(k, _)| k == "license") else {
        return Vec::new();
    };
    match value {
        // v2: an array of license strings.
        Yaml::Seq(items) => {
            items.iter().filter_map(|v| v.as_scalar().map(str::to_string)).collect()
        }
        // v1.4: a single string.
        Yaml::Scalar(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn root_prereqs(root: &[(String, Yaml)]) -> Vec<Prereq> {
    let mut prereqs = Vec::new();
    let mut recovered_v2_entries = false;
    // v2: prereqs[phase][relation] = { module: version }.
    if let Some((_, Yaml::Map(phases))) = root.iter().find(|(k, _)| k == "prereqs") {
        for (phase, relations) in phases {
            let Yaml::Map(relations) = relations else { continue };
            for (relation, modules) in relations {
                if !RELATIONS.contains(&relation.as_str()) {
                    continue;
                }
                recovered_v2_entries |= collect_modules(modules, phase, relation, &mut prereqs);
            }
        }
    }
    // v1.4 flat fallback.
    if !recovered_v2_entries {
        for &(key, phase) in META_V1_PHASED_REQUIRES {
            if let Some((_, modules)) = root.iter().find(|(k, _)| k == key) {
                let _ = collect_modules(modules, phase, "requires", &mut prereqs);
            }
        }
        for relation in RELATIONS {
            if let Some((_, modules)) = root.iter().find(|(k, _)| k == relation) {
                let _ = collect_modules(modules, "runtime", relation, &mut prereqs);
            }
        }
    }
    prereqs.sort_by(|a, b| {
        (&a.phase, &a.relation, &a.module).cmp(&(&b.phase, &b.relation, &b.module))
    });
    prereqs
}

fn collect_modules(modules: &Yaml, phase: &str, relation: &str, out: &mut Vec<Prereq>) -> bool {
    let Yaml::Map(map) = modules else { return false };
    let mut recovered = false;
    for (module, version) in map {
        recovered = true;
        let version = version.as_scalar().map(str::to_string);
        out.push(Prereq {
            module: module.clone(),
            version,
            phase: phase.to_string(),
            relation: relation.to_string(),
        });
    }
    recovered
}

/// Resolve the recognized `meta-spec` version without erasing the observed
/// value: an unrecognized spec yields a finding, not a parse failure.
fn recognized_spec_version(
    root: &[(String, Yaml)],
    findings: &mut Vec<MetaYmlFinding>,
) -> Option<MetaSpecVersion> {
    let (_, value) = root.iter().find(|(k, _)| k == "meta-spec")?;
    let Yaml::Map(map) = value else { return None };
    let raw = map.iter().find(|(k, _)| k == "version")?.1.as_scalar()?.to_string();
    match raw.as_str() {
        "2" | "2.0" => Some(MetaSpecVersion::V2),
        "1.4" | "1.3" | "1.2" | "1.1" | "1.0" => Some(MetaSpecVersion::V1),
        other => {
            findings.push(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedSyntax,
                None,
                format!("unrecognized meta-spec version '{other}'"),
            ));
            None
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Digest;
    use perl_test_must::{must_some, must_some_with};

    fn fid() -> FileId {
        FileId::new("META.yml", &Digest::of("x"))
    }

    fn assert_non_success(outcome: &MetaYmlOutcome, kind: MetaYmlFindingKind, what: &str) {
        assert_ne!(outcome.state, MetaYmlParseState::Parsed, "{what}: must not parse");
        assert!(outcome.facts.is_none(), "{what}: a non-success state must carry no facts");
        assert!(
            outcome.findings.iter().any(|f| f.kind == kind),
            "{what}: expected a {kind:?} finding, got {:?}",
            outcome.findings
        );
    }

    #[test]
    fn flow_plain_scalars_refuse_embedded_delimiters_in_keys_and_values() {
        for token in ["a[b]", "a{b}"] {
            for source in [
                format!("requires: {{ Foo: {token} }}\n"),
                format!("license: [{token}]\n"),
                format!("requires: {{ {token}: 1 }}\n"),
            ] {
                assert_non_success(
                    &parse_meta_yml(fid(), &source),
                    MetaYmlFindingKind::MalformedSyntax,
                    &source,
                );
            }
            let block = format!("name: {token}\n");
            assert_eq!(must_some(parse_meta_yml(fid(), &block).facts).name.as_deref(), Some(token));
            let quoted = format!("requires: {{ '{token}': '{token}' }}\n");
            let facts = must_some(parse_meta_yml(fid(), &quoted).facts);
            assert_eq!(facts.prereqs[0].module, token);
            assert_eq!(facts.prereqs[0].version.as_deref(), Some(token));
        }
    }

    #[test]
    fn document_markers_use_ascii_separation_without_hiding_content() {
        for separator in [" ", "\t"] {
            let source = format!("---{separator}name: X\n");
            assert_eq!(must_some(parse_meta_yml(fid(), &source).facts).name.as_deref(), Some("X"));
            for source in
                [format!("...{separator}name: X\n"), format!("name: X\n...{separator}name: Y\n")]
            {
                assert_non_success(
                    &parse_meta_yml(fid(), &source),
                    MetaYmlFindingKind::MalformedSyntax,
                    &source,
                );
            }
            let source = format!("---{separator}# start\nname: X\n...{separator}# end\n");
            assert_eq!(must_some(parse_meta_yml(fid(), &source).facts).name.as_deref(), Some("X"));
        }
        let facts = must_some(parse_meta_yml(fid(), "...suffix: X\n---suffix: Y\n").facts);
        assert!(facts.name.is_none());
    }

    #[test]
    fn plain_scalar_admission_refuses_reserved_tokens_without_facts() {
        for token in [
            "X: Y", "X:", "]", "}", ",", "?", "? key", "- item", ": value", "@value", "`value",
            "%value",
        ] {
            let mut sources = vec![format!("name: {token}\n")];
            // A comma after an empty flow-map value is a valid trailing
            // separator, not a plain scalar. Exercise comma in a block item.
            sources.push(if token == "," {
                "license:\n  - ,\n".to_string()
            } else {
                format!("requires: {{ Foo: {token} }}\n")
            });
            for source in sources {
                assert_non_success(
                    &parse_meta_yml(fid(), &source),
                    MetaYmlFindingKind::MalformedSyntax,
                    &source,
                );
            }
        }
        for source in ["@name: X\n", "? name: X\n", "requires: { @Foo: 1 }\n"] {
            assert_non_success(
                &parse_meta_yml(fid(), source),
                MetaYmlFindingKind::MalformedSyntax,
                source,
            );
        }
    }

    #[test]
    fn plain_scalar_admission_preserves_safe_punctuation_and_quoted_indicators() {
        for token in [
            "?query",
            "-123",
            ":symbol",
            "Foo::Bar",
            "https://example.com/a#b",
            "...",
            "---",
            "Up, up, and away!",
            "prefix:\u{a0}suffix",
        ] {
            let source = format!("name: {token}\n");
            let result = parse_meta_yml(fid(), &source);
            assert_eq!(result.state, MetaYmlParseState::Parsed, "{source}: {:?}", result.findings);
            assert_eq!(must_some(result.facts).name.as_deref(), Some(token));
        }
        for token in ["X: Y", "]", ",", "?", "@value", "`value"] {
            let source = format!("name: '{token}'\n");
            assert_eq!(
                must_some(parse_meta_yml(fid(), &source).facts).name.as_deref(),
                Some(token)
            );
        }
    }

    #[test]
    fn empty_flow_value_with_trailing_comma_remains_null() {
        let result = parse_meta_yml(fid(), "requires: { Foo: , }\n");
        let facts = must_some(result.facts);
        assert_eq!(facts.prereqs.len(), 1);
        assert_eq!(facts.prereqs[0].module, "Foo");
        assert!(facts.prereqs[0].version.is_none());
    }

    #[test]
    fn indented_document_markers_refuse_instead_of_losing_values() {
        for marker in ["...", "---"] {
            for source in [format!("name:\n  {marker}\n"), format!("name:\n  {marker} # value\n")] {
                assert_non_success(
                    &parse_meta_yml(fid(), &source),
                    MetaYmlFindingKind::MalformedSyntax,
                    &source,
                );
            }
            let source = format!("license:\n  - {marker}\n");
            assert_eq!(must_some(parse_meta_yml(fid(), &source).facts).licenses, vec![marker]);
        }
        let result = parse_meta_yml(fid(), "---\nname: X\n... # end\n");
        assert_eq!(must_some(result.facts).name.as_deref(), Some("X"));
    }

    #[test]
    fn trailing_empty_document_is_refused() {
        for source in ["name: X\n---\n", "---\nname: X\n---\n", "name: X\n---\n# empty\n"] {
            assert_non_success(
                &parse_meta_yml(fid(), source),
                MetaYmlFindingKind::MultipleDocuments,
                source,
            );
        }
        for source in ["---\nname: X\n", "name: X\n...\n"] {
            assert_eq!(parse_meta_yml(fid(), source).state, MetaYmlParseState::Parsed);
        }
    }

    #[test]
    fn quoted_null_like_scalars_remain_strings_across_metadata_fields() {
        for (token, expected) in [
            ("'null'", "null"),
            ("\"null\"", "null"),
            ("'~'", "~"),
            ("\"~\"", "~"),
            ("''", ""),
            ("' '", " "),
        ] {
            for prerequisites in [
                format!("requires: {{ Foo: {token} }}"),
                format!("prereqs: {{ runtime: {{ requires: {{ Foo: {token} }} }} }}"),
            ] {
                let source = format!(
                    "name: {token}\nversion: {token}\nabstract: {token}\nlicense: [{token}]\n{prerequisites}\n"
                );
                let outcome = parse_meta_yml(fid(), &source);
                assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
                let facts = must_some(outcome.facts);
                assert_eq!(facts.name.as_deref(), Some(expected));
                assert_eq!(facts.version.as_deref(), Some(expected));
                assert_eq!(facts.summary.as_deref(), Some(expected));
                assert_eq!(facts.licenses, vec![expected]);
                assert_eq!(must_some(facts.prereqs.first()).version.as_deref(), Some(expected));
            }
            let source = format!("license: {token}\nrequires:\n  Foo: {token}\n");
            let facts = must_some(parse_meta_yml(fid(), &source).facts);
            assert_eq!(facts.licenses, vec![expected]);
            assert_eq!(must_some(facts.prereqs.first()).version.as_deref(), Some(expected));
        }
    }

    #[test]
    fn unquoted_null_scalars_are_absent_across_metadata_fields() {
        for token in ["null", "Null", "NULL", "~", ""] {
            let source = format!(
                "name: {token}\nversion: {token}\nabstract: {token}\nlicense: {token}\nrequires:\n  Foo: {token}\n"
            );
            let outcome = parse_meta_yml(fid(), &source);
            assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
            let facts = must_some(outcome.facts);
            assert!(facts.name.is_none());
            assert!(facts.version.is_none());
            assert!(facts.summary.is_none());
            assert!(facts.licenses.is_empty());
            assert!(must_some(facts.prereqs.first()).version.is_none());
        }
        let facts = must_some(parse_meta_yml(fid(), "license: [null, Null, NULL, ~]\n").facts);
        assert!(facts.licenses.is_empty());
    }

    #[test]
    fn null_prerequisite_versions_are_absent_but_zero_is_preserved() {
        for source in [
            "name: X\nrequires: { A: null, B: ~, C: 0, D: 1.50 }\n",
            "name: X\nprereqs: { runtime: { requires: { A: null, B: ~, C: 0, D: 1.50 } } }\n",
        ] {
            let outcome = parse_meta_yml(fid(), source);
            assert_eq!(outcome.state, MetaYmlParseState::Parsed);
            let facts = must_some(outcome.facts);
            for (module, expected) in
                [("A", None), ("B", None), ("C", Some("0")), ("D", Some("1.50"))]
            {
                let prerequisite = must_some(facts.prereqs.iter().find(|p| p.module == module));
                assert_eq!(prerequisite.version.as_deref(), expected, "{module}");
            }
        }
    }

    #[test]
    fn flow_values_preserve_structure_and_keys() {
        let parsed = parse_meta_yml(
            fid(),
            "name: X\nprereqs: { runtime: { requires: { Foo::Bar: 1.0 } } }\n",
        );
        assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{:?}", parsed.findings);
        let facts = parsed.facts.as_ref();
        assert!(facts.is_some_and(|f| {
            f.prereqs.iter().any(|p| p.module == "Foo::Bar" && p.version.as_deref() == Some("1.0"))
        }));
        let mut doc = Doc::new("items:\n  - key: [a, b]\n");
        assert!(scan_stream_safety(&mut doc).is_ok());
        let mut parser = Parser { lines: doc.body, pos: 0, nodes: 0, depth: 0 };
        let result = parser.parse_block(0);
        assert!(matches!(&result, Ok(Yaml::Map(root)) if matches!(&root[0].1,
            Yaml::Seq(items) if matches!(&items[0], Yaml::Map(entries)
                if matches!(&entries[0].1, Yaml::Seq(values) if values.len() == 2)))));
    }

    #[test]
    fn flow_collection_separators_and_delimiters_fail_closed() {
        for input in [
            "license: [perl_5,,mit]",
            "requires: {strict: 0,,warnings: 0}",
            "license: [perl_5}]",
            "license: [,perl_5]",
            "requires: {,strict: 0}",
            "license: [[perl_5},mit]",
            "license: [[perl_5]",
            "requires: {strict: 0]]}",
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_eq!(
                parsed.state,
                MetaYmlParseState::Malformed,
                "{input}: {:?}",
                parsed.findings
            );
            assert_non_success(&parsed, MetaYmlFindingKind::MalformedSyntax, input);
        }
        for input in [
            "license: []",
            "requires: {}",
            "license: [perl_5,]",
            "requires: {strict: 0,}",
            "prereqs: {runtime: {requires: {strict: 0,},},}",
            "license: ['a,b', 'a]b', 'a}b']",
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{input}: {:?}", parsed.findings);
            assert!(parsed.facts.is_some());
        }
    }

    #[test]
    fn plain_scalar_markers_are_content_not_yaml_features() {
        for value in ["Bread & butter", "Bread * butter", "Bread ! butter", "a,&b", "a,*b", "a,!b"]
        {
            let parsed = parse_meta_yml(fid(), &format!("abstract: {value}"));
            assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{value}: {:?}", parsed.findings);
            assert_eq!(parsed.facts.and_then(|f| f.summary), Some(value.to_string()));
        }
        for input in [
            "name: &anchor X",
            "name: *alias",
            "name: !tag X",
            "license: [*alias]",
            "license: [!tag X]",
            "license: [&anchor X]",
            "requires: {*alias: 0}",
            "license:\n  - *alias",
            "license: [ok, !tag X]",
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_non_success(&parsed, MetaYmlFindingKind::AnchorAliasOrTag, input);
        }
    }

    #[test]
    fn plain_scalar_punctuation_does_not_open_quotes() {
        for name in ["foo,'x", "foo:'x", "-'x", "foo['x", "foo{'x", "foo,\"x", "foo:\"x", "-\"x"] {
            let input = format!("name: {name} # comment");
            let parsed = parse_meta_yml(fid(), &input);
            assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{input}: {:?}", parsed.findings);
            assert_eq!(parsed.facts.and_then(|f| f.name), Some(name.to_string()), "{input}");
        }
        for input in [
            "license: ['a # quoted', 'b'] # comment",
            "license:\n  - 'a # quoted'\n  - 'b' # comment",
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{input}: {:?}", parsed.findings);
            assert_eq!(
                parsed.facts.map(|f| f.licenses),
                Some(vec!["a # quoted".into(), "b".into()])
            );
        }
    }

    #[test]
    fn quoted_scanners_preserve_escaped_delimiters() {
        for (input, expected) in [
            (r#"name: "a\" # literal" # comment"#, "a\" # literal"),
            (r#"name: "a\" , literal""#, "a\" , literal"),
            ("name: 'a'' # literal' # comment", "a' # literal"),
            ("name: O'Reilly # comment", "O'Reilly"),
            ("name: plain ' quote # comment", "plain ' quote"),
            ("name: mid\"word # comment", "mid\"word"),
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{input}: {:?}", parsed.findings);
            assert_eq!(parsed.facts.and_then(|f| f.name), Some(expected.to_string()));
        }
        let parsed = parse_meta_yml(fid(), r#"license: ["a\" , literal", 'b']"#);
        assert_eq!(parsed.state, MetaYmlParseState::Parsed, "{:?}", parsed.findings);
        assert_eq!(
            parsed.facts.map(|f| f.licenses),
            Some(vec!["a\" , literal".into(), "b".into()])
        );
        let parsed = parse_meta_yml(fid(), r#"requires: { "a\" : b": 1.0 }"#);
        assert!(parsed.facts.is_some_and(|f| f.prereqs.iter().any(|p| p.module == "a\" : b")));
        assert!(!token_present(r#"name: "a\" ! literal""#, "!"));
    }

    #[test]
    fn quoted_values_fail_closed_in_every_context() {
        for input in [
            "name: \"Broken",
            "name: 'Broken",
            r#"name: "bad\x41""#,
            r#"license: ["bad\x41"]"#,
            r#"requires: { Foo: "bad\x41" }"#,
            "items:\n  - key: \"bad\\x41\"\n",
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_ne!(parsed.state, MetaYmlParseState::Parsed, "{input}");
            assert!(parsed.facts.is_none(), "{input}");
        }
        for (input, state) in [
            (r#""bad\x41": value"#, MetaYmlParseState::Unsupported),
            (r#""bad"inner": value"#, MetaYmlParseState::Malformed),
        ] {
            let parsed = parse_meta_yml(fid(), input);
            assert_eq!(parsed.state, state, "{input}: {:?}", parsed.findings);
            assert!(parsed.facts.is_none());
        }
        let good = parse_meta_yml(fid(), r#"name: "a\nb""#);
        assert_eq!(good.facts.and_then(|f| f.name), Some("a\nb".into()));
    }

    #[test]
    fn flow_indicators_are_refused_but_literals_survive() {
        for value in ["[*alias]", "[&anchor x]", "[!tag x]", "[ok,*alias]", "{*alias: x}"] {
            let parsed = parse_meta_yml(fid(), &format!("license: {value}"));
            assert_non_success(&parsed, MetaYmlFindingKind::AnchorAliasOrTag, value);
        }
        let good = parse_meta_yml(fid(), "license: ['*literal', 'a!b', '&literal']");
        assert_eq!(good.state, MetaYmlParseState::Parsed, "{:?}", good.findings);
    }

    #[test]
    fn raw_comments_cannot_hide_controls() {
        for (input, line) in [("name: X\n# hidden \u{1}\n", 2), ("name: X # hidden \u{1}\n", 1)] {
            let parsed = parse_meta_yml(fid(), input);
            assert_non_success(&parsed, MetaYmlFindingKind::MalformedEncoding, "raw comment");
            assert_eq!(parsed.findings[0].line, Some(line));
        }
        assert_eq!(parse_meta_yml(fid(), "name: X # valid\r\n").state, MetaYmlParseState::Parsed);
    }

    const META_V2: &str = "--- # http://module-build.sourceforge.net/META-spec-v2.html
name:             App-Dist
version:          1.5
abstract:         Sample distribution
author:
  - A. Uthor <a.uthor@example.com>
license:          [perl_5, gpl_2]
meta-spec:
  url:            http://module-build.sourceforge.net/META-spec-v2.html
  version:        2
prereqs:
  runtime:
    requires:
      perl:       5.010
      File::Spec: 3.75
    recommends:
      JSON::PP:   4.0
  build:
    requires:
      Test::More: 0.98
provides:
  App::Dist:
    file:         lib/App/Dist.pm
    version:      1.5
no_index:
  directory:
    - t
    - inc
";

    const META_V1: &str = "--- #YAML:1.0
name:         Old-Dist
version:      0.01
abstract:     Classic v1.4 metadata
license:      perl_5
meta-spec:
  version:    1.4
  url:        http://module-build.sourceforge.net/META-spec-v1.4.html
requires:
  perl:       5.008
  strict:     0
build_requires:
  Test::More: 0
";

    #[test]
    fn valid_meta_v2_produces_deterministic_typed_facts() {
        let outcome = parse_meta_yml(fid(), META_V2);
        assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
        assert_eq!(outcome.spec_version, Some(MetaSpecVersion::V2));
        let facts = must_some_with(outcome.facts.as_ref(), "parsed facts");
        assert_eq!(facts.source, DistMetadataSource::MetaYml);
        assert_eq!(facts.name.as_deref(), Some("App-Dist"));
        // The version keeps its source spelling: no float coercion.
        assert_eq!(facts.version.as_deref(), Some("1.5"));
        assert_eq!(facts.summary.as_deref(), Some("Sample distribution"));
        assert_eq!(facts.licenses, vec!["perl_5", "gpl_2"]);

        let find = |module: &str| must_some(facts.prereqs.iter().find(|p| p.module == module));
        assert_eq!(find("perl").version.as_deref(), Some("5.010"));
        assert_eq!(find("perl").phase, "runtime");
        assert_eq!(find("Test::More").phase, "build");
        assert_eq!(find("JSON::PP").relation, "recommends");

        // Determinism: identical input → byte-identical outcome (including
        // fingerprint and ordering).
        assert_eq!(parse_meta_yml(fid(), META_V2), parse_meta_yml(fid(), META_V2));
    }

    #[test]
    fn valid_meta_v1_produces_flat_facts_and_v1_spec() {
        let outcome = parse_meta_yml(fid(), META_V1);
        assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
        assert_eq!(outcome.spec_version, Some(MetaSpecVersion::V1));
        let facts = must_some_with(outcome.facts.as_ref(), "parsed facts");
        assert_eq!(facts.name.as_deref(), Some("Old-Dist"));
        // "0.01" must never become a float.
        assert_eq!(facts.version.as_deref(), Some("0.01"));
        assert_eq!(facts.licenses, vec!["perl_5"]);
        let find = |module: &str| must_some(facts.prereqs.iter().find(|p| p.module == module));
        assert_eq!(find("strict").phase, "runtime");
        assert_eq!(find("Test::More").phase, "build");
        // `0` means "any" and stays a source string.
        assert_eq!(find("Test::More").version.as_deref(), Some("0"));
    }

    #[test]
    fn duplicate_keys_are_an_explicit_non_success() {
        let outcome = parse_meta_yml(fid(), "name: X\nname: Y\nversion: 1\n");
        assert_non_success(&outcome, MetaYmlFindingKind::DuplicateKey, "duplicate name key");
    }

    #[test]
    fn multiple_documents_are_an_explicit_non_success() {
        let outcome = parse_meta_yml(fid(), "name: X\nversion: 1\n---\nname: Y\nversion: 2\n");
        assert_non_success(&outcome, MetaYmlFindingKind::MultipleDocuments, "second document");

        let outcome = parse_meta_yml(fid(), "name: X\nversion: 1\n...\nname: Y\n");
        assert_non_success(
            &outcome,
            MetaYmlFindingKind::MultipleDocuments,
            "content after explicit document end",
        );
    }

    #[test]
    fn anchors_aliases_and_tags_are_refused() {
        let anchored = "name: &a X\nversion: *a\n";
        assert_non_success(
            &parse_meta_yml(fid(), anchored),
            MetaYmlFindingKind::AnchorAliasOrTag,
            "anchor/alias",
        );

        let tagged = "name: !!str X\nversion: 1\n";
        assert_non_success(
            &parse_meta_yml(fid(), tagged),
            MetaYmlFindingKind::AnchorAliasOrTag,
            "explicit tag",
        );
    }

    #[test]
    fn merge_keys_refuse_at_every_key_seam_without_fake_facts() {
        for input in [
            "<<: *defaults\nname: X\n",
            "<< : *defaults\nname: X\n",
            "requires: { <<: { Foo: 1 } }\n",
            "license:\n  - <<: *defaults\n",
        ] {
            let outcome = parse_meta_yml(fid(), input);
            assert_non_success(&outcome, MetaYmlFindingKind::AnchorAliasOrTag, input);
        }

        // The `<<` token as a *value* is an ordinary plain scalar.
        let facts = must_some(parse_meta_yml(fid(), "name: <<\n").facts);
        assert_eq!(facts.name.as_deref(), Some("<<"));
    }

    #[test]
    fn block_scalars_are_refused() {
        let outcome = parse_meta_yml(fid(), "name: X\nabstract: |\n  long text\nversion: 1\n");
        assert_non_success(&outcome, MetaYmlFindingKind::BlockScalar, "block scalar");

        let outcome = parse_meta_yml(fid(), "name: X\nitems:\n  - |\n    long text\nversion: 1\n");
        assert_non_success(&outcome, MetaYmlFindingKind::BlockScalar, "sequence block scalar");
    }

    #[test]
    fn malformed_syntax_is_not_an_empty_success() {
        let outcome = parse_meta_yml(fid(), "name: [unterminated\nversion: 1\n");
        assert_non_success(&outcome, MetaYmlFindingKind::MalformedSyntax, "unterminated flow");

        let outcome = parse_meta_yml(fid(), "   \n   \n");
        assert_non_success(
            &outcome,
            MetaYmlFindingKind::MalformedSyntax,
            "whitespace-only document",
        );

        // A bare scalar at top level is not a mapping.
        let outcome = parse_meta_yml(fid(), "just a scalar\n");
        assert_non_success(&outcome, MetaYmlFindingKind::MalformedSyntax, "scalar top level");
    }

    #[test]
    fn forbidden_control_characters_are_malformed_encoding() {
        let outcome = parse_meta_yml(fid(), "name: X\u{0007}\nversion: 1\n");
        assert_non_success(
            &outcome,
            MetaYmlFindingKind::MalformedEncoding,
            "BEL control character",
        );
    }

    #[test]
    fn resource_budgets_fail_closed() {
        let mut deep = String::from("root:\n");
        for depth in 0..64 {
            deep.push_str(&" ".repeat(depth + 1));
            deep.push_str("k:\n");
        }
        let outcome = parse_meta_yml(fid(), &deep);
        assert_non_success(&outcome, MetaYmlFindingKind::ResourceLimit, "excessive nesting");

        let wide = format!("name: {}\nversion: 1\n", "x".repeat(MAX_INPUT_BYTES + 1));
        let outcome = parse_meta_yml(fid(), &wide);
        assert_non_success(&outcome, MetaYmlFindingKind::ResourceLimit, "oversized input");

        let flow = format!(
            "name: X\nversion: 1\nnested: {}x{}\n",
            "[".repeat(MAX_DEPTH + 2),
            "]".repeat(MAX_DEPTH + 2)
        );
        let outcome = parse_meta_yml(fid(), &flow);
        assert_non_success(&outcome, MetaYmlFindingKind::ResourceLimit, "nested flow depth");
    }

    #[test]
    fn flow_mapping_duplicate_keys_are_a_typed_finding() {
        // Flow maps used to last-value-win duplicate keys silently; the
        // contract ("detected rather than last-value-accepted") must hold for
        // both collection styles.
        let outcome = parse_meta_yml(fid(), "name: X\nversion: 1\nnested: {a: 1, a: 2}\n");
        assert_non_success(&outcome, MetaYmlFindingKind::DuplicateKey, "flow duplicate key");

        // The same refusal applies to flow maps nested in flow sequences.
        let nested = parse_meta_yml(fid(), "name: X\nversion: 1\nnested: [{a: 1, a: 2}]\n");
        assert_non_success(
            &nested,
            MetaYmlFindingKind::DuplicateKey,
            "flow duplicate key inside a flow sequence",
        );

        // Well-formed flow mappings still parse cleanly.
        let clean = parse_meta_yml(fid(), "name: X\nversion: 1\nnested: {a: 1, b: 2}\n");
        assert_eq!(clean.state, MetaYmlParseState::Parsed, "{:?}", clean.findings);
    }

    #[test]
    fn flat_documents_cannot_dodge_the_node_budget() {
        // Nodes were once charged only per parse_block, so these flat shapes
        // parsed clean with a dead 10k budget.
        let mut flat_map = String::new();
        for i in 0..10_500 {
            flat_map.push_str(&format!("k{i}: {i}\n"));
        }
        let outcome = parse_meta_yml(fid(), &flat_map);
        assert_non_success(&outcome, MetaYmlFindingKind::ResourceLimit, "flat map over budget");

        let mut flat_seq = String::from("root:\n");
        for i in 0..10_500 {
            flat_seq.push_str(&format!("  - {i}\n"));
        }
        let outcome = parse_meta_yml(fid(), &flat_seq);
        assert_non_success(&outcome, MetaYmlFindingKind::ResourceLimit, "flat seq over budget");

        // Documents inside the budget still parse.
        let mut within = String::new();
        for i in 0..4_000 {
            within.push_str(&format!("k{i}: {i}\n"));
        }
        let outcome = parse_meta_yml(fid(), &within);
        assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
    }

    #[test]
    fn wide_duplicate_detection_stays_linear() {
        // 60k unique keys were previously quadratic in duplicate detection
        // (~4s); the seen-key set keeps the whole parse well under a second,
        // and the node budget bounds how far a flat document can run at all.
        let mut wide = String::new();
        for i in 0..60_000 {
            wide.push_str(&format!("k{i}: 0\n"));
        }
        let start = std::time::Instant::now();
        let outcome = parse_meta_yml(fid(), &wide);
        let elapsed = start.elapsed();
        assert_non_success(&outcome, MetaYmlFindingKind::ResourceLimit, "wide map over budget");
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "60k-entry parse must stay bounded, took {elapsed:?}"
        );

        // Duplicate detection at scale: 9k unique keys then a repeat, fast
        // and still a typed refusal.
        let mut dup_at_scale = String::new();
        for i in 0..9_000 {
            dup_at_scale.push_str(&format!("k{i}: 0\n"));
        }
        dup_at_scale.push_str("k0: 1\n");
        let start = std::time::Instant::now();
        let outcome = parse_meta_yml(fid(), &dup_at_scale);
        let elapsed = start.elapsed();
        assert_non_success(&outcome, MetaYmlFindingKind::DuplicateKey, "duplicate at scale");
        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "9k-entry duplicate scan must stay linear, took {elapsed:?}"
        );
    }

    #[test]
    fn missing_scalars_do_not_become_empty_success() {
        // A document that parses but declares nothing still yields facts with
        // explicit Option/vec emptiness — the STATE is honest, and malformed
        // input never reaches this path.
        let outcome = parse_meta_yml(fid(), "name: X\n");
        assert_eq!(outcome.state, MetaYmlParseState::Parsed);
        let facts = must_some_with(outcome.facts, "facts");
        assert_eq!(facts.name.as_deref(), Some("X"));
        assert_eq!(facts.version, None, "absent version stays absent");
        assert!(facts.licenses.is_empty());
        assert!(facts.prereqs.is_empty());
    }

    #[test]
    fn malformed_never_yields_facts() {
        // The core negative: every non-Parsed state must carry facts: None.
        for (label, input) in [
            ("malformed", "name: [unterminated\n"),
            ("duplicate", "name: X\nname: Y\n"),
            ("multi-doc", "name: X\n---\nname: Y\n"),
            ("anchor", "name: &a X\n"),
            ("block scalar", "abstract: |\n  x\n"),
            ("empty", "   \n"),
        ] {
            let outcome = parse_meta_yml(fid(), input);
            assert!(
                matches!(
                    outcome.state,
                    MetaYmlParseState::Malformed | MetaYmlParseState::Unsupported
                ),
                "{label}: unexpected state {:?}",
                outcome.state
            );
            assert!(outcome.facts.is_none(), "{label}: non-success carried facts");
        }
    }

    #[test]
    fn facts_are_host_path_independent() {
        let a = parse_meta_yml(FileId::new("META.yml", &Digest::of("x")), META_V2);
        let b = parse_meta_yml(FileId::new("META.yml", &Digest::of("x")), META_V2);
        assert_eq!(a.facts, b.facts, "facts depend only on path+content identity");
        assert_eq!(a.source_digest, b.source_digest);
        assert!(a.source_digest.starts_with("fnv64:"));
    }

    #[test]
    fn crlf_and_comments_are_tolerated() {
        let outcome =
            parse_meta_yml(fid(), "---\r\nname: X # trailing comment\r\nversion: 2 # keep\r\n");
        assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
        let facts = must_some_with(outcome.facts, "facts");
        assert_eq!(facts.name.as_deref(), Some("X"));
        assert_eq!(facts.version.as_deref(), Some("2"));
    }

    #[test]
    fn quoted_scalars_keep_exact_spelling() {
        let outcome = parse_meta_yml(
            fid(),
            "name: \"Quoted-Name\"\nversion: '0.01'\nabstract: 'it''s quoted'\n",
        );
        assert_eq!(outcome.state, MetaYmlParseState::Parsed, "{:?}", outcome.findings);
        let facts = must_some_with(outcome.facts, "facts");
        assert_eq!(facts.name.as_deref(), Some("Quoted-Name"));
        assert_eq!(facts.version.as_deref(), Some("0.01"));
        assert_eq!(facts.summary.as_deref(), Some("it's quoted"));
    }

    #[test]
    fn limitations_are_stable_and_declared() {
        let outcome = parse_meta_yml(fid(), META_V2);
        assert_eq!(outcome.limitations.len(), META_YML_LIMITATIONS.len());
        assert_eq!(outcome.limitations, META_YML_LIMITATIONS.to_vec());
        assert!(!META_YML_LIMITATIONS.is_empty());
    }
}
