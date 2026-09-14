//! Validation for the actual-host Neovim activation/root envelope (#10502).
//!
//! `scripts/ux/neovim_activation_root_smoke.sh` runs the canonical Neovim
//! configuration against a real `perllsp` and emits one envelope describing,
//! per row, the native filetype, whether the canonical config actually
//! activated, and whether the selected workspace root controlled an observable
//! semantic result.
//!
//! This module owns the envelope's honesty rules rather than its content. In
//! particular it deliberately does **not** restate the root-marker list or the
//! activating filetypes: `scripts/ux/neovim/perllsp.lua` remains the only
//! executable authority for those, and the requirements below are derived from
//! whichever configuration the envelope recorded. What is fixed here is the
//! required denominator of rows and the set of contradictions a row may not
//! contain.

use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const SCHEMA_VERSION: &str = "neovim_activation_root_envelope.v1";

/// File families that must appear in every envelope, each bound to the fixture
/// it is about.
///
/// A missing row cannot be allowed to shrink the denominator silently, so the
/// population is fixed here rather than inferred from whatever the harness
/// happened to emit. Binding the fixture is what stops the opposite trick:
/// copying one passing row under every required identifier would otherwise
/// produce a complete-looking denominator out of a single observation.
pub const REQUIRED_FILE_FAMILIES: &[(&str, &str)] = &[
    ("adjacent.pod", "Doc.pod"),
    ("adjacent.xs", "Native.xs"),
    ("metadata.cpanfile", "cpanfile"),
    ("shebang.bin_tool", "bin/tool"),
    ("shebang.cgi", "handler.cgi"),
    ("shebang.fcgi", "handler.fcgi"),
    ("shebang.script_tool", "script/tool"),
    ("source.PL", "legacy.PL"),
    ("source.pl", "sample.pl"),
    ("source.pm", "Sample.pm"),
    ("source.psgi", "app.psgi"),
    ("source.t", "basic.t"),
    ("suffix_only.cgi", "plain.cgi"),
    ("template.ep", "view.ep"),
    ("template.mason", "view.mason"),
    ("template.tt", "template.tt"),
    ("template.tt2", "template.tt2"),
];

/// Root cells that must appear in every envelope, each bound to the marker
/// *and* the fixture root it is about.
///
/// The marker alone is not enough: three cells legitimately share `cpanfile`
/// and two share `.git`, so one proven row could otherwise be copied across the
/// identifiers that share its marker and the distinct conflict and isolation
/// scenarios would disappear while validation passed. The expected role is what
/// makes each required cell name its own scenario.
pub const REQUIRED_ROOT_CELLS: &[(&str, &str, &str)] = &[
    ("boundary.no_marker_single_file", "none", "observation_only"),
    ("conflict.competing_markers_at_depth", "cpanfile", "fixture:roots/depth-conflict/nested/deep"),
    ("conflict.nearest_perl_marker_beats_farther", "Makefile.PL", "fixture:roots/nearest-perl/sub"),
    ("conflict.perl_marker_beats_git", "cpanfile", "fixture:roots/perl-beats-git/app"),
    ("fallback.git_file_linked_worktree", ".git", "fixture:roots/worktree-linked"),
    ("fallback.git_only", ".git", "fixture:roots/git-only"),
    ("isolation.sibling_same_relative_path", "cpanfile", "fixture:roots/siblings/alpha"),
    ("marker.build_pl", "Build.PL", "fixture:roots/marker-build"),
    ("marker.dist_ini", "dist.ini", "fixture:roots/marker-dist"),
    ("marker.perl_lsp_toml", ".perl-lsp.toml", "fixture:roots/marker-dot"),
];

/// Root cells whose whole point is that an identically-spelled fact exists in a
/// competing parent or sibling root. These may never pass on `root_dir`
/// equality alone: they must name the facts they rejected.
pub const ISOLATION_ROOT_CELLS: &[&str] = &[
    "conflict.competing_markers_at_depth",
    "conflict.nearest_perl_marker_beats_farther",
    "conflict.perl_marker_beats_git",
    "fallback.git_file_linked_worktree",
    "isolation.sibling_same_relative_path",
];

const FILE_FAMILY_FIELDS: &[&str] = &[
    "attached",
    "config_eligible",
    "content_dependent",
    "disposition",
    "fixture",
    "language_id",
    "native_filetype",
    "opened_filetype",
    "override_applied",
    "reason",
];

const ROOT_FIELDS: &[&str] = &["actual_role", "expected_role", "marker", "root_match", "semantic"];

const SEMANTIC_FIELDS: &[&str] = &[
    "content_method",
    "definition_method",
    "expected_marker",
    "expected_target_role",
    "observed_marker",
    "observed_target_role",
    "outcome",
    "reason",
    "rejected_symbols",
];

/// Dispositions that assert the canonical configuration activated natively.
const ATTACHING_DISPOSITIONS: &[&str] = &["native_perl_and_attached"];

/// Dispositions that assert the canonical config deliberately leaves a family
/// inactive. Unlike `unsupported`, these are claims about the configuration
/// rather than about what the host can do.
const NON_ACTIVATING_POLICY_DISPOSITIONS: &[&str] =
    &["intentionally_adjacent_or_mixed", "native_nonperl_override_possible"];

/// Dispositions that require an explicit recorded reason.
const REASONED_DISPOSITIONS: &[&str] = &[
    "instrument_failed",
    "intentionally_adjacent_or_mixed",
    "native_nonperl_override_possible",
    "not_proven",
    "unsupported",
];

const ALL_DISPOSITIONS: &[&str] = &[
    "instrument_failed",
    "intentionally_adjacent_or_mixed",
    "native_nonperl_override_possible",
    "native_perl_activation_only",
    "native_perl_and_attached",
    "not_proven",
    "unsupported",
];

const SEMANTIC_OUTCOMES: &[&str] =
    &["instrument_failed", "not_applicable", "not_proven", "proven", "unsupported"];

/// Marker recorded by the boundary row that deliberately has no marker at all.
const BOUNDARY_MARKER: &str = "none";

/// The only outcome the observation cell may record: it neither proves nor
/// fails a root, because it asserts none.
const OBSERVATION_ONLY_OUTCOME: &str = "not_applicable";

/// The role the boundary row records instead of a root it never asserted.
const OBSERVATION_ONLY_ROLE: &str = "observation_only";

/// The filetype this envelope's Perl dispositions are about. Membership of the
/// configured activating list is a separate, weaker fact.
const PERL_FILETYPE: &str = "perl";

/// This schema describes one editor. A receipt from another host is not weaker
/// evidence, it is evidence about something else.
const HOST_FAMILY: &str = "neovim";

/// Server identities this envelope version can describe.
const SERVER_ROLES: &[&str] = &["candidate_build"];

/// The one required root cell that records an observation instead of asserting
/// a root. Every other required cell must actually reach `proven`.
const OBSERVATION_ONLY_ROOT_CELL: &str = "boundary.no_marker_single_file";

/// Dispositions that mean the run failed to establish the row, as opposed to
/// recording a deliberate policy about it.
const FAILED_DISPOSITIONS: &[&str] = &["instrument_failed", "not_proven"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvelopeValidationError(String);

impl EnvelopeValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for EnvelopeValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for EnvelopeValidationError {}

type Validated = Result<(), EnvelopeValidationError>;

pub fn validate_envelope(envelope: &Value) -> Validated {
    let root = as_object(envelope, "envelope")?;

    require_exact_string(root, "schema_version", SCHEMA_VERSION, "envelope.schema_version")?;
    let version = require_u64(root, "envelope_version", "envelope.envelope_version")?;
    if version != 1 {
        return Err(EnvelopeValidationError::new(format!(
            "envelope.envelope_version: expected `1`, found `{version}`"
        )));
    }
    reject_unknown_keys(
        root,
        &[
            "claim_boundary",
            "config",
            "envelope_version",
            "file_families",
            "host",
            "limitations",
            "roots",
            "schema_version",
            "server",
        ],
        "envelope",
    )?;

    validate_identity(root, "host", &["arch", "family", "os", "version"])?;
    validate_identity(root, "server", &["role", "sha256"])?;
    let config = validate_config(root)?;

    validate_file_families(root, &config)?;
    validate_roots(root, &config)?;

    let limitations = require_array(root, "limitations", "envelope.limitations")?;
    if limitations.is_empty() {
        return Err(EnvelopeValidationError::new(
            "envelope.limitations: at least one limitation is required",
        ));
    }
    for (index, limitation) in limitations.iter().enumerate() {
        require_str(limitation, &format!("envelope.limitations[{index}]"))?;
    }
    require_nonempty_string(root, "claim_boundary", "envelope.claim_boundary")?;
    Ok(())
}

/// The activating filetypes and root markers the envelope recorded.
///
/// These are read out of the envelope rather than restated here so this
/// validator never becomes a second root-marker or filetype authority.
struct RecordedConfig {
    filetypes: BTreeSet<String>,
    markers: BTreeSet<String>,
}

fn validate_config(root: &Map<String, Value>) -> Result<RecordedConfig, EnvelopeValidationError> {
    let config = require_object(root, "config", "envelope.config")?;
    reject_unknown_keys(
        config,
        &["filetypes", "path", "root_marker_groups", "sha256"],
        "envelope.config",
    )?;
    let path = require_nonempty_string(config, "path", "envelope.config.path")?;
    require_repo_relative(path, "envelope.config.path")?;
    require_sha256(config, "sha256", "envelope.config.sha256")?;

    let mut filetypes = BTreeSet::new();
    for (index, value) in
        require_array(config, "filetypes", "envelope.config.filetypes")?.iter().enumerate()
    {
        let path = format!("envelope.config.filetypes[{index}]");
        filetypes.insert(require_str(value, &path)?.to_owned());
    }
    if filetypes.is_empty() {
        return Err(EnvelopeValidationError::new(
            "envelope.config.filetypes: at least one activating filetype is required",
        ));
    }

    // Marker groups are either a bare marker or an equal-priority group of
    // markers. Both shapes are flattened: the requirement below is only that
    // every configured marker is exercised, not how it is grouped.
    let mut markers = BTreeSet::new();
    let groups = require_array(config, "root_marker_groups", "envelope.config.root_marker_groups")?;
    if groups.is_empty() {
        return Err(EnvelopeValidationError::new(
            "envelope.config.root_marker_groups: at least one root marker is required",
        ));
    }
    for (index, group) in groups.iter().enumerate() {
        let path = format!("envelope.config.root_marker_groups[{index}]");
        match group {
            Value::String(marker) => {
                markers.insert(marker.clone());
            }
            Value::Array(entries) => {
                for (inner, entry) in entries.iter().enumerate() {
                    markers.insert(require_str(entry, &format!("{path}[{inner}]"))?.to_owned());
                }
            }
            _ => {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: expected a marker string or an equal-priority group"
                )));
            }
        }
    }

    Ok(RecordedConfig { filetypes, markers })
}

fn validate_file_families(root: &Map<String, Value>, config: &RecordedConfig) -> Validated {
    let families = require_object(root, "file_families", "envelope.file_families")?;

    for (required, fixture) in REQUIRED_FILE_FAMILIES {
        let Some(row) = families.get(*required) else {
            return Err(EnvelopeValidationError::new(format!(
                "envelope.file_families: required family `{required}` is missing"
            )));
        };
        // The identifier names a specific fixture. Without this, one passing row
        // copied under every required key would satisfy the denominator.
        let recorded = row.get("fixture").and_then(Value::as_str).unwrap_or_default();
        if recorded != *fixture {
            return Err(EnvelopeValidationError::new(format!(
                "envelope.file_families.{required}.fixture: expected `{fixture}`, found \
                 `{recorded}`"
            )));
        }
    }

    for (id, value) in families {
        let path = format!("envelope.file_families.{id}");
        let family = as_object(value, &path)?;
        reject_unknown_keys(family, FILE_FAMILY_FIELDS, &path)?;

        require_nonempty_string(family, "fixture", &format!("{path}.fixture"))?;
        let native =
            require_str_field(family, "native_filetype", &format!("{path}.native_filetype"))?;
        let opened =
            require_str_field(family, "opened_filetype", &format!("{path}.opened_filetype"))?;
        let eligible = require_bool(family, "config_eligible", &format!("{path}.config_eligible"))?;
        let attached = require_bool(family, "attached", &format!("{path}.attached"))?;
        let override_applied =
            require_bool(family, "override_applied", &format!("{path}.override_applied"))?;
        require_bool(family, "content_dependent", &format!("{path}.content_dependent"))?;
        let language_id = require_str_field(family, "language_id", &format!("{path}.language_id"))?;
        let disposition =
            require_nonempty_string(family, "disposition", &format!("{path}.disposition"))?;

        if !ALL_DISPOSITIONS.contains(&disposition) {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.disposition: unsupported value `{disposition}`"
            )));
        }

        if attached && !eligible {
            return Err(EnvelopeValidationError::new(format!(
                "{path}: attached=true contradicts config_eligible=false"
            )));
        }
        if attached && language_id.is_empty() {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.language_id: an attached buffer must record the language id it sent"
            )));
        }
        if !attached && !language_id.is_empty() {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.language_id: a buffer that never attached cannot report a sent language id"
            )));
        }

        // Eligibility follows the filetype the buffer actually carried when it
        // was opened, because that is what the client matches against. Native
        // detection is a separate fact and is what the dispositions below speak
        // about.
        let opened_activates = config.filetypes.contains(opened);
        if eligible != opened_activates {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.config_eligible: `{eligible}` contradicts opened filetype `{opened}` \
                 against the recorded activating filetypes"
            )));
        }
        // Without a recorded override there is nothing that could have changed
        // the buffer's filetype after detection, so a divergence here would mean
        // an unreported override.
        if !override_applied && opened != native {
            return Err(EnvelopeValidationError::new(format!(
                "{path}: opened filetype `{opened}` differs from native `{native}` with no \
                 recorded override"
            )));
        }

        // Failure states are orthogonal to the native/attach policy branches
        // below: they say the run did not establish the row, not what the
        // project intends for that family.
        if FAILED_DISPOSITIONS.contains(&disposition) {
            if REQUIRED_FILE_FAMILIES.iter().any(|(name, _)| *name == id.as_str()) {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}.disposition: required family `{id}` is `{disposition}`; the run did \
                     not establish it"
                )));
            }
            require_nonempty_string(family, "reason", &format!("{path}.reason"))?;
        } else if ATTACHING_DISPOSITIONS.contains(&disposition) {
            // The disposition names Perl, so it must mean Perl. Membership of
            // the activating list is a separate fact and cannot stand in for
            // it: widening that list must never turn Mason or POD evidence into
            // native Perl support.
            if native != PERL_FILETYPE {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` requires native filetype \
                     `{PERL_FILETYPE}`, found `{native}`"
                )));
            }
            if !attached {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` requires attached=true"
                )));
            }
            if override_applied {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: an applied override cannot be recorded as native support"
                )));
            }
        } else if disposition == "native_perl_activation_only" {
            if native != PERL_FILETYPE {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` requires native filetype \
                     `{PERL_FILETYPE}`, found `{native}`"
                )));
            }
            if !attached {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` requires attached=true"
                )));
            }
        } else {
            if native == PERL_FILETYPE {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` contradicts native filetype `{native}`"
                )));
            }
            if attached {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` cannot attach through the canonical config"
                )));
            }
            // Only the dispositions that actually claim the canonical config
            // leaves the family inactive. `unsupported` is about what the host
            // can do, which is orthogonal: an opened filetype can match the
            // config on a host that still cannot establish the family.
            if eligible && NON_ACTIVATING_POLICY_DISPOSITIONS.contains(&disposition) {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: disposition `{disposition}` says the canonical config does not \
                     activate this family, but config_eligible=true"
                )));
            }
        }

        if REASONED_DISPOSITIONS.contains(&disposition) {
            require_nonempty_string(family, "reason", &format!("{path}.reason"))?;
        }
    }
    Ok(())
}

fn validate_roots(root: &Map<String, Value>, config: &RecordedConfig) -> Validated {
    let roots = require_object(root, "roots", "envelope.roots")?;

    for (required, marker, expected_role) in REQUIRED_ROOT_CELLS {
        let Some(row) = roots.get(*required) else {
            return Err(EnvelopeValidationError::new(format!(
                "envelope.roots: required root cell `{required}` is missing"
            )));
        };
        let recorded = row.get("marker").and_then(Value::as_str).unwrap_or_default();
        if recorded != *marker {
            return Err(EnvelopeValidationError::new(format!(
                "envelope.roots.{required}.marker: expected `{marker}`, found `{recorded}`"
            )));
        }
        let role = row.get("expected_role").and_then(Value::as_str).unwrap_or_default();
        if role != *expected_role {
            return Err(EnvelopeValidationError::new(format!(
                "envelope.roots.{required}.expected_role: expected `{expected_role}`, found \
                 `{role}`"
            )));
        }
    }

    let mut proven_markers = BTreeSet::new();

    for (id, value) in roots {
        let path = format!("envelope.roots.{id}");
        let cell = as_object(value, &path)?;
        reject_unknown_keys(cell, ROOT_FIELDS, &path)?;

        let marker = require_nonempty_string(cell, "marker", &format!("{path}.marker"))?;
        // Together with the "every configured marker wins a cell" rule below,
        // this pins the exercised marker set to the recorded configuration in
        // both directions, so the harness cannot quietly grow a marker
        // authority of its own.
        if marker != BOUNDARY_MARKER && !config.markers.contains(marker) {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.marker: `{marker}` is not one of the configured root markers"
            )));
        }
        let expected_role = require_role(cell, "expected_role", &format!("{path}.expected_role"))?;
        let actual_role = require_role(cell, "actual_role", &format!("{path}.actual_role"))?;
        let root_match = require_bool(cell, "root_match", &format!("{path}.root_match"))?;

        // The observation cell exists precisely to assert no root. Left to the
        // generic role-equality invariant, an envelope could set both roles to
        // the same value and turn it into the single-file root claim this row
        // is here to withhold.
        if id == OBSERVATION_ONLY_ROOT_CELL {
            if marker != BOUNDARY_MARKER {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}.marker: the observation cell must record `{BOUNDARY_MARKER}`"
                )));
            }
            if expected_role != OBSERVATION_ONLY_ROLE {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}.expected_role: the observation cell must record \
                     `{OBSERVATION_ONLY_ROLE}`"
                )));
            }
            if root_match {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}.root_match: the observation cell asserts no root and cannot claim a \
                     match"
                )));
            }
            if actual_role == OBSERVATION_ONLY_ROLE {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}.actual_role: `{OBSERVATION_ONLY_ROLE}` is the expectation placeholder, \
                     not an observed root"
                )));
            }
        } else if root_match != (expected_role == actual_role) {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.root_match: `{root_match}` contradicts expected `{expected_role}` \
                 against actual `{actual_role}`"
            )));
        }

        let semantic_path = format!("{path}.semantic");
        let semantic = require_object(cell, "semantic", &semantic_path)?;
        reject_unknown_keys(semantic, SEMANTIC_FIELDS, &semantic_path)?;

        require_nonempty_string(
            semantic,
            "definition_method",
            &format!("{semantic_path}.definition_method"),
        )?;
        require_nonempty_string(
            semantic,
            "content_method",
            &format!("{semantic_path}.content_method"),
        )?;
        let expected_target = require_role(
            semantic,
            "expected_target_role",
            &format!("{semantic_path}.expected_target_role"),
        )?;
        let observed_target = require_role(
            semantic,
            "observed_target_role",
            &format!("{semantic_path}.observed_target_role"),
        )?;
        let expected_marker = require_str_field(
            semantic,
            "expected_marker",
            &format!("{semantic_path}.expected_marker"),
        )?;
        let observed_marker = require_str_field(
            semantic,
            "observed_marker",
            &format!("{semantic_path}.observed_marker"),
        )?;
        let outcome =
            require_nonempty_string(semantic, "outcome", &format!("{semantic_path}.outcome"))?;
        if !SEMANTIC_OUTCOMES.contains(&outcome) {
            return Err(EnvelopeValidationError::new(format!(
                "{semantic_path}.outcome: unsupported value `{outcome}`"
            )));
        }

        let mut rejected = Vec::new();
        for (index, entry) in require_array(
            semantic,
            "rejected_symbols",
            &format!("{semantic_path}.rejected_symbols"),
        )?
        .iter()
        .enumerate()
        {
            let entry_path = format!("{semantic_path}.rejected_symbols[{index}]");
            let symbol = require_str(entry, &entry_path)?;
            if symbol.trim().is_empty() {
                return Err(EnvelopeValidationError::new(format!(
                    "{entry_path}: must not be empty"
                )));
            }
            rejected.push(symbol.to_owned());
        }

        if outcome == "proven" {
            if !root_match {
                return Err(EnvelopeValidationError::new(format!(
                    "{path}: outcome=proven requires root_match=true"
                )));
            }
            if expected_marker.is_empty() {
                return Err(EnvelopeValidationError::new(format!(
                    "{semantic_path}: outcome=proven requires a root-specific expected_marker; \
                     `root_dir` equality is not a semantic result"
                )));
            }
            if observed_marker != expected_marker {
                return Err(EnvelopeValidationError::new(format!(
                    "{semantic_path}: observed marker `{observed_marker}` does not confirm \
                     expected `{expected_marker}`"
                )));
            }
            if observed_target != expected_target {
                return Err(EnvelopeValidationError::new(format!(
                    "{semantic_path}: resolved `{observed_target}` but expected `{expected_target}`"
                )));
            }
            if !observed_target.starts_with(&format!("{actual_role}/")) {
                return Err(EnvelopeValidationError::new(format!(
                    "{semantic_path}: resolved target `{observed_target}` lies outside the \
                     selected root `{actual_role}`"
                )));
            }
            if ISOLATION_ROOT_CELLS.contains(&id.as_str()) && rejected.is_empty() {
                return Err(EnvelopeValidationError::new(format!(
                    "{semantic_path}.rejected_symbols: `{id}` claims root isolation and must name \
                     the competing facts it rejected"
                )));
            }
            proven_markers.insert(marker.to_owned());
        } else {
            require_nonempty_string(semantic, "reason", &format!("{semantic_path}.reason"))?;
            if id == OBSERVATION_ONLY_ROOT_CELL {
                // The observation cell is the sole exemption from `proven`, but
                // only for the deliberate observation it exists to record. A
                // run that failed to make that observation has not established
                // the required cell either.
                if outcome != OBSERVATION_ONLY_OUTCOME {
                    return Err(EnvelopeValidationError::new(format!(
                        "{semantic_path}.outcome: the observation cell must record \
                         `{OBSERVATION_ONLY_OUTCOME}`, found `{outcome}`"
                    )));
                }
            } else if REQUIRED_ROOT_CELLS.iter().any(|(name, _, _)| *name == id.as_str()) {
                // Marker coverage alone cannot carry this: several cells share
                // one marker, so a degraded conflict or isolation cell would
                // otherwise hide behind a sibling cell naming the same marker.
                return Err(EnvelopeValidationError::new(format!(
                    "{semantic_path}.outcome: required root cell `{id}` is `{outcome}`; it must \
                     reach `proven` on its own evidence"
                )));
            }
        }

        let observed_symbol = format!("probe_{observed_marker}");
        if !observed_marker.is_empty() && rejected.contains(&observed_symbol) {
            return Err(EnvelopeValidationError::new(format!(
                "{semantic_path}: `{observed_symbol}` cannot be both the observed result and a \
                 rejected wrong-root fact"
            )));
        }
    }

    for marker in &config.markers {
        if !proven_markers.contains(marker) {
            return Err(EnvelopeValidationError::new(format!(
                "envelope.roots: configured root marker `{marker}` never won a proven cell"
            )));
        }
    }
    Ok(())
}

fn validate_identity(root: &Map<String, Value>, key: &str, fields: &[&str]) -> Validated {
    let path = format!("envelope.{key}");
    let object = require_object(root, key, &path)?;
    reject_unknown_keys(object, fields, &path)?;
    for field in fields {
        require_nonempty_string(object, field, &format!("{path}.{field}"))?;
    }
    if key == "host" {
        require_exact_string(object, "family", HOST_FAMILY, &format!("{path}.family"))?;
    }
    if key == "server" {
        require_sha256(object, "sha256", &format!("{path}.sha256"))?;
        let role = require_nonempty_string(object, "role", &format!("{path}.role"))?;
        if !SERVER_ROLES.contains(&role) {
            return Err(EnvelopeValidationError::new(format!(
                "{path}.role: unsupported value `{role}`"
            )));
        }
    }
    Ok(())
}

/// Absolute in the POSIX or Windows sense, or a URI.
///
/// A durable envelope carries normalized identities, so the producing machine's
/// own paths must be rejected wherever they could appear. Both call sites share
/// this predicate so the two surfaces cannot drift apart.
/// A role must be a normalized identity. Dot segments or empty segments would
/// let a target that escapes the selected root still satisfy the prefix
/// containment check below.
fn has_non_normalized_segment(value: &str) -> bool {
    value.split('/').skip(1).any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

fn is_absolute_or_uri(value: &str) -> bool {
    if value.starts_with('/') || value.starts_with('\\') || value.contains("://") {
        return true;
    }
    let bytes = value.as_bytes();
    bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// Roles are normalized fixture-relative identities. A durable envelope must
/// not carry the private absolute paths of the machine that produced it.
fn require_role<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, EnvelopeValidationError> {
    let value = require_nonempty_string(parent, key, path)?;
    if is_absolute_or_uri(value) {
        return Err(EnvelopeValidationError::new(format!(
            "{path}: `{value}` is an absolute path; roles must be normalized identities"
        )));
    }
    if has_non_normalized_segment(value) {
        return Err(EnvelopeValidationError::new(format!(
            "{path}: `{value}` is not normalized; a dot or empty segment can escape the root it \
             appears to be inside"
        )));
    }
    Ok(value)
}

fn require_repo_relative(value: &str, path: &str) -> Validated {
    if is_absolute_or_uri(value) {
        return Err(EnvelopeValidationError::new(format!(
            "{path}: `{value}` must be repository-relative"
        )));
    }
    Ok(())
}

fn require_sha256(parent: &Map<String, Value>, key: &str, path: &str) -> Validated {
    let value = require_nonempty_string(parent, key, path)?;
    if value.len() != 64 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(EnvelopeValidationError::new(format!(
            "{path}: expected a sha256 digest, found `{value}`"
        )));
    }
    Ok(())
}

fn reject_unknown_keys(object: &Map<String, Value>, allowed: &[&str], path: &str) -> Validated {
    for key in object.keys() {
        if !allowed.iter().any(|allowed_key| allowed_key == key) {
            return Err(EnvelopeValidationError::new(format!("{path}: unknown field `{key}`")));
        }
    }
    Ok(())
}

fn as_object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a Map<String, Value>, EnvelopeValidationError> {
    value
        .as_object()
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: expected object")))
}

fn require_object<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Map<String, Value>, EnvelopeValidationError> {
    let value = parent
        .get(key)
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: missing required field")))?;
    as_object(value, path)
}

fn require_array<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Vec<Value>, EnvelopeValidationError> {
    parent
        .get(key)
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: missing required field")))?
        .as_array()
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: expected array")))
}

fn require_str<'a>(value: &'a Value, path: &str) -> Result<&'a str, EnvelopeValidationError> {
    value.as_str().ok_or_else(|| EnvelopeValidationError::new(format!("{path}: expected string")))
}

/// A string field that is allowed to be empty, such as a filetype Neovim did
/// not classify.
fn require_str_field<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, EnvelopeValidationError> {
    let value = parent
        .get(key)
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: missing required field")))?;
    require_str(value, path)
}

fn require_nonempty_string<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, EnvelopeValidationError> {
    let value = require_str_field(parent, key, path)?;
    if value.trim().is_empty() {
        return Err(EnvelopeValidationError::new(format!("{path}: must not be empty")));
    }
    Ok(value)
}

fn require_exact_string(
    parent: &Map<String, Value>,
    key: &str,
    expected: &str,
    path: &str,
) -> Validated {
    let actual = require_nonempty_string(parent, key, path)?;
    if actual != expected {
        return Err(EnvelopeValidationError::new(format!(
            "{path}: expected `{expected}`, found `{actual}`"
        )));
    }
    Ok(())
}

fn require_bool(
    parent: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<bool, EnvelopeValidationError> {
    parent
        .get(key)
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: missing required field")))?
        .as_bool()
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: expected boolean")))
}

fn require_u64(
    parent: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<u64, EnvelopeValidationError> {
    parent
        .get(key)
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: missing required field")))?
        .as_u64()
        .ok_or_else(|| EnvelopeValidationError::new(format!("{path}: expected unsigned integer")))
}

/// Coherence of the tables above, proven where they are declared.
///
/// The falsifiers for the validator's *behaviour* live in
/// `xtask/tests/neovim_activation_root_envelope.rs` and stay there. What this
/// module adds is the property those falsifiers cannot see: the constants are
/// hand-maintained parallel tables, and a plausible edit — dropping a row,
/// duplicating an identifier, reusing a scenario role, naming a disposition
/// that is not in the census — leaves every behavioural test passing while the
/// denominator quietly stops meaning what it says.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn is_sorted_unique(values: impl Iterator<Item = &'static str>) -> bool {
        let collected: Vec<&str> = values.collect();
        let mut sorted = collected.clone();
        sorted.sort_unstable();
        sorted.dedup();
        collected == sorted
    }

    #[test]
    fn the_schema_version_is_the_one_the_producer_emits() {
        // The producer writes this literal into every envelope
        // (scripts/ux/neovim/neovim_activation_root_smoke.lua). Changing one
        // side alone silently rejects every captured run.
        assert_eq!(SCHEMA_VERSION, "neovim_activation_root_envelope.v1");
    }

    #[test]
    fn the_required_file_family_census_is_sorted_and_unambiguous() {
        assert!(!REQUIRED_FILE_FAMILIES.is_empty());
        assert!(is_sorted_unique(REQUIRED_FILE_FAMILIES.iter().map(|(id, _)| *id)));
        // A fixture may back at most one family: two families sharing a file
        // would make one of them a relabelled copy of the other, which is the
        // fabrication the fixture binding exists to stop.
        let mut fixtures: Vec<&str> = REQUIRED_FILE_FAMILIES.iter().map(|(_, f)| *f).collect();
        fixtures.sort_unstable();
        let before = fixtures.len();
        fixtures.dedup();
        assert_eq!(fixtures.len(), before, "two required families cite the same fixture");
    }

    #[test]
    fn every_required_root_cell_names_its_own_scenario() {
        assert!(!REQUIRED_ROOT_CELLS.is_empty());
        assert!(is_sorted_unique(REQUIRED_ROOT_CELLS.iter().map(|(id, _, _)| *id)));
        // The load-bearing property: markers are deliberately shared (three
        // cells use `cpanfile`, two use `.git`), so the role is the only column
        // that keeps each required identifier bound to a distinct scenario.
        let mut roles: Vec<&str> = REQUIRED_ROOT_CELLS.iter().map(|(_, _, r)| *r).collect();
        roles.sort_unstable();
        let before = roles.len();
        roles.dedup();
        assert_eq!(roles.len(), before, "two required root cells share an expected role");
        assert!(
            REQUIRED_ROOT_CELLS.iter().filter(|(_, m, _)| *m == "cpanfile").count() > 1,
            "the shared-marker case this binding exists for has disappeared"
        );
    }

    #[test]
    fn the_observation_cell_is_the_single_exemption_and_is_declared_as_one() {
        let boundary = REQUIRED_ROOT_CELLS
            .iter()
            .find(|(id, _, _)| *id == OBSERVATION_ONLY_ROOT_CELL)
            .copied();
        let Some((_, marker, role)) = boundary else {
            panic!("the observation cell is not in the required census");
        };
        assert_eq!(marker, BOUNDARY_MARKER);
        assert_eq!(role, OBSERVATION_ONLY_ROLE);
        assert!(SEMANTIC_OUTCOMES.contains(&OBSERVATION_ONLY_OUTCOME));
        // Every other required cell asserts a real root, so it must carry a
        // fixture role rather than the observation placeholder.
        for (id, _, role) in REQUIRED_ROOT_CELLS {
            if *id == OBSERVATION_ONLY_ROOT_CELL {
                continue;
            }
            assert!(role.starts_with("fixture:"), "{id} does not name a fixture root");
        }
    }

    #[test]
    fn isolation_cells_are_required_cells_that_assert_a_root() {
        assert!(!ISOLATION_ROOT_CELLS.is_empty());
        assert!(is_sorted_unique(ISOLATION_ROOT_CELLS.iter().copied()));
        for id in ISOLATION_ROOT_CELLS {
            assert!(
                REQUIRED_ROOT_CELLS.iter().any(|(required, _, _)| required == id),
                "{id} claims isolation but is not a required root cell"
            );
            assert_ne!(
                *id, OBSERVATION_ONLY_ROOT_CELL,
                "the observation cell asserts no root and cannot claim isolation"
            );
        }
    }

    #[test]
    fn every_accepted_key_list_is_sorted_and_unambiguous() {
        // `reject_unknown_keys` compares against these, so a duplicate is dead
        // weight and an unsorted list hides the next missing field.
        assert!(is_sorted_unique(FILE_FAMILY_FIELDS.iter().copied()));
        assert!(is_sorted_unique(ROOT_FIELDS.iter().copied()));
        assert!(is_sorted_unique(SEMANTIC_FIELDS.iter().copied()));
        assert!(is_sorted_unique(SEMANTIC_OUTCOMES.iter().copied()));
        assert!(is_sorted_unique(SERVER_ROLES.iter().copied()));
        assert!(!SERVER_ROLES.is_empty());
    }

    #[test]
    fn every_disposition_subset_is_drawn_from_the_census() {
        assert!(is_sorted_unique(ALL_DISPOSITIONS.iter().copied()));
        for subset in [
            ATTACHING_DISPOSITIONS,
            NON_ACTIVATING_POLICY_DISPOSITIONS,
            REASONED_DISPOSITIONS,
            FAILED_DISPOSITIONS,
        ] {
            assert!(is_sorted_unique(subset.iter().copied()));
            for disposition in subset {
                assert!(
                    ALL_DISPOSITIONS.contains(disposition),
                    "{disposition} is not in the disposition census"
                );
            }
        }
        // These two partition the ways a row can decline to activate, so an
        // overlap would make one row satisfy contradictory rules.
        for disposition in NON_ACTIVATING_POLICY_DISPOSITIONS {
            assert!(!ATTACHING_DISPOSITIONS.contains(disposition));
            assert!(!FAILED_DISPOSITIONS.contains(disposition));
        }
    }

    #[test]
    fn the_pinned_subject_identities_are_the_ones_this_schema_describes() {
        assert_eq!(HOST_FAMILY, "neovim");
        assert_eq!(PERL_FILETYPE, "perl");
        assert_eq!(BOUNDARY_MARKER, "none");
        assert_eq!(OBSERVATION_ONLY_ROLE, "observation_only");
        assert_eq!(OBSERVATION_ONLY_OUTCOME, "not_applicable");
    }

    #[test]
    fn a_rejection_carries_the_path_it_was_raised_against() {
        let failure: Validated = Err(EnvelopeValidationError::new("envelope.config.path: bad"));
        let Err(error) = failure else {
            panic!("the constructed rejection did not survive the Result");
        };
        assert_eq!(error.to_string(), "envelope.config.path: bad");
        assert_eq!(error, EnvelopeValidationError::new("envelope.config.path: bad"));
    }

    #[test]
    fn the_recorded_config_carries_both_flattened_marker_shapes() {
        let envelope = json!({
            "path": "scripts/ux/neovim/perllsp.lua",
            "sha256": "2ba6d3dc4faedfbce75a397b70b4d4bf88237950dacd86fcd6060c3c2c83e018",
            "filetypes": ["perl"],
            "root_marker_groups": [["cpanfile", "dist.ini"], ".git"],
        });
        let root = json!({ "config": envelope });
        let Some(object) = root.as_object() else {
            panic!("the constructed envelope is not an object");
        };
        let Ok(recorded) = validate_config(object) else {
            panic!("a well-formed config did not validate");
        };
        // A bare marker and an equal-priority group must reach the same set:
        // the nesting is Neovim's priority mechanism, not a marker identity.
        assert!(recorded.markers.contains("cpanfile"));
        assert!(recorded.markers.contains("dist.ini"));
        assert!(recorded.markers.contains(".git"));
        assert!(recorded.filetypes.contains(PERL_FILETYPE));
    }
}
