//! Deterministic checked-in subject registry for the native formatter pipeline
//! benches and counter canaries (#10302).
//!
//! Subjects are generated from fixed base units so every consumer (bench,
//! canary test, nightly receipt) observes byte-identical content. Each subject
//! pins exact identity — family, line-ending convention, indentation family,
//! scaling size, and an FNV-1a content digest computed with the same
//! constants and prefix as the production `FormatIdentity::content_digest` —
//! so #9327 representative-corpus identities can enroll through this registry
//! without schema movement, and per-subject identity rows can never be hidden
//! behind an aggregate average (NPC-008).
//!
//! This module must stay lint-clean: it is compiled into ordinary test
//! targets as well as the bench harness.

use perl_lsp_perltidy::native::{FormatIdentity, NativePipelineCounters, TypedFormatResult};
use serde_json::{Value, json};

/// Production default `FormatConfig::line_width`.
pub const LINE_WIDTH: usize = 100;

/// Scaling steps used by the N / 2N / 4N counter-shape canaries (units).
pub const SCALE_STEPS: [usize; 3] = [4, 8, 16];

/// Sizes enrolled for the nightly bench cohort (units).
pub const BENCH_UNIT_SIZES: [usize; 3] = [8, 32, 128];

/// Admitted subject families.
pub const FAMILIES: [&str; 5] = ["delimited", "statement", "opaque", "refusal", "no-change"];

/// Line-ending conventions exercised by the cohort.
pub const LINE_ENDINGS: [&str; 3] = ["lf", "crlf", "bare-cr"];

/// Indentation families exercised by the cohort.
pub const INDENTS: [&str; 3] = ["tabs", "spaces", "width-boundary"];

/// The one representative bench id required by the nightly `--expect-id`
/// integrity pin (`group/name` pair on disk).
pub const REPRESENTATIVE_BENCH_ID: &str = "native_pipeline_document/delimited_n32_lf_tabs";

/// Criterion group every subject bench is registered under.
pub const BENCH_GROUP: &str = "native_pipeline_document";

/// Engine label recorded in emitted subject identity rows.
pub const SUBJECT_ENGINE: &str = "native";

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Compute the production-compatible stable digest for `bytes`.
///
/// Mirrors the private `stable_digest` in `native/outcome.rs` (same FNV-1a
/// constants, same `source-v1` prefix) so subject digests can be compared
/// against `FormatIdentity::content_digest` byte-for-byte.
#[must_use]
pub fn source_digest(source: &str) -> String {
    let hash = source
        .as_bytes()
        .iter()
        .fold(FNV_OFFSET_BASIS, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME));
    format!("source-v1:{hash:016x}")
}

/// One deterministic scaling subject of the checked-in cohort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubjectSpec {
    /// Admitted family: `delimited`, `statement`, `opaque`, `refusal`, or
    /// `no-change`.
    pub family: &'static str,
    /// Line-ending convention: `lf`, `crlf`, or `bare-cr`.
    pub line_ending: &'static str,
    /// Indentation family: `tabs`, `spaces`, or `width-boundary`.
    pub indent: &'static str,
    /// Number of repeated base units (the scaling parameter N).
    pub units: usize,
}

impl SubjectSpec {
    /// Stable subject identifier used in bench names and receipt rows.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}_n{}_{}_{}", self.family, self.units, self.line_ending, self.indent)
    }

    /// Criterion on-disk benchmark id (`group/name`).
    #[must_use]
    pub fn bench_id(&self) -> String {
        format!("{BENCH_GROUP}/{}", self.id())
    }

    /// Exact source text of the subject.
    #[must_use]
    pub fn source(&self) -> String {
        let mut lines = Vec::new();
        for index in 0..self.units {
            lines.extend(family_unit_lines(self.family, index, self.indent));
        }
        join_lines(&lines, self.line_ending)
    }

    /// Number of physical lines a `split('\n')` view of the subject reports.
    /// `bare-cr` subjects are one physical line by construction.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.source().split('\n').count()
    }

    /// Production-compatible content digest of the subject source.
    #[must_use]
    pub fn content_digest(&self) -> String {
        source_digest(&self.source())
    }
}

/// Build the N / 2N / 4N scaling row triple for one family (LF, tabs).
#[must_use]
pub fn scaling_rows(family: &str) -> Option<[SubjectSpec; 3]> {
    let family = static_family(family)?;
    let [n, two_n, four_n] = SCALE_STEPS;
    Some([
        SubjectSpec { family, line_ending: "lf", indent: "tabs", units: n },
        SubjectSpec { family, line_ending: "lf", indent: "tabs", units: two_n },
        SubjectSpec { family, line_ending: "lf", indent: "tabs", units: four_n },
    ])
}

/// Build one line-ending variant row at a fixed size for one family.
#[must_use]
pub fn line_ending_row(family: &str, line_ending: &str, units: usize) -> Option<SubjectSpec> {
    let family = static_family(family)?;
    let line_ending = static_line_ending(line_ending)?;
    Some(SubjectSpec { family, line_ending, indent: "tabs", units })
}

/// Build one indentation variant row at a fixed size for one family.
#[must_use]
pub fn indent_row(family: &str, indent: &str, units: usize) -> Option<SubjectSpec> {
    let family = static_family(family)?;
    let indent = static_indent(indent)?;
    Some(SubjectSpec { family, line_ending: "lf", indent, units })
}

/// The full nightly bench cohort: every family at small/medium/large with the
/// default LF/tabs convention, plus medium-size line-ending and indentation
/// variants so receipts carry per-subject rows for the whole admitted matrix.
#[must_use]
pub fn bench_rows() -> Vec<SubjectSpec> {
    let mut rows = Vec::new();
    for family in FAMILIES {
        for units in BENCH_UNIT_SIZES {
            rows.push(SubjectSpec { family, line_ending: "lf", indent: "tabs", units });
        }
        for line_ending in ["crlf", "bare-cr"] {
            rows.push(SubjectSpec { family, line_ending, indent: "tabs", units: 32 });
        }
        for indent in ["spaces", "width-boundary"] {
            rows.push(SubjectSpec { family, line_ending: "lf", indent, units: 32 });
        }
    }
    rows
}

/// Emit one receipt identity row for `spec`.
///
/// `config_fingerprint` must be the production
/// `FormatIdentity::config_fingerprint` observed on the real typed path.
#[must_use]
pub fn identity_row(spec: &SubjectSpec, config_fingerprint: &str, toolchain: &str) -> Value {
    json!({
        "schema": perl_lsp_perltidy::COUNTER_SCHEMA_V1,
        "bench_group": BENCH_GROUP,
        "bench_name": spec.id(),
        "bench_id": spec.bench_id(),
        "subject": {
            "family": spec.family,
            "line_ending": spec.line_ending,
            "indent": spec.indent,
            "units": spec.units,
            "content_digest": spec.content_digest(),
            "line_count": spec.line_count(),
        },
        "engine": SUBJECT_ENGINE,
        "config_fingerprint": config_fingerprint,
        "toolchain": toolchain,
    })
}

/// Emit one receipt row with the counter snapshot captured by the production
/// typed pipeline for this exact subject.
#[must_use]
pub fn identity_row_with_counters(
    spec: &SubjectSpec,
    typed: &TypedFormatResult,
    toolchain: &str,
    run_id: &str,
    counters: &NativePipelineCounters,
) -> Value {
    let identity = &typed.outcome.identity;
    assert_eq!(
        identity.content_digest,
        spec.content_digest(),
        "production identity digest drift for {}",
        spec.id()
    );
    let mut row = identity_row_from_production_identity(spec, identity, toolchain);
    row["run_id"] = json!(run_id);
    row["counters"] = json!(counters);
    row["counters"]["schema"] = json!(counters.schema());
    row["counters"]["clock_tag"] = json!(counters.clock_tag());
    row
}

fn identity_row_from_production_identity(
    spec: &SubjectSpec,
    identity: &FormatIdentity,
    toolchain: &str,
) -> Value {
    json!({
        "schema": perl_lsp_perltidy::COUNTER_SCHEMA_V1,
        "bench_group": BENCH_GROUP,
        "bench_name": spec.id(),
        "bench_id": spec.bench_id(),
        "subject": {
            "family": spec.family,
            "line_ending": spec.line_ending,
            "indent": spec.indent,
            "units": spec.units,
            "content_digest": identity.content_digest,
            "line_count": spec.line_count(),
        },
        "engine": identity.actual_engine,
        "requested_mode": identity.requested_mode,
        "source_id": identity.source_id,
        "source_generation": identity.source_generation,
        "config_fingerprint": identity.config_fingerprint,
        "toolchain": toolchain,
    })
}

/// Toolchain/environment tag for receipt rows. The host-only fallback is for
/// local runs; CI supplies the exact rustc identity through
/// `NATIVE_PIPELINE_TOOLCHAIN_TAG`.
#[must_use]
pub fn toolchain_tag() -> String {
    if let Some(override_tag) =
        std::env::var("NATIVE_PIPELINE_TOOLCHAIN_TAG").ok().filter(|tag| !tag.is_empty())
    {
        return override_tag;
    }
    format!("rust-{}-{}-{}", std::env::consts::ARCH, std::env::consts::OS, std::env::consts::FAMILY)
}

fn static_family(family: &str) -> Option<&'static str> {
    FAMILIES.iter().copied().find(|candidate| *candidate == family)
}

fn static_line_ending(line_ending: &str) -> Option<&'static str> {
    LINE_ENDINGS.iter().copied().find(|candidate| *candidate == line_ending)
}

fn static_indent(indent: &str) -> Option<&'static str> {
    INDENTS.iter().copied().find(|candidate| *candidate == indent)
}

fn join_lines(lines: &[String], line_ending: &str) -> String {
    let separator = match line_ending {
        "crlf" => "\r\n",
        "bare-cr" => "\r",
        _ => "\n",
    };
    let mut source = lines.join(separator);
    if !source.is_empty() {
        source.push_str(separator);
    }
    source
}

fn family_unit_lines(family: &str, index: usize, indent: &str) -> Vec<String> {
    // Indices are zero-padded to a fixed width so unit content stays uniform
    // across the N / 2N / 4N scaling rows: a digit-width artifact would
    // otherwise masquerade as superlinear replacement growth.
    let tag = format!("{index:02}");
    let raw = match family {
        // Hash/list delimited layout: exercises the delimited group fit
        // decision (rendered flat when within the configured line width).
        "delimited" => vec![format!("my $config_{tag} = {{ name => 'unit-{tag}', size => 4 }};")],
        // Control-block layout: exercises the block render path.
        "statement" => vec![
            format!("if ( $flag_{tag} ) {{ return 1; }}"),
            format!("if ( $other_{tag} ) {{ return 2; }}"),
        ],
        // Parses cleanly, renders unchanged, and derives no edits.
        "opaque" => vec![
            format!("# opaque commentary {tag}: preserved verbatim by the native formatter"),
            format!("my %opaque_{tag} = map {{ $_ => 1 }} @items;"),
        ],
        // Parse-error flood: refuses at the source parse gate.
        "refusal" => vec![format!("my $broken_{tag} = ;")],
        // Already canonical under the default configuration.
        "no-change" => vec![format!("my $clean_{tag} = 42;")],
        _ => Vec::new(),
    };
    raw.iter().map(|line| apply_indent(line, indent)).collect()
}

fn apply_indent(line: &str, indent: &str) -> String {
    match indent {
        "tabs" => format!("\t{line}"),
        "spaces" => format!("    {line}"),
        "width-boundary" => {
            let used = line.chars().count();
            if used < LINE_WIDTH {
                let pad = LINE_WIDTH - used;
                let mut indented = " ".repeat(pad);
                indented.push_str(line);
                indented
            } else {
                line.to_string()
            }
        }
        _ => line.to_string(),
    }
}
