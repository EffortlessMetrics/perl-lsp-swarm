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
