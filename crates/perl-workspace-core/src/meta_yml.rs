//! Bounded `META.yml` parsing and the shared metadata parse-state model
//! (#8458).
//!
//! `META.yml` is the YAML-flavored sibling of `META.json` ([crate::dist]
//! owns the JSON path). This module adds the missing YAML metadata source
//! with an explicit parse-state surface so consumers (Kwalitee, #7176 fact
//! wiring) can distinguish absent, unreadable, malformed, unsupported, and
//! successfully-parsed inputs — malformed metadata can never become an empty
//! successful fact set.
//!
//! # Safety contract
//!
//! The parser is a **bounded YAML subset** written in std-only Rust; there is
//! no Perl/CPAN YAML subprocess and no new dependency:
//!
//! * block mappings and block sequences (indentation-based);
//! * flow collections (`[a, b]`, `{k: v}`);
//! * plain, single-quoted, and double-quoted scalars;
//! * `#` comments, `---` / `...` document markers (single document only).
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
//! stay the string `"1.5"`); only recognized v1.4/v2 fields are normalized
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

    let mut doc = Doc::new(content);
    if let Err(finding) = scan_stream_safety(&mut doc) {
        findings.push(finding);
        return outcome(MetaYmlParseState::Unsupported, None, None, findings, source_digest);
    }

    let mut parser = Parser { lines: doc.body, pos: 0, nodes: 0, depth: 0 };
    let value = match parser.parse_block(0) {
        Ok(value) => value,
        Err(finding) => {
            let state = match finding.kind {
                MetaYmlFindingKind::ResourceLimit
                | MetaYmlFindingKind::DuplicateKey
                | MetaYmlFindingKind::AnchorAliasOrTag
                | MetaYmlFindingKind::BlockScalar
                | MetaYmlFindingKind::MultipleDocuments => MetaYmlParseState::Unsupported,
                _ => MetaYmlParseState::Malformed,
            };
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

    for (idx, line) in doc.raw.iter().enumerate() {
        if has_forbidden_control(&line.text) {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::MalformedEncoding,
                Some(line.number),
                "line contains a control character YAML forbids",
            ));
        }
        let trimmed = line.text.trim();
        if trimmed == "..." {
            continue;
        }
        if trimmed == "---" || trimmed.starts_with("--- ") {
            let has_later_content = doc.raw[idx + 1..]
                .iter()
                .any(|l| !l.text.trim().is_empty() && l.text.trim() != "...");
            if !body.is_empty() || seen_first_marker {
                // A separator after content (or a second marker) starts a
                // second document when real content follows; a bare trailing
                // closer is tolerated.
                if has_later_content {
                    return Err(MetaYmlFinding::new(
                        MetaYmlFindingKind::MultipleDocuments,
                        Some(line.number),
                        "a second document starts after the first; single-document META.yml only",
                    ));
                }
                break;
            }
            seen_first_marker = true;
            if let Some(rest) = trimmed.strip_prefix("--- ") {
                // `--- name: X` starts the document inline with content.
                if !rest.trim().is_empty() {
                    body.push(Line { number: line.number, text: rest.to_string().into() });
                }
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
        let value_part =
            split_key(trimmed).map(|(_, rest)| rest).unwrap_or_else(|| trimmed.to_string());
        let value_trimmed = value_part.trim_start();
        if value_trimmed.starts_with('|') || value_trimmed.starts_with('>') {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::BlockScalar,
                Some(line.number),
                "block scalars ('|' / '>') are not supported",
            ));
        }
        if trimmed.starts_with("<<:") {
            return Err(MetaYmlFinding::new(
                MetaYmlFindingKind::AnchorAliasOrTag,
                Some(line.number),
                "merge keys ('<<') are refused",
            ));
        }
    }

    doc.body = body;
    Ok(())
}

/// Whether a token starting with `marker` appears outside quoted regions and
/// at a token boundary (line start or after whitespace) — a mid-word `!` in a
/// plain scalar is not a tag.
fn token_present(line: &str, marker: &str) -> bool {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {}
            _ => {
                let at_start = i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t';
                if at_start && line[i..].starts_with(marker) {
                    return true;
                }
            }
        }
        i += 1;
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
    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double => {
                let at_start = i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t';
                if at_start {
                    return std::borrow::Cow::Owned(line[..i].trim_end().to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    match line.len() - line.trim_end().len() {
        0 => std::borrow::Cow::Borrowed(line),
        _ => std::borrow::Cow::Owned(line.trim_end().to_string()),
    }
}

// ── Block parser ─────────────────────────────────────────────────────────────

/// A YAML value in the bounded subset. Scalars keep their source spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Yaml {
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
            } else if let Some((key, value)) = try_split_map_entry(&rest)? {
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
                items.push(self.parse_scalar_or_flow(&rest, line.number)?);
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
            let Some((key, rest)) = split_key(&trimmed) else {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line.number),
                    format!("expected `key: value`, found: {trimmed:.40}"),
                ));
            };
            self.pos += 1;
            let value = if rest.is_empty() {
                // Value is the nested block (or an empty scalar when the next
                // line is a sibling). Every entry charges a node — block
                // values through their own parse_block, flow values through
                // their elements — so flat maps cannot dodge the budget.
                match self.peek() {
                    Some(next) if indentation(&next.text) > indent => {
                        self.parse_block(indent + 1)?
                    }
                    _ => {
                        self.charge_node_at(line.number)?;
                        Yaml::Scalar(String::new())
                    }
                }
            } else {
                self.parse_scalar_or_flow(&rest, line.number)?
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
    fn parse_scalar_or_flow(&mut self, text: &str, line: usize) -> Result<Yaml, MetaYmlFinding> {
        let trimmed = text.trim();
        if trimmed.starts_with('[') {
            return self.flow_seq(trimmed, line);
        }
        if trimmed.starts_with('{') {
            return self.flow_map(trimmed, line);
        }
        self.charge_node_at(line)?;
        Ok(Yaml::Scalar(unquote(trimmed)))
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
        let mut items = Vec::new();
        for part in split_flow(inner) {
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
                items.push(Yaml::Scalar(unquote(part)));
            }
        }
        Ok(Yaml::Seq(items))
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
        let mut entries = Vec::new();
        let mut seen_keys: HashSet<String> = HashSet::new();
        for part in split_flow(inner) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let Some((raw_key, value)) = part.split_once(':') else {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::MalformedSyntax,
                    Some(line),
                    format!("flow mapping entry without ':': {part:.40}"),
                ));
            };
            self.charge_node_at(line)?;
            let key = unquote(raw_key.trim());
            if !seen_keys.insert(key.clone()) {
                return Err(MetaYmlFinding::new(
                    MetaYmlFindingKind::DuplicateKey,
                    Some(line),
                    format!("duplicate key `{key}` in a flow mapping; last-value-wins is refused"),
                ));
            }
            entries.push((key, Yaml::Scalar(unquote(value.trim()))));
        }
        Ok(Yaml::Map(entries))
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
fn split_key(text: &str) -> Option<(String, String)> {
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0usize;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'[' | b'{' if !in_single && !in_double => depth += 1,
            b']' | b'}' if !in_single && !in_double => depth = depth.saturating_sub(1),
            b':' if !in_single && !in_double && depth == 0 => {
                let after = text[i + 1..].trim_start();
                if after.is_empty() || text[i + 1..].starts_with(' ') {
                    let key = unquote(text[..i].trim());
                    return Some((key, after.to_string()));
                }
            }
            _ => {}
        }
    }
    None
}

/// `- key: value` detection returning the split entry.
fn try_split_map_entry(text: &str) -> Result<Option<(String, Yaml)>, MetaYmlFinding> {
    match split_key(text) {
        Some((key, rest)) => Ok(Some((key, Yaml::Scalar(rest)))),
        None => Ok(None),
    }
}

fn unquote(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return trimmed[1..trimmed.len() - 1].replace("''", "'");
    }
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        return decode_double_quoted(&trimmed[1..trimmed.len() - 1]);
    }
    trimmed.to_string()
}

fn decode_double_quoted(text: &str) -> String {
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
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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
fn split_flow(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut in_single = false;
    let mut in_double = false;
    let mut current = String::new();
    for c in text.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                current.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                current.push(c);
            }
            '[' | '{' if !in_single && !in_double => {
                depth += 1;
                current.push(c);
            }
            ']' | '}' if !in_single && !in_double => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 && !in_single && !in_double => {
                parts.push(std::mem::take(&mut current));
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

// ── Fact normalization (mirrors dist::parse_meta_json) ──────────────────────

fn root_string(root: &[(String, Yaml)], key: &str) -> Option<String> {
    let value = root.iter().find(|(k, _)| k == key).map(|(_, v)| v)?;
    let scalar = value.as_scalar()?;
    let scalar = scalar.trim();
    if scalar.is_empty() || scalar == "~" || scalar == "null" {
        return None;
    }
    Some(scalar.to_string())
}

fn root_licenses(root: &[(String, Yaml)]) -> Vec<String> {
    let Some((_, value)) = root.iter().find(|(k, _)| k == "license") else {
        return Vec::new();
    };
    match value {
        // v2: an array of license strings.
        Yaml::Seq(items) => items
            .iter()
            .filter_map(|v| v.as_scalar().map(str::to_string))
            .filter(|s| !s.is_empty() && s != "~" && s != "null")
            .collect(),
        // v1.4: a single string.
        Yaml::Scalar(s) if !s.is_empty() && s != "~" && s != "null" => vec![s.clone()],
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
        let version = version.as_scalar().map(str::to_string).filter(|v| !v.is_empty() && v != "~");
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
        let facts = outcome.facts.as_ref().expect("parsed facts");
        assert_eq!(facts.source, DistMetadataSource::MetaYml);
        assert_eq!(facts.name.as_deref(), Some("App-Dist"));
        // The version keeps its source spelling: no float coercion.
        assert_eq!(facts.version.as_deref(), Some("1.5"));
        assert_eq!(facts.summary.as_deref(), Some("Sample distribution"));
        assert_eq!(facts.licenses, vec!["perl_5", "gpl_2"]);

        let find = |module: &str| facts.prereqs.iter().find(|p| p.module == module).unwrap();
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
        let facts = outcome.facts.as_ref().expect("parsed facts");
        assert_eq!(facts.name.as_deref(), Some("Old-Dist"));
        // "0.01" must never become a float.
        assert_eq!(facts.version.as_deref(), Some("0.01"));
        assert_eq!(facts.licenses, vec!["perl_5"]);
        let find = |module: &str| facts.prereqs.iter().find(|p| p.module == module).unwrap();
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
    fn block_scalars_are_refused() {
        let outcome = parse_meta_yml(fid(), "name: X\nabstract: |\n  long text\nversion: 1\n");
        assert_non_success(&outcome, MetaYmlFindingKind::BlockScalar, "block scalar");
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
        let facts = outcome.facts.expect("facts");
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
        let facts = outcome.facts.expect("facts");
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
        let facts = outcome.facts.expect("facts");
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
