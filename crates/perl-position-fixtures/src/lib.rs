//! Independent, versioned source-coordinate facts for exact byte subjects (#8172).
//!
//! The checked JSON is literal expected data. Hex is the raw-byte storage format:
//! Git's line-ending conversion can change JSON whitespace but cannot change the
//! represented CRLF/BOM/invalid bytes without failing the length, digest, or
//! semantic checks. No production scanner or mapper is imported here.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

/// On-disk authority version.
pub const SCHEMA_VERSION: &str = "position-fixtures/v1";
/// Accepted source-line policy identity from ADR-0048.
pub const POLICY_ID: &str = "lf-source-lines/v1";
/// Checked-in machine authority, relative to the repository root.
pub const MANIFEST_PATH: &str = "crates/perl-position-fixtures/fixtures/position-fixtures.v1.json";
/// Generated human review projection, relative to the repository root.
pub const PROJECTION_PATH: &str = "docs/generated/position-fixtures.v1.md";

// Versioned coverage admission. A valid subset is not the v1 conformance pack.
const REQUIRED_CASE_IDS: &[&str] = &[
    "ascii",
    "ascii_for_bom",
    "astral",
    "bare_cr",
    "bmp",
    "bom_after_lf",
    "bom_ascii",
    "bom_only",
    "bom_unicode",
    "combining",
    "consecutive_lf",
    "crcrlf",
    "crlf",
    "double_bom",
    "empty",
    "empty_for_bom",
    "ff",
    "invalid_continuation_end",
    "invalid_continuation_middle",
    "invalid_continuation_start",
    "invalid_leading_end",
    "invalid_leading_middle",
    "invalid_leading_start",
    "invalid_truncated_end",
    "invalid_truncated_middle",
    "invalid_truncated_start",
    "large_line",
    "lf",
    "lfcr",
    "ls",
    "many_lines",
    "mixed",
    "multiple_terminal_lf",
    "nel",
    "newline_only",
    "nonleading_bom",
    "nul",
    "ps",
    "repeated",
    "single_bom_for_double",
    "terminal_cr",
    "terminal_lf",
    "unicode_crlf",
    "unicode_for_bom",
    "vt",
];
// Tags are a consumer-facing filter contract, including both membership and
// order in list/explain output. Keep this literal v1 inventory independent of
// the generator and manifest so a missing or added tag cannot hide a case.
const REQUIRED_TAGS: &[(&str, &[&str])] = &[
    ("ascii", &["basic", "small"]),
    ("ascii_for_bom", &["bom", "parser_subject", "small"]),
    ("astral", &["unicode", "crlf", "small", "decisive"]),
    ("bare_cr", &["newline", "bare_cr", "small"]),
    ("bmp", &["unicode", "small"]),
    ("bom_after_lf", &["bom", "newline", "small"]),
    ("bom_ascii", &["bom", "presentation", "small"]),
    ("bom_only", &["bom", "presentation", "small"]),
    ("bom_unicode", &["bom", "presentation", "unicode", "small"]),
    ("combining", &["unicode", "small"]),
    ("consecutive_lf", &["newline", "small"]),
    ("crcrlf", &["newline", "crlf", "small"]),
    ("crlf", &["newline", "crlf", "small"]),
    ("double_bom", &["bom", "presentation", "small"]),
    ("empty", &["basic", "small"]),
    ("empty_for_bom", &["bom", "parser_subject", "small"]),
    ("ff", &["ropey_control", "small"]),
    ("invalid_continuation_end", &["invalid_utf8", "ingress"]),
    ("invalid_continuation_middle", &["invalid_utf8", "ingress"]),
    ("invalid_continuation_start", &["invalid_utf8", "ingress"]),
    ("invalid_leading_end", &["invalid_utf8", "ingress"]),
    ("invalid_leading_middle", &["invalid_utf8", "ingress"]),
    ("invalid_leading_start", &["invalid_utf8", "ingress"]),
    ("invalid_truncated_end", &["invalid_utf8", "ingress"]),
    ("invalid_truncated_middle", &["invalid_utf8", "ingress"]),
    ("invalid_truncated_start", &["invalid_utf8", "ingress"]),
    ("large_line", &["generated_boundary", "newline"]),
    ("lf", &["newline", "small"]),
    ("lfcr", &["newline", "bare_cr", "small"]),
    ("ls", &["ropey_control", "small"]),
    ("many_lines", &["generated_boundary", "newline"]),
    ("mixed", &["newline", "crlf", "small"]),
    ("multiple_terminal_lf", &["newline", "small"]),
    ("nel", &["ropey_control", "small"]),
    ("newline_only", &["newline", "small"]),
    ("nonleading_bom", &["bom", "small"]),
    ("nul", &["basic", "small"]),
    ("ps", &["ropey_control", "small"]),
    ("repeated", &["basic", "small"]),
    ("single_bom_for_double", &["bom", "parser_subject", "small"]),
    ("terminal_cr", &["newline", "bare_cr", "small"]),
    ("terminal_lf", &["newline", "small"]),
    ("unicode_crlf", &["newline", "crlf", "unicode", "small"]),
    ("unicode_for_bom", &["bom", "parser_subject", "unicode", "small"]),
    ("vt", &["ropey_control", "small"]),
];
const REQUIRED_RELATIONS: &[(&str, &str, &str)] = &[
    ("ascii", "identity", "ascii"),
    ("bom_after_lf", "identity", "bom_after_lf"),
    ("bom_ascii", "strip_leading_bom", "ascii_for_bom"),
    ("bom_only", "strip_leading_bom", "empty_for_bom"),
    ("bom_unicode", "strip_leading_bom", "unicode_for_bom"),
    ("double_bom", "strip_leading_bom", "single_bom_for_double"),
    ("nonleading_bom", "identity", "nonleading_bom"),
];

/// Versioned collection of exact source subjects and literal expectations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Schema identity.
    pub schema_version: String,
    /// Accepted LF policy identity.
    pub policy_id: String,
    /// Checked cases, in stable ID order.
    pub cases: Vec<Case>,
}

/// Manifest whose byte integrity and expected facts have been checked.
pub struct ValidatedManifest {
    inner: Manifest,
}

impl ValidatedManifest {
    /// Read all validated cases in stable ID order.
    pub fn cases(&self) -> &[Case] {
        &self.inner.cases
    }

    /// Select validated cases by exact ID or tag.
    pub fn select(&self, filter: &str) -> Vec<&Case> {
        self.inner
            .cases
            .iter()
            .filter(|case| case.id == filter || case.tags.iter().any(|tag| tag == filter))
            .collect()
    }

    /// Explain one validated case by ID.
    pub fn explain(&self, id: &str) -> Option<String> {
        self.inner.cases.iter().find(|case| case.id == id).map(explain_case)
    }

    /// Render a deterministic projection of validated literal facts.
    pub fn markdown(&self) -> String {
        render_markdown(&self.inner)
    }
}

/// One exact raw-byte subject.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Stable cross-consumer subject ID.
    pub id: String,
    /// Source and consumer-domain tags.
    pub tags: Vec<String>,
    /// Generator identity; `literal-hex/v1` never depends on checkout EOL.
    pub raw_identity: String,
    /// Complete raw-byte representation.
    pub raw_hex: String,
    /// Small cases enumerate every scalar and wire boundary; large generated
    /// boundary cases declare selected probes to keep checked data reviewable.
    pub exhaustive: bool,
    /// Independent exact byte count.
    pub byte_len: usize,
    /// Independent SHA-256 over raw bytes.
    pub sha256: String,
    /// `valid_utf8` or `invalid_utf8_ingress`.
    pub decode: String,
    /// Typed decoder failure, present only for invalid ingress bytes.
    pub decode_error: Option<DecodeError>,
    /// Literal Unicode scalar values for valid cases.
    pub scalars: Vec<u32>,
    /// Exact presentation source subject ID, absent for invalid input.
    pub subject: Option<String>,
    /// Optional explicit relation to a second exact subject.
    pub relation: Option<Relation>,
    /// Literal LF records; absent for invalid input.
    pub lines: Vec<Line>,
    /// All valid source scalar boundaries for exhaustive small cases.
    pub boundaries: Vec<usize>,
    /// Literal parser-point facts, including separator interior.
    pub parser_points: Vec<ParserPoint>,
    /// Literal parser point-to-byte queries, including noncanonical points.
    pub parser_queries: Vec<ParserQuery>,
    /// Literal UTF-8/UTF-16 wire facts and negative dispositions.
    pub wire: Vec<WireFact>,
    /// Literal range, source-byte, and harness-refusal controls.
    pub refusals: Vec<RefusalFact>,
    /// Expected partition cuts; `all_byte_cuts` means every single split.
    pub chunks: Vec<String>,
}

/// Explicit relation between two source subjects, never an implicit BOM offset.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Relation {
    /// Target case ID.
    pub target: String,
    /// `identity` or `strip_leading_bom`.
    pub kind: String,
    /// Literal source/target boundary pairs. `None` denotes an elided prefix.
    pub boundary_map: Vec<BoundaryMap>,
    /// Literal range controls for wholly elided, crossing, and content spans.
    pub ranges: Vec<RangeMap>,
}

/// One source-to-target boundary relation fact.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryMap {
    /// Source byte boundary.
    pub source: usize,
    /// Target byte boundary if addressable.
    pub target: Option<usize>,
}

/// One explicitly mapped source interval under a named relation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RangeMap {
    /// Source start boundary.
    pub source_start: usize,
    /// Source end boundary.
    pub source_end: usize,
    /// Target start, absent for wholly elided input.
    pub target_start: Option<usize>,
    /// Target end, absent for wholly elided input.
    pub target_end: Option<usize>,
    /// `exact`, `elided`, or `crosses_elided_prefix`.
    pub disposition: String,
}

/// One accepted LF source-line record.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    /// First byte.
    pub start: usize,
    /// Exclusive content end.
    pub content_end: usize,
    /// Exclusive separator end.
    pub separator_end: usize,
    /// `none`, `lf`, or `crlf`.
    pub separator: String,
}

/// One canonical parser UTF-8-byte point.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserPoint {
    /// Source byte boundary.
    pub byte: usize,
    /// Zero-based LF row.
    pub row: usize,
    /// UTF-8 byte column from row start.
    pub column: usize,
}

/// One LF-row UTF-8-byte-column parser query.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParserQuery {
    /// Zero-based row.
    pub row: usize,
    /// Byte column from row start.
    pub column: usize,
    /// `exact`, `invalid_byte_boundary`, `old_row_noncanonical`, or `invalid_row`.
    pub disposition: String,
    /// Canonical source byte only for exact queries.
    pub byte: Option<usize>,
}

/// Strict UTF-8 decoder failure at original ingress byte coordinates.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecodeError {
    /// `truncated`, `invalid_leading`, or `invalid_continuation`.
    pub kind: String,
    /// First offending byte.
    pub start: usize,
    /// Exclusive decoder error end.
    pub end: usize,
}

/// One incoming or outgoing strict wire-coordinate expectation.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireFact {
    /// `utf-8` or `utf-16`.
    pub encoding: String,
    /// `incoming` or `outgoing`.
    pub direction: String,
    /// Zero-based line.
    pub line: usize,
    /// Wire code-unit column.
    pub column: usize,
    /// `exact`, `line_end_normalized`, `invalid_line`,
    /// `invalid_code_unit_boundary`, or `invalid_separator_interior`.
    pub disposition: String,
    /// Source byte boundary for exact/normalized results.
    pub byte: Option<usize>,
}

/// One negative control outside a valid wire content position.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RefusalFact {
    /// Query or injected harness condition.
    pub kind: String,
    /// First byte when coordinate-based.
    pub start: Option<usize>,
    /// End byte when range-based.
    pub end: Option<usize>,
    /// Expected typed disposition.
    pub disposition: String,
}

/// Decode the complete hex source without text normalization.
pub fn raw_bytes(case: &Case) -> Result<Vec<u8>, String> {
    let chars = case.raw_hex.as_bytes();
    if !chars.len().is_multiple_of(2) || !chars.iter().all(u8::is_ascii_hexdigit) {
        return Err(format!("{}: raw_hex is not even-length ASCII hex", case.id));
    }
    chars
        .chunks_exact(2)
        .map(|pair| {
            let chunk = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
            u8::from_str_radix(chunk, 16).map_err(|error| error.to_string())
        })
        .collect()
}

/// Read the checked authority and validate it before returning any case.
pub fn load(root: &Path) -> Result<ValidatedManifest, String> {
    let path = root.join(MANIFEST_PATH);
    let contents =
        fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    validate(&manifest)?;
    Ok(ValidatedManifest { inner: manifest })
}

fn explain_case(case: &Case) -> String {
    let mut out = format!(
        "{} [{}] {} bytes SHA-256 {}\nraw hex: {}\ndecode: {}\n",
        case.id,
        case.tags.join(", "),
        case.byte_len,
        case.sha256,
        case.raw_hex,
        case.decode
    );
    if let Some(error) = &case.decode_error {
        out.push_str(&format!("decode error: {} {}..{}\n", error.kind, error.start, error.end));
    }
    for (index, line) in case.lines.iter().enumerate() {
        out.push_str(&format!(
            "row {index}: {}..{}..{} {}\n",
            line.start, line.content_end, line.separator_end, line.separator
        ));
    }
    for fact in &case.wire {
        out.push_str(&format!(
            "{} {} {}:{} -> {} {:?}\n",
            fact.direction, fact.encoding, fact.line, fact.column, fact.disposition, fact.byte
        ));
    }
    for query in &case.parser_queries {
        out.push_str(&format!(
            "parser {}:{} -> {} {:?}\n",
            query.row, query.column, query.disposition, query.byte
        ));
    }
    for fact in &case.refusals {
        out.push_str(&format!(
            "refusal {} {:?}..{:?} -> {}\n",
            fact.kind, fact.start, fact.end, fact.disposition
        ));
    }
    out
}

fn render_markdown(manifest: &Manifest) -> String {
    let mut out = String::from(
        "# Source coordinate fixtures v1\n\nGenerated from `crates/perl-position-fixtures/fixtures/position-fixtures.v1.json`. Do not edit. ADR-0048 (`lf-source-lines/v1`) governs LF rows. Invalid UTF-8 is ingress evidence only.\n\n",
    );
    for case in &manifest.cases {
        out.push_str(&format!(
            "## `{}`\n\nTags: {}. Coverage: `{}`. Decode: `{}`. Bytes: {}. SHA-256: `{}`.\n\nHex: `{}`\n\n",
            case.id,
            case.tags.join(", "),
            if case.exhaustive { "all small-case boundaries" } else { "selected large-boundary probes" },
            case.decode,
            case.byte_len,
            case.sha256,
            case.raw_hex
        ));
        if let Some(error) = &case.decode_error {
            out.push_str(&format!(
                "Decoder error: `{}` at bytes {}..{}.\n\n",
                error.kind, error.start, error.end
            ));
        }
        if let Some(relation) = &case.relation {
            out.push_str(&format!(
                "Relation: `{}` to `{}`; boundaries: {}; ranges: {}.\n\n",
                relation.kind,
                relation.target,
                relation
                    .boundary_map
                    .iter()
                    .map(|m| format!(
                        "{}→{}",
                        m.source,
                        m.target.map_or_else(|| "elided".to_owned(), |n| n.to_string())
                    ))
                    .collect::<Vec<_>>()
                    .join(", "),
                relation
                    .ranges
                    .iter()
                    .map(|m| format!("{}..{} `{}`", m.source_start, m.source_end, m.disposition))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !case.lines.is_empty() {
            out.push_str("| Row | Start | Content end | Separator end | Kind |\n| ---: | ---: | ---: | ---: | --- |\n");
            for (index, line) in case.lines.iter().enumerate() {
                out.push_str(&format!(
                    "| {index} | {} | {} | {} | {} |\n",
                    line.start, line.content_end, line.separator_end, line.separator
                ));
            }
            out.push('\n');
        }
        if !case.wire.is_empty() {
            out.push_str("| Direction | Encoding | Line | Column | Disposition | Byte |\n| --- | --- | ---: | ---: | --- | ---: |\n");
            for fact in &case.wire {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} | {} |\n",
                    fact.direction,
                    fact.encoding,
                    fact.line,
                    fact.column,
                    fact.disposition,
                    fact.byte.map_or_else(|| "—".to_owned(), |n| n.to_string())
                ));
            }
            out.push('\n');
        }
        if !case.parser_queries.is_empty() {
            let selected: Vec<_> = case
                .parser_queries
                .iter()
                .filter(|query| {
                    query.disposition != "exact" || (query.row == 0 && query.column <= 8)
                })
                .take(12)
                .collect();
            out.push_str(&format!(
                "Parser point controls (showing first {} of {}; full facts in machine authority): ",
                selected.len(),
                case.parser_queries.len()
            ));
            out.push_str(
                &selected
                    .iter()
                    .map(|query| {
                        format!("`{}:{}` `{}`", query.row, query.column, query.disposition)
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push_str(".\n\n");
        }
        if !case.refusals.is_empty() {
            out.push_str("Refusals: ");
            out.push_str(
                &case
                    .refusals
                    .iter()
                    .map(|fact| format!("`{}` → `{}`", fact.kind, fact.disposition))
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            out.push_str(".\n\n");
        }
    }
    let last_content = out.trim_end_matches('\n').len();
    out.truncate(last_content);
    out.push('\n');
    out
}

/// Fail closed on byte identity, domain separation, completeness, and facts.
pub fn validate(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema_version != SCHEMA_VERSION || manifest.policy_id != POLICY_ID {
        return Err("manifest schema or policy identity mismatch".into());
    }
    let actual_ids: Vec<_> = manifest.cases.iter().map(|case| case.id.as_str()).collect();
    if actual_ids != REQUIRED_CASE_IDS {
        return Err("required versioned case inventory differs".into());
    }
    let mut ids = BTreeSet::new();
    for (index, case) in manifest.cases.iter().enumerate() {
        if !ids.insert(case.id.as_str())
            || case.raw_identity != "literal-hex/v1"
            || case.tags.is_empty()
        {
            return Err(format!("{}: duplicate ID, raw identity, or tags", case.id));
        }
        if case.exhaustive == matches!(case.id.as_str(), "large_line" | "many_lines") {
            return Err(format!("{}: exhaustive coverage class differs", case.id));
        }
        let (tagged_id, required_tags) = REQUIRED_TAGS
            .get(index)
            .ok_or_else(|| "required tag inventory incomplete".to_string())?;
        if *tagged_id != case.id.as_str()
            || !case.tags.iter().map(String::as_str).eq(required_tags.iter().copied())
        {
            return Err(format!("{}: required tag inventory differs", case.id));
        }
        let bytes = raw_bytes(case)?;
        let digest: String =
            Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        if bytes.len() != case.byte_len || digest != case.sha256 {
            return Err(format!("{}: byte length/digest mismatch", case.id));
        }
        match (case.decode.as_str(), std::str::from_utf8(&bytes)) {
            ("invalid_utf8_ingress", Err(decode_error))
                if case.subject.is_none()
                    && case.relation.is_none()
                    && case.scalars.is_empty()
                    && case.lines.is_empty()
                    && case.boundaries.is_empty()
                    && case.parser_points.is_empty()
                    && case.parser_queries.is_empty()
                    && case.wire.is_empty()
                    && case.refusals.is_empty()
                    && case.chunks.is_empty() =>
            {
                let start = decode_error.valid_up_to();
                let end = start + decode_error.error_len().unwrap_or(bytes.len() - start);
                let kind = if decode_error.error_len().is_none() {
                    "truncated"
                } else if bytes.get(start).is_some_and(|lead| (0xc2..=0xf4).contains(lead)) {
                    "invalid_continuation"
                } else {
                    "invalid_leading"
                };
                if !case
                    .decode_error
                    .as_ref()
                    .is_some_and(|fact| fact.kind == kind && fact.start == start && fact.end == end)
                {
                    return Err(format!("{}: decoder error class/span differs", case.id));
                }
                continue;
            }
            ("invalid_utf8_ingress", Err(_)) => {
                return Err(format!("{}: invalid UTF-8 ingress facts differ", case.id));
            }
            ("valid_utf8", Ok(source)) if case.subject.as_deref() == Some(case.id.as_str()) => {
                if case.decode_error.is_some() {
                    return Err(format!("{}: valid source has decoder error", case.id));
                }
                validate_text(case, source)?
            }
            _ => return Err(format!("{}: decode/source-domain mismatch", case.id)),
        }
    }
    for case in &manifest.cases {
        let declared = REQUIRED_RELATIONS.iter().find(|(id, _, _)| *id == case.id);
        match (declared, &case.relation) {
            (Some((_, kind, target)), Some(relation))
                if relation.kind == *kind && relation.target == *target => {}
            (None, None) => {}
            _ => return Err(format!("{}: required source relation differs", case.id)),
        }
        if let Some(relation) = &case.relation {
            let target = manifest
                .cases
                .iter()
                .find(|candidate| candidate.id == relation.target)
                .ok_or_else(|| format!("{}: missing relation target", case.id))?;
            let source_bytes = raw_bytes(case)?;
            let target_bytes = raw_bytes(target)?;
            let prefix = match relation.kind.as_str() {
                "identity" => 0,
                "strip_leading_bom" if source_bytes.starts_with(&[0xef, 0xbb, 0xbf]) => 3,
                _ => return Err(format!("{}: invalid relation kind/source", case.id)),
            };
            if source_bytes.get(prefix..) != Some(target_bytes.as_slice()) {
                return Err(format!("{}: relation bytes differ", case.id));
            }
            if relation.boundary_map.len() != source_bytes.len() + 1
                || relation.boundary_map.iter().enumerate().any(|(boundary, mapping)| {
                    mapping.source != boundary
                        || mapping.target
                            != if boundary < prefix { None } else { Some(boundary - prefix) }
                })
            {
                return Err(format!("{}: relation boundary map differs", case.id));
            }
            let expected_ranges: Vec<(usize, usize, Option<usize>, Option<usize>, &str)> =
                if prefix == 0 {
                    vec![(0, source_bytes.len(), Some(0), Some(source_bytes.len()), "exact")]
                } else {
                    let mut ranges = vec![
                        (0, prefix, None, None, "elided"),
                        (prefix, prefix, Some(0), Some(0), "exact"),
                        (prefix, source_bytes.len(), Some(0), Some(target_bytes.len()), "exact"),
                    ];
                    if let Some(&cross_end) =
                        case.boundaries.iter().find(|&&boundary| boundary > prefix)
                    {
                        ranges.push((
                            0,
                            cross_end,
                            Some(0),
                            Some(cross_end - prefix),
                            "crosses_elided_prefix",
                        ));
                    }
                    ranges
                };
            if relation.ranges.len() != expected_ranges.len()
                || relation.ranges.iter().zip(expected_ranges).any(|(range, expected)| {
                    (
                        range.source_start,
                        range.source_end,
                        range.target_start,
                        range.target_end,
                        range.disposition.as_str(),
                    ) != expected
                })
            {
                return Err(format!("{}: relation range facts differ", case.id));
            }
        }
    }
    Ok(())
}

fn validate_text(case: &Case, source: &str) -> Result<(), String> {
    let error = |detail: &str| format!("{}: {detail}", case.id);
    if case.scalars != source.chars().map(u32::from).collect::<Vec<_>>() {
        return Err(error("scalar sequence differs"));
    }
    let boundaries: Vec<usize> =
        source.char_indices().map(|(byte, _)| byte).chain(std::iter::once(source.len())).collect();
    let expected_boundaries = if case.exhaustive {
        boundaries
    } else {
        let candidates = [0, 1, source.len() / 2, source.len().saturating_sub(1), source.len()];
        candidates
            .into_iter()
            .filter(|&byte| source.is_char_boundary(byte))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    if case.boundaries != expected_boundaries {
        return Err(error("missing/invalid scalar boundary"));
    }
    let mut expected_lines = Vec::new();
    let mut start = 0;
    for (byte, value) in source.bytes().enumerate() {
        if value == b'\n' {
            let crlf = byte > start && source.as_bytes().get(byte - 1) == Some(&b'\r');
            expected_lines.push((
                start,
                if crlf { byte - 1 } else { byte },
                byte + 1,
                if crlf { "crlf" } else { "lf" },
            ));
            start = byte + 1;
        }
    }
    expected_lines.push((start, source.len(), source.len(), "none"));
    if case.lines.len() != expected_lines.len()
        || case.lines.iter().zip(&expected_lines).any(|(actual, expected)| {
            (actual.start, actual.content_end, actual.separator_end, actual.separator.as_str())
                != *expected
        })
    {
        return Err(error("LF line records differ"));
    }
    if case.parser_points.len() != expected_boundaries.len()
        || case.parser_points.iter().zip(&expected_boundaries).any(|(point, &byte)| {
            let row = expected_lines
                .partition_point(|(_, _, end, _)| *end <= byte)
                .min(expected_lines.len() - 1);
            point.byte != byte || point.row != row || point.column != byte - expected_lines[row].0
        })
    {
        return Err(error("parser points differ or incomplete"));
    }
    let mut expected_queries: BTreeSet<_> = case
        .parser_points
        .iter()
        .map(|point| (point.row, point.column, "exact", Some(point.byte)))
        .collect();
    for (row, &(start, _, separator_end, separator)) in expected_lines.iter().enumerate() {
        if separator != "none" {
            expected_queries.insert((row, separator_end - start, "old_row_noncanonical", None));
        }
        for byte in start..separator_end {
            if !source.is_char_boundary(byte) {
                expected_queries.insert((row, byte - start, "invalid_byte_boundary", None));
            }
        }
    }
    expected_queries.insert((expected_lines.len(), 0, "invalid_row", None));
    let actual_queries: BTreeSet<_> = case
        .parser_queries
        .iter()
        .map(|query| (query.row, query.column, query.disposition.as_str(), query.byte))
        .collect();
    if actual_queries.len() != case.parser_queries.len() || actual_queries != expected_queries {
        return Err(error("parser point queries differ or incomplete"));
    }
    let mut expected = Vec::new();
    let mut utf16_lengths = Vec::with_capacity(expected_lines.len());
    for (row, &(start, content_end, _, _)) in expected_lines.iter().enumerate() {
        let content = &source[start..content_end];
        let mut utf16_column = 0;
        let mut utf16_boundaries = BTreeSet::new();
        for (offset, ch) in content.char_indices().chain(std::iter::once((content.len(), '\0'))) {
            utf16_boundaries.insert(utf16_column);
            expected.push(("utf-8", "incoming", row, offset, "exact", Some(start + offset)));
            expected.push(("utf-16", "incoming", row, utf16_column, "exact", Some(start + offset)));
            expected.push(("utf-8", "outgoing", row, offset, "exact", Some(start + offset)));
            expected.push(("utf-16", "outgoing", row, utf16_column, "exact", Some(start + offset)));
            if offset < content.len() {
                utf16_column += ch.len_utf16();
            }
        }
        for column in 0..content.len() {
            if !content.is_char_boundary(column) {
                expected.push((
                    "utf-8",
                    "incoming",
                    row,
                    column,
                    "invalid_code_unit_boundary",
                    None,
                ));
            }
        }
        let utf16_len = utf16_column;
        utf16_lengths.push(utf16_len);
        for column in 0..utf16_len {
            if !utf16_boundaries.contains(&column) {
                expected.push((
                    "utf-16",
                    "incoming",
                    row,
                    column,
                    "invalid_code_unit_boundary",
                    None,
                ));
            }
        }
        expected.push((
            "utf-8",
            "incoming",
            row,
            content.len() + 1,
            "line_end_normalized",
            Some(content_end),
        ));
        expected.push((
            "utf-16",
            "incoming",
            row,
            utf16_len + 1,
            "line_end_normalized",
            Some(content_end),
        ));
    }
    expected.push(("utf-8", "incoming", expected_lines.len(), 0, "invalid_line", None));
    expected.push(("utf-16", "incoming", expected_lines.len(), 0, "invalid_line", None));
    for (row, (&(start, content_end, separator_end, _), &utf16_len)) in
        expected_lines.iter().zip(&utf16_lengths).enumerate()
    {
        if separator_end > content_end + 1 {
            expected.push((
                "utf-8",
                "outgoing",
                row,
                content_end + 1 - start,
                "invalid_separator_interior",
                None,
            ));
            expected.push((
                "utf-16",
                "outgoing",
                row,
                utf16_len + 1,
                "invalid_separator_interior",
                None,
            ));
        }
    }
    let actual: BTreeSet<_> = case
        .wire
        .iter()
        .map(|fact| {
            (
                fact.encoding.as_str(),
                fact.direction.as_str(),
                fact.line,
                fact.column,
                fact.disposition.as_str(),
                fact.byte,
            )
        })
        .collect();
    let expected: BTreeSet<_> = expected
        .into_iter()
        .filter(|fact| {
            case.exhaustive || {
                let &(encoding, direction, row, column, _, _) = fact;
                let selected_row = row == 0
                    || row == expected_lines.len() / 2
                    || row == expected_lines.len().saturating_sub(1)
                    || row == expected_lines.len();
                if !selected_row {
                    return false;
                }
                let Some(&(start, content_end, _, _)) = expected_lines.get(row) else {
                    return true;
                };
                let Some(&utf16_len) = utf16_lengths.get(row) else {
                    return false;
                };
                let end = if encoding == "utf-8" { content_end - start } else { utf16_len };
                let mid = end / 2;
                let _ = direction;
                column == 0 || column == mid || column == end || column == end + 1
            }
        })
        .collect();
    if actual.len() != case.wire.len() || actual != expected {
        return Err(error("wire facts differ, duplicate, or incomplete"));
    }
    let mut expected_refusals = vec![
        ("equal_range", Some(0), Some(0), "exact"),
        ("source_byte_out_of_bounds", Some(source.len() + 1), None, "invalid_source_byte"),
        ("wrong_source", None, None, "source_mismatch"),
        ("wrong_signature", None, None, "signature_mismatch"),
        ("wrong_schema", None, None, "schema_mismatch"),
        ("overflow_resource", None, None, "overflow_or_resource_boundary"),
        ("instrument_failure", None, None, "instrument_failure"),
    ];
    if !source.is_empty() {
        expected_refusals.push(("reversed_range", Some(1), Some(0), "invalid_range_order"));
    }
    if let Some(line) = case.lines.iter().find(|line| line.separator == "crlf") {
        expected_refusals.push((
            "separator_interior",
            Some(line.content_end + 1),
            None,
            "invalid_newline_boundary",
        ));
    }
    if let Some(byte) = (0..source.len()).find(|&byte| !source.is_char_boundary(byte)) {
        expected_refusals.push(("scalar_interior", Some(byte), None, "invalid_code_unit_boundary"));
    }
    let actual_refusals: BTreeSet<_> = case
        .refusals
        .iter()
        .map(|fact| (fact.kind.as_str(), fact.start, fact.end, fact.disposition.as_str()))
        .collect();
    if actual_refusals.len() != case.refusals.len()
        || actual_refusals != expected_refusals.into_iter().collect()
    {
        return Err(error("range/source/harness refusal facts differ or incomplete"));
    }
    if case.chunks != ["contiguous", "all_byte_cuts", "empty_chunks"] {
        return Err(error("chunk partition expectations incomplete"));
    }
    Ok(())
}

/// Focused discriminators co-located with the seams they pin.
///
/// The crate's cross-crate mutation-control suites (`tests/integrity.rs` and
/// the `perl-position-tracking` parity test) exercise this validator through
/// the public `load`/`validate` entry points from integration-test targets,
/// which the static exposure analysis cannot trace into these definitions.
/// These `#[cfg(test)]` tests reach the same behavior from inside the module
/// with exact-value assertions so every seam carries a locally visible
/// discriminator. They complement, and never replace, the checked-data
/// suites: the corpus itself stays the authority under test.
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// The checked-in authority parsed but not yet validated.
    fn corpus() -> Manifest {
        let contents = fs::read_to_string(repo_root().join(MANIFEST_PATH)).unwrap();
        serde_json::from_str(&contents).unwrap()
    }

    /// The checked-in authority after its fail-closed validation.
    fn checked() -> Manifest {
        let manifest = corpus();
        validate(&manifest).unwrap();
        manifest
    }

    fn case<'a>(manifest: &'a Manifest, id: &str) -> &'a Case {
        manifest.cases.iter().find(|case| case.id == id).unwrap()
    }

    fn case_mut<'a>(manifest: &'a mut Manifest, id: &str) -> &'a mut Case {
        manifest.cases.iter_mut().find(|case| case.id == id).unwrap()
    }

    fn rejected(manifest: &Manifest, expected: &str) {
        let failure = validate(manifest).expect_err("mutated manifest passed validate");
        assert_eq!(failure, expected, "validate rejected with a different error");
    }

    fn rejected_text(case: &Case, source: &str, expected: &str) {
        let failure = validate_text(case, source).expect_err("mutated case passed validate_text");
        assert_eq!(failure, expected, "validate_text rejected with a different error");
    }

    // ── Declaration pinning ──────────────────────────────────────────────

    #[test]
    fn authority_constants_pin_the_versioned_contract_identities() {
        assert_eq!(SCHEMA_VERSION, "position-fixtures/v1");
        assert_eq!(POLICY_ID, "lf-source-lines/v1");
        assert_eq!(
            MANIFEST_PATH,
            "crates/perl-position-fixtures/fixtures/position-fixtures.v1.json"
        );
        assert_eq!(PROJECTION_PATH, "docs/generated/position-fixtures.v1.md");
    }

    #[test]
    fn case_literal_round_trips_every_declared_field() {
        let case = Case {
            id: "pinned".into(),
            tags: vec!["basic".into()],
            raw_identity: "literal-hex/v1".into(),
            raw_hex: "616263".into(),
            exhaustive: true,
            byte_len: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            decode: "valid_utf8".into(),
            decode_error: None,
            scalars: vec![97, 98, 99],
            subject: Some("pinned".into()),
            relation: None,
            lines: vec![Line {
                start: 0,
                content_end: 3,
                separator_end: 3,
                separator: "none".into(),
            }],
            boundaries: vec![0, 1, 2, 3],
            parser_points: vec![ParserPoint { byte: 0, row: 0, column: 0 }],
            parser_queries: vec![ParserQuery {
                row: 0,
                column: 0,
                disposition: "exact".into(),
                byte: Some(0),
            }],
            wire: vec![WireFact {
                encoding: "utf-8".into(),
                direction: "incoming".into(),
                line: 0,
                column: 0,
                disposition: "exact".into(),
                byte: Some(0),
            }],
            refusals: vec![RefusalFact {
                kind: "equal_range".into(),
                start: Some(0),
                end: Some(0),
                disposition: "exact".into(),
            }],
            chunks: vec!["contiguous".into()],
        };
        assert_eq!(case.id, "pinned");
        assert_eq!(case.tags, ["basic"]);
        assert_eq!(case.raw_identity, "literal-hex/v1");
        assert_eq!(case.raw_hex, "616263");
        assert!(case.exhaustive);
        assert_eq!(case.byte_len, 3);
        assert_eq!(case.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(case.decode, "valid_utf8");
        assert!(case.decode_error.is_none());
        assert_eq!(case.scalars, [97, 98, 99]);
        assert_eq!(case.subject.as_deref(), Some("pinned"));
        assert!(case.relation.is_none());
        assert_eq!(case.lines.len(), 1);
        assert_eq!(
            (case.lines[0].start, case.lines[0].content_end, case.lines[0].separator_end),
            (0, 3, 3)
        );
        assert_eq!(case.lines[0].separator, "none");
        assert_eq!(case.boundaries, [0, 1, 2, 3]);
        assert_eq!(case.parser_points.len(), 1);
        assert_eq!(
            (case.parser_points[0].byte, case.parser_points[0].row, case.parser_points[0].column),
            (0, 0, 0)
        );
        assert_eq!(case.parser_queries.len(), 1);
        assert_eq!(case.parser_queries[0].byte, Some(0));
        assert_eq!(case.parser_queries[0].disposition, "exact");
        assert_eq!(case.wire.len(), 1);
        assert_eq!(case.wire[0].encoding, "utf-8");
        assert_eq!(case.wire[0].direction, "incoming");
        assert_eq!(case.refusals.len(), 1);
        assert_eq!(case.refusals[0].kind, "equal_range");
        assert_eq!(case.refusals[0].start, Some(0));
        assert_eq!(case.refusals[0].end, Some(0));
        assert_eq!(case.chunks, ["contiguous"]);
    }

    #[test]
    fn decode_error_relation_and_wire_literals_round_trip_their_fields() {
        let error = DecodeError { kind: "truncated".into(), start: 0, end: 1 };
        assert_eq!((error.kind.as_str(), error.start, error.end), ("truncated", 0, 1));
        let relation = Relation {
            target: "ascii_for_bom".into(),
            kind: "strip_leading_bom".into(),
            boundary_map: vec![BoundaryMap { source: 0, target: None }],
            ranges: vec![RangeMap {
                source_start: 0,
                source_end: 3,
                target_start: None,
                target_end: None,
                disposition: "elided".into(),
            }],
        };
        assert_eq!(relation.target, "ascii_for_bom");
        assert_eq!(relation.kind, "strip_leading_bom");
        assert_eq!(relation.boundary_map[0].source, 0);
        assert_eq!(relation.boundary_map[0].target, None);
        assert_eq!(
            (
                relation.ranges[0].source_start,
                relation.ranges[0].source_end,
                relation.ranges[0].target_start,
                relation.ranges[0].target_end,
                relation.ranges[0].disposition.as_str(),
            ),
            (0, 3, None, None, "elided")
        );
        let wire = WireFact {
            encoding: "utf-16".into(),
            direction: "outgoing".into(),
            line: 2,
            column: 5,
            disposition: "invalid_code_unit_boundary".into(),
            byte: None,
        };
        assert_eq!(wire.encoding, "utf-16");
        assert_eq!(wire.direction, "outgoing");
        assert_eq!(wire.line, 2);
        assert_eq!(wire.column, 5);
        assert_eq!(wire.byte, None);
    }

    // ── raw_bytes ────────────────────────────────────────────────────────

    #[test]
    fn raw_bytes_decodes_even_ascii_hex_to_the_exact_byte_sequence() {
        let mut case = case(&checked(), "lf").clone();
        case.raw_hex = "616263".into();
        assert_eq!(raw_bytes(&case).unwrap(), b"abc");
        case.raw_hex = "610a62".into();
        assert_eq!(raw_bytes(&case).unwrap(), b"a\nb");
        case.raw_hex = String::new();
        assert_eq!(raw_bytes(&case).unwrap(), b"");
    }

    #[test]
    fn raw_bytes_rejects_odd_length_and_non_hex_input_with_the_typed_error() {
        let case = case(&checked(), "lf").clone();
        let mut odd = case.clone();
        odd.raw_hex = "61626".into();
        assert_eq!(raw_bytes(&odd).unwrap_err(), "lf: raw_hex is not even-length ASCII hex");
        let mut non_hex = case;
        non_hex.raw_hex = "zz".into();
        assert_eq!(raw_bytes(&non_hex).unwrap_err(), "lf: raw_hex is not even-length ASCII hex");
    }

    // ── load and the checked authority ───────────────────────────────────

    #[test]
    fn load_returns_the_checked_authority_in_stable_id_order() {
        let validated = load(&repo_root()).unwrap();
        let ids: Vec<_> = validated.cases().iter().map(|case| case.id.as_str()).collect();
        assert_eq!(ids.len(), 45);
        assert_eq!(ids, REQUIRED_CASE_IDS);
        assert_eq!(ids.first().copied(), Some("ascii"));
        assert_eq!(ids.last().copied(), Some("vt"));
    }

    #[test]
    fn select_filters_by_exact_id_and_tag_and_stays_empty_for_unknown_filters() {
        let validated = load(&repo_root()).unwrap();
        let by_id = validated.select("ascii");
        assert_eq!(by_id.len(), 1);
        assert_eq!(by_id[0].id, "ascii");
        let small = validated.select("small");
        assert_eq!(small.len(), 34);
        assert!(small.iter().all(|case| case.tags.iter().any(|tag| tag == "small")));
        let ingress = validated.select("ingress");
        assert_eq!(ingress.len(), 9);
        assert!(ingress.iter().all(|case| case.decode == "invalid_utf8_ingress"));
        assert!(validated.select("no-such-id-or-tag").is_empty());
    }

    #[test]
    fn load_names_the_missing_authority_path_in_its_error() {
        let missing_root = repo_root().join("does-not-exist");
        let error = load(&missing_root).map(|_| ()).unwrap_err();
        let expected_path = missing_root.join(MANIFEST_PATH);
        assert!(error.starts_with(&expected_path.display().to_string()));
        assert!(error.contains("position-fixtures.v1.json"));
    }

    #[test]
    fn load_fails_closed_on_authority_bytes_that_are_not_json() {
        let temp = std::env::temp_dir()
            .join(format!("perl-position-fixtures-load-{}", std::process::id()));
        let fixture_dir = temp.join("crates/perl-position-fixtures/fixtures");
        fs::create_dir_all(&fixture_dir).unwrap();
        fs::write(fixture_dir.join("position-fixtures.v1.json"), "not json").unwrap();
        let error = load(&temp).map(|_| ()).unwrap_err();
        let _ = fs::remove_dir_all(&temp);
        assert!(error.contains("expected ident"), "unexpected error: {error}");
    }

    // ── validate: fail-closed admission ──────────────────────────────────

    #[test]
    fn validate_accepts_the_checked_authority_wholesale() {
        assert_eq!(validate(&corpus()), Ok(()));
    }

    #[test]
    fn validate_rejects_a_manifest_whose_schema_or_policy_identity_differs() {
        let mut manifest = checked();
        manifest.schema_version = "position-fixtures/v2".into();
        rejected(&manifest, "manifest schema or policy identity mismatch");
        let mut policy = checked();
        policy.policy_id = "lf-source-lines/v2".into();
        rejected(&policy, "manifest schema or policy identity mismatch");
    }

    #[test]
    fn validate_rejects_a_manifest_whose_case_inventory_differs() {
        let mut dropped = checked();
        dropped.cases.pop();
        rejected(&dropped, "required versioned case inventory differs");
        let mut reordered = checked();
        reordered.cases.swap(0, 1);
        rejected(&reordered, "required versioned case inventory differs");
    }

    #[test]
    fn validate_rejects_wrong_raw_identity_or_empty_tags() {
        let mut identity = checked();
        case_mut(&mut identity, "ascii").raw_identity = "derived-hex/v9".into();
        rejected(&identity, "ascii: duplicate ID, raw identity, or tags");
        let mut untagged = checked();
        case_mut(&mut untagged, "ascii").tags.clear();
        rejected(&untagged, "ascii: duplicate ID, raw identity, or tags");
    }

    #[test]
    fn validate_rejects_the_wrong_exhaustive_coverage_class() {
        let mut demoted = checked();
        case_mut(&mut demoted, "astral").exhaustive = false;
        rejected(&demoted, "astral: exhaustive coverage class differs");
        let mut promoted = checked();
        case_mut(&mut promoted, "large_line").exhaustive = true;
        rejected(&promoted, "large_line: exhaustive coverage class differs");
    }

    #[test]
    fn validate_rejects_bytes_that_do_not_match_the_declared_length_or_digest() {
        let mut payload = checked();
        case_mut(&mut payload, "lf").raw_hex = "610a63".into();
        rejected(&payload, "lf: byte length/digest mismatch");
        let mut length = checked();
        case_mut(&mut length, "lf").byte_len = 2;
        rejected(&length, "lf: byte length/digest mismatch");
    }

    #[test]
    fn validate_rejects_invalid_ingress_cases_whose_typed_facts_differ() {
        let mut extra = checked();
        case_mut(&mut extra, "invalid_truncated_start").chunks.push("all_byte_cuts".into());
        rejected(&extra, "invalid_truncated_start: invalid UTF-8 ingress facts differ");
        let mut presented = checked();
        case_mut(&mut presented, "invalid_truncated_start").subject = Some("e2".into());
        rejected(&presented, "invalid_truncated_start: invalid UTF-8 ingress facts differ");
        let mut class = checked();
        case_mut(&mut class, "invalid_truncated_start").decode_error =
            Some(DecodeError { kind: "invalid_leading".into(), start: 0, end: 1 });
        rejected(&class, "invalid_truncated_start: decoder error class/span differs");
    }

    #[test]
    fn validate_rejects_a_valid_source_declared_with_a_decoder_error() {
        let mut manifest = checked();
        case_mut(&mut manifest, "ascii").decode_error =
            Some(DecodeError { kind: "truncated".into(), start: 0, end: 1 });
        rejected(&manifest, "ascii: valid source has decoder error");
    }

    #[test]
    fn validate_rejects_cases_whose_decode_and_source_domain_disagree() {
        let mut manifest = checked();
        case_mut(&mut manifest, "ascii").decode = "invalid_utf8_ingress".into();
        rejected(&manifest, "ascii: decode/source-domain mismatch");
    }

    #[test]
    fn validate_rejects_missing_or_unexpected_required_source_relations() {
        let mut missing = checked();
        case_mut(&mut missing, "bom_ascii").relation = None;
        rejected(&missing, "bom_ascii: required source relation differs");
        let mut unexpected = checked();
        case_mut(&mut unexpected, "vt").relation = Some(Relation {
            target: "vt".into(),
            kind: "identity".into(),
            boundary_map: vec![],
            ranges: vec![],
        });
        rejected(&unexpected, "vt: required source relation differs");
    }

    #[test]
    fn validate_rejects_relations_whose_boundary_or_range_facts_differ() {
        let mut boundary = checked();
        case_mut(&mut boundary, "bom_ascii").relation.as_mut().unwrap().boundary_map.pop();
        rejected(&boundary, "bom_ascii: relation boundary map differs");
        let mut range = checked();
        case_mut(&mut range, "bom_ascii").relation.as_mut().unwrap().ranges.pop();
        rejected(&range, "bom_ascii: relation range facts differ");
    }

    // ── validate_text: per-domain fact discriminators ────────────────────

    #[test]
    fn validate_text_accepts_every_checked_valid_case_against_its_decoded_source() {
        let manifest = checked();
        let valid = manifest.cases.iter().filter(|case| case.decode == "valid_utf8");
        let mut count = 0;
        for case in valid {
            let bytes = raw_bytes(case).unwrap();
            let source = std::str::from_utf8(&bytes).unwrap();
            assert_eq!(validate_text(case, source), Ok(()), "case {}", case.id);
            count += 1;
        }
        assert_eq!(count, 36);
    }

    #[test]
    fn validate_text_accepts_the_lf_case_against_its_literal_source() {
        let manifest = checked();
        let lf = case(&manifest, "lf");
        assert_eq!(lf.lines.len(), 2);
        assert_eq!(validate_text(lf, "a\nb"), Ok(()));
    }

    #[test]
    fn validate_text_rejects_a_mutated_scalar_sequence_or_boundary_set() {
        let manifest = checked();
        let mut scalars = case(&manifest, "lf").clone();
        scalars.scalars.pop();
        rejected_text(&scalars, "a\nb", "lf: scalar sequence differs");
        let mut boundaries = case(&manifest, "lf").clone();
        boundaries.boundaries.pop();
        rejected_text(&boundaries, "a\nb", "lf: missing/invalid scalar boundary");
    }

    #[test]
    fn validate_text_rejects_mutated_lf_line_records() {
        let manifest = checked();
        let mut shifted = case(&manifest, "lf").clone();
        shifted.lines[0].content_end = 2;
        rejected_text(&shifted, "a\nb", "lf: LF line records differ");
        let mut truncated = case(&manifest, "lf").clone();
        truncated.lines.pop();
        rejected_text(&truncated, "a\nb", "lf: LF line records differ");
    }

    #[test]
    fn validate_text_rejects_mutated_parser_points() {
        let manifest = checked();
        let mut dropped = case(&manifest, "lf").clone();
        dropped.parser_points.pop();
        rejected_text(&dropped, "a\nb", "lf: parser points differ or incomplete");
        let mut moved = case(&manifest, "lf").clone();
        moved.parser_points[1].column = 0;
        rejected_text(&moved, "a\nb", "lf: parser points differ or incomplete");
    }

    #[test]
    fn validate_text_rejects_mutated_or_duplicated_parser_point_queries() {
        let manifest = checked();
        let mut dropped = case(&manifest, "lf").clone();
        dropped.parser_queries.pop();
        rejected_text(&dropped, "a\nb", "lf: parser point queries differ or incomplete");
        let mut duplicated = case(&manifest, "lf").clone();
        let exact = duplicated.parser_queries[0].clone();
        duplicated.parser_queries.push(exact);
        rejected_text(&duplicated, "a\nb", "lf: parser point queries differ or incomplete");
    }

    #[test]
    fn validate_text_rejects_mutated_or_duplicated_wire_facts() {
        let manifest = checked();
        let mut dropped = case(&manifest, "lf").clone();
        dropped.wire.pop();
        rejected_text(&dropped, "a\nb", "lf: wire facts differ, duplicate, or incomplete");
        let mut duplicated = case(&manifest, "lf").clone();
        let fact = duplicated.wire[0].clone();
        duplicated.wire.push(fact);
        rejected_text(&duplicated, "a\nb", "lf: wire facts differ, duplicate, or incomplete");
    }

    #[test]
    fn validate_text_rejects_mutated_or_duplicated_refusal_facts() {
        let manifest = checked();
        let mut dropped = case(&manifest, "lf").clone();
        dropped.refusals.pop();
        rejected_text(
            &dropped,
            "a\nb",
            "lf: range/source/harness refusal facts differ or incomplete",
        );
        let mut duplicated = case(&manifest, "lf").clone();
        let fact = duplicated.refusals[0].clone();
        duplicated.refusals.push(fact);
        rejected_text(
            &duplicated,
            "a\nb",
            "lf: range/source/harness refusal facts differ or incomplete",
        );
    }

    #[test]
    fn validate_text_rejects_incomplete_chunk_partition_expectations() {
        let manifest = checked();
        let mut chunks = case(&manifest, "lf").clone();
        chunks.chunks = vec!["contiguous".into()];
        rejected_text(&chunks, "a\nb", "lf: chunk partition expectations incomplete");
    }

    // ── explain and the generated projection ─────────────────────────────

    #[test]
    fn explain_returns_none_for_an_unknown_case_id() {
        let validated = load(&repo_root()).unwrap();
        assert!(validated.explain("no-such-case").is_none());
    }

    #[test]
    fn explain_renders_the_exact_literal_facts_for_the_empty_case() {
        let validated = load(&repo_root()).unwrap();
        assert_eq!(
            validated.explain("empty").unwrap(),
            "empty [basic, small] 0 bytes SHA-256 \
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\n\
             raw hex: \n\
             decode: valid_utf8\n\
             row 0: 0..0..0 none\n\
             incoming utf-8 0:0 -> exact Some(0)\n\
             outgoing utf-8 0:0 -> exact Some(0)\n\
             incoming utf-16 0:0 -> exact Some(0)\n\
             outgoing utf-16 0:0 -> exact Some(0)\n\
             incoming utf-8 0:1 -> line_end_normalized Some(0)\n\
             incoming utf-16 0:1 -> line_end_normalized Some(0)\n\
             incoming utf-8 1:0 -> invalid_line None\n\
             incoming utf-16 1:0 -> invalid_line None\n\
             parser 0:0 -> exact Some(0)\n\
             parser 1:0 -> invalid_row None\n\
             refusal equal_range Some(0)..Some(0) -> exact\n\
             refusal source_byte_out_of_bounds Some(1)..None -> invalid_source_byte\n\
             refusal wrong_source None..None -> source_mismatch\n\
             refusal wrong_signature None..None -> signature_mismatch\n\
             refusal wrong_schema None..None -> schema_mismatch\n\
             refusal overflow_resource None..None -> overflow_or_resource_boundary\n\
             refusal instrument_failure None..None -> instrument_failure\n"
        );
    }

    #[test]
    fn explain_renders_the_exact_decode_error_line_for_truncated_ingress() {
        let validated = load(&repo_root()).unwrap();
        assert_eq!(
            validated.explain("invalid_truncated_start").unwrap(),
            "invalid_truncated_start [invalid_utf8, ingress] 1 bytes SHA-256 \
             30a5bfa58e128af9e5a4955725d8ad26d4d574a537b58b7dc6d357acad578572\n\
             raw hex: e2\n\
             decode: invalid_utf8_ingress\n\
             decode error: truncated 0..1\n"
        );
    }

    #[test]
    fn explain_renders_wire_parser_and_refusal_rows_for_the_astral_case() {
        let validated = load(&repo_root()).unwrap();
        let rendered = validated.explain("astral").unwrap();
        assert!(rendered.starts_with(
            "astral [unicode, crlf, small, decisive] 9 bytes SHA-256 \
             587527b0f4f0a1573965d9e6c295f0ce1fd3250fc1e7d2bd81bb6db986782b40\n\
             raw hex: 78f09f9880790d0a7a\n\
             decode: valid_utf8\n"
        ));
        assert!(rendered.contains("row 0: 0..6..8 crlf\n"));
        assert!(rendered.contains("incoming utf-8 0:0 -> exact Some(0)\n"));
        assert!(rendered.contains("parser 0:7 -> exact Some(7)\n"));
        assert!(rendered.contains(
            "refusal separator_interior Some(7)..None -> \
                                   invalid_newline_boundary\n"
        ));
        assert!(rendered.contains(
            "refusal scalar_interior Some(2)..None -> \
                                   invalid_code_unit_boundary\n"
        ));
    }

    #[test]
    fn markdown_regenerates_the_checked_projection_byte_for_byte() {
        let validated = load(&repo_root()).unwrap();
        let projection = fs::read_to_string(repo_root().join(PROJECTION_PATH)).unwrap();
        assert_eq!(validated.markdown(), projection);
        assert_eq!(render_markdown(&corpus()), projection);
    }

    #[test]
    fn markdown_renders_relations_and_omits_absent_sections() {
        let validated = load(&repo_root()).unwrap();
        let rendered = validated.markdown();
        let ascii_header = "## `ascii`\n\nTags: basic, small. Coverage: \
             `all small-case boundaries`. Decode: `valid_utf8`. Bytes: 3. SHA-256: \
             `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.\n\n\
             Hex: `616263`\n\n";
        assert!(rendered.contains(ascii_header), "ascii section header changed");
        assert!(rendered.contains(
            "Relation: `identity` to `ascii`; boundaries: 0\u{2192}0, 1\u{2192}1, \
             2\u{2192}2, 3\u{2192}3; ranges: 0..3 `exact`."
        ));
        let lf_section =
            rendered.split("## `lfcr`").next().unwrap().split("## `lf`").nth(1).unwrap();
        assert!(!lf_section.contains("Relation:"));
        assert!(rendered.contains(
            "Relation: `strip_leading_bom` to `ascii_for_bom`; boundaries: \
             0\u{2192}elided, 1\u{2192}elided, 2\u{2192}elided, 3\u{2192}0, 4\u{2192}1, \
             5\u{2192}2, 6\u{2192}3; ranges: 0..3 `elided`, 3..3 `exact`, 3..6 `exact`, \
             0..4 `crosses_elided_prefix`."
        ));
        assert!(rendered.contains(
            "Parser point controls (showing first 10 of 12; full facts in machine authority):"
        ));
        assert!(rendered.ends_with('\n'));
        assert!(!rendered.ends_with("\n\n"));
    }
}
