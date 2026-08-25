//! The #11388 expanded-activation family cell catalog.
//!
//! Every cell is keyed by the finite #7762 activation-root denominator: the 18
//! filetype rows of `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json`,
//! mirrored in [`ACTIVATION_DENOMINATOR`] and checked against that artifact by
//! tests so the mirror cannot drift from the landed authority. Each row
//! pre-registers the five independently visible activation propositions:
//!
//! | Aspect | Classifying action | Proposition |
//! | --- | --- | --- |
//! | `<row>_native_filetype` | `observe_native_filetype` | the native Vim filetype result for the exact row (a pre-forced filetype is never native) |
//! | `<row>_override` | `declared_override_row` | the bounded override disposition — never native, never blanket first-class |
//! | `<row>_attachment` | `observe_service_attachment` | vim-lsp attachment/languageId/root identity — activation only, never semantic support |
//! | `<row>_semantic_result` | `root_semantic_discriminator` | semantic eligibility/result, claimed only where the #7762 row claims it |
//! | `<row>_ambiguity_preserved` | `root_semantic_discriminator` | ambiguity/adjacent-language preservation stays independently visible |
//!
//! The landed cell-ID grammar admits exactly `vim.vim_lsp.<family>.<name>`
//! (two stable reason-token segments), so the spec's illustrated
//! `vim.vim_lsp.activation.<row>.<aspect>` registers in its
//! convention-equivalent form `vim.vim_lsp.activation.<row_slug>_<aspect>`:
//! the row and the aspect stay visible in the ID and the denominator laws
//! below bind them. The only row whose artifact case id (`PL`) is not a
//! lowercase reason token carries the documented slug `pl_uppercase`.
//!
//! Ownership split — consumed, never duplicated:
//!
//! - [`super`] owns the registration model and cross-catalog laws; this module
//!   owns this family's ledger, denominator mirror, fixture substrate,
//!   vocabulary, cells, and the family laws [`validate_activation_catalog`]
//!   adds on top.
//! - `crate::vim_lsp_specialized_driver` (#11380) owns the action vocabulary;
//!   the scenario ledger is *derived* from the landed activation actions, so
//!   the binding cannot drift from the vocabulary.
//! - `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json` (#7762 /
//!   #7766) owns the filetype/root denominator bytes: row cases, paths,
//!   expectations, detection sources, negative controls, override
//!   authorization boundaries, and independent semantic support. The mirror
//!   here is checked against that file by tests; a denominator change is a
//!   reviewed edit that changes every affected digest visibly.
//! - #11376 owns the activation BDD scenarios and #11378 the activation
//!   fixtures; both remain pending. Until they land, cells bind the landed
//!   #11380 action vocabulary as scenario owners and the landed #11369/#7762
//!   fixture authorities as fixture owners; re-binding is a reviewed edit.
//!
//! Family laws beyond the shared model (all fail-closed):
//!
//! - the ledger mirrors exactly the landed #11380 activation actions;
//! - the registered cells are exactly denominator rows x aspects: a missing
//!   row-aspect cell, a duplicate row-aspect registration, or a cell outside
//!   the finite #7762 denominator is rejected;
//! - every cell binds exactly one `activation.row.*` dimension that matches
//!   its own cell-ID slug, its row's `activation.expect.*` expectation, and
//!   its row's `activation.row_binding.*` authority identity (a sha256 over
//!   every #7762 row field), so one row's observation cannot inherit another
//!   row's identity and a denominator edit of any authority field — fixture
//!   path, controls, boundaries included — is digest-visible;
//! - each aspect is classified by its one pinned #11380 action, so an
//!   attachment observation can never classify a semantic cell;
//! - a `semantic_result` cell of a row whose #7762 expectation is not `perl`
//!   never admits a semantic-support-affirming result, so a successfully
//!   attached adjacent-language false subject still fails the semantic
//!   proposition (the ambiguity cell keeps its own preservation disposition);
//! - each aspect's allowed result set is pinned (filetype, override, and
//!   attachment can never admit each other's or the semantic cell's
//!   dispositions), every cell admits `fail` and `not_proven`, and an
//!   override row carrying a `manual_override` boundary keeps its
//!   `not_authorized_by_extension_alone` limitation;
//! - cells citing the between-rows reset action require cleanup evidence, so
//!   rows cannot contaminate each other silently;
//! - the stage bound is `exact_source_local` only and every cell feeds only
//!   `vim_first_class_exact_source`.

use anyhow::{Context as _, Result, ensure};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

use super::{
    CellCatalog, CellRegistration, CellSubject, CoverageRule, InstrumentEvidence, Scenario,
    ScenarioClass, ScenarioLedger,
};
use crate::editor_client_compat::EvidenceStage;
use crate::vim_lsp_specialized_driver::{ACTIONS, ActionFamily};

pub const ACTIVATION_CATALOG_ID: &str = "vim_lsp_activation";
pub const ACTIVATION_LEDGER_ID: &str = "vim.vim_lsp.specialized.activation.v1";

/// Fixture substrate: the landed #11369/#7762 authorities this family binds
/// until #11376/#11378 land their owning surfaces. Tests verify each ID
/// resolves to `.ci/editor-clients/<id>.json`, so an absent authority fails
/// closed.
pub const ACTIVATION_FIXTURE_SUBSTRATE: &[&str] = &[
    "vim-vim-lsp-activation-root.v1",
    "vim-vim-lsp-configuration.v1",
    "vim-vim-lsp-public-surface.v1",
    "vim-vim-lsp-subject.v1",
];

/// Activation dispositions (#11388). Beyond the receipt-serializable generic
/// set, `native_supported`/`bounded_override_supported`/`activation_only` are
/// family-level result tokens naming detection/override/attachment outcomes —
/// never semantic support, which only `semantic_result` cells may carry and
/// only where the #7762 row claims it.
pub const ACTIVATION_RESULT_VOCABULARY: &[&str] = &[
    "native_supported",
    "bounded_override_supported",
    "activation_only",
    "fail",
    "client_not_exposed",
    "unsupported",
    "not_proven",
];

pub const ACTIVATION_LIMITATION_VOCABULARY: &[&str] = &[
    "activation_is_not_semantic_support",
    "client_not_exposed",
    "not_authorized_by_extension_alone",
    "not_proven",
    "observation_incomplete",
    "instrument_incomplete",
];

/// The one #7762 expectation that claims Perl semantic support; every other
/// expectation (`xpm`, `tads`, `observe`) keeps its semantic cell
/// non-affirming.
const PERL_EXPECTATION: &str = "perl";

const ACTIVATION_PROFILE: &str = "vim_first_class_exact_source";
const CELL_PREFIX: &str = "vim.vim_lsp.activation.";
const ROW_DIMENSION_PREFIX: &str = "activation.row.";
const EXPECT_DIMENSION_PREFIX: &str = "activation.expect.";
const ROW_BINDING_PREFIX: &str = "activation.row_binding.";

/// Dimensions every activation cell must bind: the pinned client/server/stage
/// identity plus exactly one denominator row dimension.
const REQUIRED_DIMENSIONS: &[&str] =
    &["client.pinned_commit", "server.executable_identity", "stage.exact_source_local"];

/// The five aspects every denominator row registers (#11388), each pinned to
/// its one classifying #11380 action: the validator enforces the mapping, not
/// only the factory, so an attachment observation can never classify a
/// semantic cell even through a reviewed row edit.
pub const ACTIVATION_ASPECTS: &[&str] =
    &["native_filetype", "override", "attachment", "semantic_result", "ambiguity_preserved"];

/// The classifying action of one aspect (`ACTIVATION_ASPECTS` order).
const ASPECT_OBSERVATION_CLASSES: &[&str] = &[
    OBSERVE_NATIVE_FILETYPE,
    DECLARED_OVERRIDE_ROW,
    OBSERVE_SERVICE_ATTACHMENT,
    ROOT_SEMANTIC_DISCRIMINATOR,
    ROOT_SEMANTIC_DISCRIMINATOR,
];

const HEX: &[u8; 16] = b"0123456789abcdef";

/// The stable authority identity of one denominator row: a sha256 over every
/// authority field the #7762 artifact carries for the row (case, path,
/// expectation, detection source, negative control, override boundary,
/// independent semantic support). Every cell of the row binds it as a
/// `activation.row_binding.sha256-<hex>` dimension, so an artifact edit of
/// *any* row field — including the fixture path or a control flag, which no
/// other binding names — changes every cell digest of that row and the
/// catalog digest: denominator edits stay digest-visible, never silent.
pub fn row_binding_identity(row: &ActivationDenominatorRow) -> String {
    let canonical = format!(
        "case={}|path={}|expect={}|source={}|negative_control={}|manual_override={}|semantic_support={}",
        row.case_id,
        row.path,
        row.expect,
        row.source.unwrap_or("none"),
        row.negative_control,
        row.manual_override.unwrap_or("none"),
        row.semantic_support.unwrap_or("none"),
    );
    let digest = Sha256::digest(canonical.as_bytes());
    let mut identity = String::with_capacity(ROW_BINDING_PREFIX.len() + "sha256-".len() + 64);
    identity.push_str(ROW_BINDING_PREFIX);
    identity.push_str("sha256-");
    for byte in digest {
        identity.push(HEX[(byte >> 4) as usize] as char);
        identity.push(HEX[(byte & 0x0f) as usize] as char);
    }
    identity
}

/// One row of the finite #7762 activation-root denominator, mirrored from
/// `.ci/editor-clients/vim-vim-lsp-activation-root.v1.json` in artifact order.
/// Tests check every field against the artifact, so the mirror cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationDenominatorRow {
    /// Verbatim artifact `case` identity.
    pub case_id: &'static str,
    /// Cell-ID row slug: a stable reason token; equal to `case_id` whenever
    /// the case id is itself a lowercase reason token (the one sanctioned
    /// deviation is `PL` -> `pl_uppercase`, because cell-ID segments must be
    /// lowercase reason tokens and `pl` is already taken by the `.pl` row).
    pub slug: &'static str,
    /// Verbatim artifact fixture path for the row.
    pub path: &'static str,
    /// Verbatim artifact expectation (`perl`, `xpm`, `tads`, `observe`).
    pub expect: &'static str,
    /// Verbatim artifact native detection source, when the row declares one
    /// (the independent-semantic-support rows declare none).
    pub source: Option<&'static str>,
    /// Verbatim artifact negative-control flag (xpm/tads discriminators).
    pub negative_control: bool,
    /// Verbatim artifact manual-override boundary, when the row carries one.
    pub manual_override: Option<&'static str>,
    /// Verbatim artifact independent-semantic-support marker, when present.
    pub semantic_support: Option<&'static str>,
}

/// The finite #7762-backed denominator: 18 filetype rows in artifact order.
/// A cell outside this set cannot register (family law), and each row
/// registers all five aspects, so the catalog carries 90 cells.
pub const ACTIVATION_DENOMINATOR: &[ActivationDenominatorRow] = &[
    ActivationDenominatorRow {
        case_id: "pl",
        slug: "pl",
        path: "sample.pl",
        expect: "perl",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "PL",
        slug: "pl_uppercase",
        path: "legacy.PL",
        expect: "perl",
        source: Some("native_vim_uppercase_extension"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "pm_perl",
        slug: "pm_perl",
        path: "Sample.pm",
        expect: "perl",
        source: Some("native_vim_discriminator"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "pm_xpm",
        slug: "pm_xpm",
        path: "Image.pm",
        expect: "xpm",
        source: Some("native_vim_discriminator"),
        negative_control: true,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "t_perl",
        slug: "t_perl",
        path: "sample.t",
        expect: "perl",
        source: Some("native_vim_discriminator"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "t_tads",
        slug: "t_tads",
        path: "game.t",
        expect: "tads",
        source: Some("native_vim_discriminator"),
        negative_control: true,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "psgi",
        slug: "psgi",
        path: "app.psgi",
        expect: "perl",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "cgi",
        slug: "cgi",
        path: "app.cgi",
        expect: "observe",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: Some("not_authorized_by_extension_alone"),
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "fcgi",
        slug: "fcgi",
        path: "app.fcgi",
        expect: "observe",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: Some("not_authorized_by_extension_alone"),
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "cpanfile",
        slug: "cpanfile",
        path: "cpanfile",
        expect: "observe",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "bin_shebang",
        slug: "bin_shebang",
        path: "bin/tool",
        expect: "observe",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "script_shebang",
        slug: "script_shebang",
        path: "script/tool",
        expect: "observe",
        source: Some("native_vim"),
        negative_control: false,
        manual_override: None,
        semantic_support: None,
    },
    ActivationDenominatorRow {
        case_id: "pod",
        slug: "pod",
        path: "notes.pod",
        expect: "observe",
        source: None,
        negative_control: false,
        manual_override: None,
        semantic_support: Some("independent"),
    },
    ActivationDenominatorRow {
        case_id: "xs",
        slug: "xs",
        path: "Native.xs",
        expect: "observe",
        source: None,
        negative_control: false,
        manual_override: None,
        semantic_support: Some("independent"),
    },
    ActivationDenominatorRow {
        case_id: "ep",
        slug: "ep",
        path: "view.ep",
        expect: "observe",
        source: None,
        negative_control: false,
        manual_override: None,
        semantic_support: Some("independent"),
    },
    ActivationDenominatorRow {
        case_id: "tt",
        slug: "tt",
        path: "view.tt",
        expect: "observe",
        source: None,
        negative_control: false,
        manual_override: None,
        semantic_support: Some("independent"),
    },
    ActivationDenominatorRow {
        case_id: "tt2",
        slug: "tt2",
        path: "view.tt2",
        expect: "observe",
        source: None,
        negative_control: false,
        manual_override: None,
        semantic_support: Some("independent"),
    },
    ActivationDenominatorRow {
        case_id: "mason",
        slug: "mason",
        path: "view.mason",
        expect: "observe",
        source: None,
        negative_control: false,
        manual_override: None,
        semantic_support: Some("independent"),
    },
];

const OPEN_WITHOUT_PRESET: &str = "vim.vim_lsp.specialized.activation.open_without_preset_filetype";
const OBSERVE_NATIVE_FILETYPE: &str = "vim.vim_lsp.specialized.activation.observe_native_filetype";
const DECLARED_OVERRIDE_ROW: &str = "vim.vim_lsp.specialized.activation.declared_override_row";
const OBSERVE_SERVICE_ATTACHMENT: &str =
    "vim.vim_lsp.specialized.activation.observe_service_attachment";
const ROOT_SEMANTIC_DISCRIMINATOR: &str =
    "vim.vim_lsp.specialized.activation.root_semantic_discriminator";
const CLOSE_RESET_BETWEEN_ROWS: &str =
    "vim.vim_lsp.specialized.activation.close_reset_between_rows";

/// The between-rows reset action: citing it makes cleanup evidence
/// independently load-bearing, so activation rows cannot contaminate each
/// other silently.
const CLEANUP_REQUIRING_OWNERS: &[&str] = &[CLOSE_RESET_BETWEEN_ROWS];

/// Result tokens that affirm semantic support arrived. Only `semantic_result`
/// cells of rows whose #7762 expectation is `perl` may ever admit them.
const SEMANTIC_AFFIRMING_RESULTS: &[&str] = &["native_supported", "bounded_override_supported"];

const NATIVE_FILETYPE_RESULTS: &[&str] =
    &["native_supported", "fail", "client_not_exposed", "unsupported", "not_proven"];
const OVERRIDE_RESULTS: &[&str] =
    &["bounded_override_supported", "fail", "client_not_exposed", "unsupported", "not_proven"];
const ATTACHMENT_RESULTS: &[&str] =
    &["activation_only", "fail", "client_not_exposed", "unsupported", "not_proven"];
const SEMANTIC_CLAIMED_RESULTS: &[&str] = &[
    "native_supported",
    "bounded_override_supported",
    "fail",
    "client_not_exposed",
    "unsupported",
    "not_proven",
];
const SEMANTIC_UNCLAIMED_RESULTS: &[&str] =
    &["activation_only", "fail", "client_not_exposed", "unsupported", "not_proven"];
const AMBIGUITY_RESULTS: &[&str] =
    &["native_supported", "fail", "client_not_exposed", "unsupported", "not_proven"];

const FILETYPE_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
];
const OVERRIDE_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CleanupObservation,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
];
const ATTACHMENT_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CapabilitySnapshot,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
    InstrumentEvidence::ServerStderr,
];
const SEMANTIC_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CapabilitySnapshot,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
];
const AMBIGUITY_INSTRUMENT: &[InstrumentEvidence] = &[
    InstrumentEvidence::CleanupObservation,
    InstrumentEvidence::ClientLog,
    InstrumentEvidence::DriverOutput,
    InstrumentEvidence::ProcessLedger,
];

const BASE_CLAIM_CEILING: &str = "registration only: pre-registers one exact-subject Vim/vim-lsp expanded-activation cell for the generic editor_client_compat.v1 receipt, keyed by the finite #7762 activation-root denominator; binds landed #11380 action owners and #11369/#7762 fixtures until #11376/#11378 land their owning surfaces; proves no host activation behavior and awards no support profile";
const NATIVE_CLAIM_CEILING: &str = "registration only: the native filetype result is native detection without a pre-set filetype, never a semantic-support claim, and a pre-forced filetype can never be relabeled native";
const OVERRIDE_CLAIM_CEILING: &str = "registration only: the override disposition is bounded — never a native result, never blanket .t/.pm/.cgi/.fcgi first-class support, and an extension-only override without its receipt keeps its not-authorized limitation";
const ATTACHMENT_CLAIM_CEILING: &str = "registration only: service attachment/languageId/root identity is activation only and never semantic support; the wrong client, root, provider, or process cannot supply it";
const SEMANTIC_CLAIM_CEILING: &str = "registration only: semantic eligibility/result is claimed only where the #7762 row claims it; filetype detection, override, and attachment can never be relabeled into it";
const AMBIGUITY_CLAIM_CEILING: &str = "registration only: ambiguity/adjacent-language preservation stays independently visible even when vim-lsp attaches to the false subject; the semantic and ambiguity cells fail closed there";

/// Look up one denominator row by its cell-ID slug.
fn row_by_slug(slug: &str) -> Option<&'static ActivationDenominatorRow> {
    ACTIVATION_DENOMINATOR.iter().find(|row| row.slug == slug)
}

/// The row-scoped dimensions every activation cell binds: its denominator row
/// identity, the row's artifact expectation, and the row's full authority
/// identity (see [`row_binding_identity`]), so a receipt must name the exact
/// row, cannot inherit another row's expectation, and a denominator edit of
/// any authority field is digest-visible.
fn row_dimensions(row: &ActivationDenominatorRow) -> Vec<String> {
    vec![
        format!("{}{}", ROW_DIMENSION_PREFIX, row.slug),
        format!("{}{}", EXPECT_DIMENSION_PREFIX, row.expect),
        row_binding_identity(row),
    ]
}

/// Build one activation cell for a denominator row and aspect.
fn build_cell(
    row: &ActivationDenominatorRow,
    aspect: &str,
    subject: CellSubject,
) -> CellRegistration {
    let mut dimensions =
        REQUIRED_DIMENSIONS.iter().map(|value| value.to_string()).collect::<Vec<_>>();
    dimensions.extend(row_dimensions(row));
    let (owners, observation_class, instrument, results, ceiling): (
        Vec<&str>,
        &str,
        &[InstrumentEvidence],
        &[&str],
        String,
    ) = match aspect {
        "native_filetype" => {
            if let Some(source) = row.source {
                dimensions.push(format!("activation.detection.{source}"));
            }
            dimensions.push("activation.filetype.native".to_string());
            (
                vec![OPEN_WITHOUT_PRESET, OBSERVE_NATIVE_FILETYPE],
                OBSERVE_NATIVE_FILETYPE,
                FILETYPE_INSTRUMENT,
                NATIVE_FILETYPE_RESULTS,
                format!("{BASE_CLAIM_CEILING}; {NATIVE_CLAIM_CEILING}"),
            )
        }
        "override" => {
            dimensions.push("activation.override.bounded".to_string());
            (
                vec![DECLARED_OVERRIDE_ROW, CLOSE_RESET_BETWEEN_ROWS],
                DECLARED_OVERRIDE_ROW,
                OVERRIDE_INSTRUMENT,
                OVERRIDE_RESULTS,
                format!("{BASE_CLAIM_CEILING}; {OVERRIDE_CLAIM_CEILING}"),
            )
        }
        "attachment" => {
            dimensions.push("activation.language_id".to_string());
            dimensions.push("root.selection".to_string());
            dimensions.push("service.provider_identity".to_string());
            (
                vec![OPEN_WITHOUT_PRESET, OBSERVE_SERVICE_ATTACHMENT],
                OBSERVE_SERVICE_ATTACHMENT,
                ATTACHMENT_INSTRUMENT,
                ATTACHMENT_RESULTS,
                format!("{BASE_CLAIM_CEILING}; {ATTACHMENT_CLAIM_CEILING}"),
            )
        }
        "semantic_result" => {
            dimensions.push("activation.semantic.expectation".to_string());
            dimensions.push("root.selection".to_string());
            let claimed = row.expect == PERL_EXPECTATION;
            (
                vec![ROOT_SEMANTIC_DISCRIMINATOR, OBSERVE_SERVICE_ATTACHMENT],
                ROOT_SEMANTIC_DISCRIMINATOR,
                SEMANTIC_INSTRUMENT,
                if claimed { SEMANTIC_CLAIMED_RESULTS } else { SEMANTIC_UNCLAIMED_RESULTS },
                format!("{BASE_CLAIM_CEILING}; {SEMANTIC_CLAIM_CEILING}"),
            )
        }
        "ambiguity_preserved" => {
            dimensions.push("activation.adjacent_language".to_string());
            dimensions.push("activation.ambiguity.discriminator".to_string());
            (
                vec![
                    ROOT_SEMANTIC_DISCRIMINATOR,
                    OBSERVE_NATIVE_FILETYPE,
                    CLOSE_RESET_BETWEEN_ROWS,
                ],
                ROOT_SEMANTIC_DISCRIMINATOR,
                AMBIGUITY_INSTRUMENT,
                AMBIGUITY_RESULTS,
                format!("{BASE_CLAIM_CEILING}; {AMBIGUITY_CLAIM_CEILING}"),
            )
        }
        _ => unreachable!("activation aspects are pinned by ACTIVATION_ASPECTS"),
    };
    CellRegistration {
        cell_id: format!("{CELL_PREFIX}{}_{}", row.slug, aspect),
        cell_version: 1,
        scenario_owners: owners.iter().map(|value| value.to_string()).collect(),
        fixture_owners: ACTIVATION_FIXTURE_SUBSTRATE
            .iter()
            .map(|value| value.to_string())
            .collect(),
        subject,
        observation_class: observation_class.to_string(),
        subject_dimensions: dimensions,
        instrument_evidence: instrument.to_vec(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_results: results.iter().map(|value| value.to_string()).collect(),
        allowed_limitations: ACTIVATION_LIMITATION_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_profiles: vec![ACTIVATION_PROFILE.to_string()],
        claim_ceiling: ceiling,
    }
}

/// The expanded-activation scenario ledger, derived from the landed #11380
/// activation action vocabulary: one baseline scenario per action ID, sorted
/// for deterministic aggregation. #11376 owns the BDD scenario ledger; when it
/// lands, this derivation is superseded by a reviewed re-bind.
pub fn activation_action_ledger() -> ScenarioLedger {
    let mut scenarios: Vec<Scenario> = ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::Activation)
        .map(|action| Scenario { id: action.action_id.to_string(), class: ScenarioClass::Baseline })
        .collect();
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    ScenarioLedger {
        ledger_id: ACTIVATION_LEDGER_ID.to_string(),
        owning_authority: "#11380 specialized action vocabulary (PR #12204), activation family; denominator: #7762 vim-vim-lsp-activation-root.v1; supersedes pending: #11376 owns the BDD scenario ledger, #11378 the fixture/expectation cells"
            .to_string(),
        scenarios,
    }
}

/// The expanded-activation family catalog registered on this PR (#11388): the
/// finite #7762 denominator rows x the five activation aspects.
pub fn activation_catalog() -> CellCatalog {
    let subject = super::vim_vim_lsp_subject();
    let mut cells = Vec::with_capacity(ACTIVATION_DENOMINATOR.len() * ACTIVATION_ASPECTS.len());
    for row in ACTIVATION_DENOMINATOR {
        for &aspect in ACTIVATION_ASPECTS {
            cells.push(build_cell(row, aspect, subject.clone()));
        }
    }
    CellCatalog {
        catalog_id: ACTIVATION_CATALOG_ID.to_string(),
        catalog_version: 1,
        ledger_id: ACTIVATION_LEDGER_ID.to_string(),
        coverage: CoverageRule::AdditiveFamily,
        fixture_substrate: ACTIVATION_FIXTURE_SUBSTRATE
            .iter()
            .map(|value| value.to_string())
            .collect(),
        allowed_stages: vec![EvidenceStage::ExactSourceLocal],
        allowed_result_vocabulary: ACTIVATION_RESULT_VOCABULARY
            .iter()
            .map(|value| value.to_string())
            .collect(),
        core_profile: None,
        cells,
    }
}

/// The landed activation action IDs, as the family's authority set.
fn activation_action_ids() -> BTreeSet<&'static str> {
    ACTIONS
        .iter()
        .filter(|action| action.family == ActionFamily::Activation)
        .map(|action| action.action_id)
        .collect()
}

/// Validate the compiled activation catalog against the family laws.
pub fn validate_family_laws() -> Result<()> {
    validate_activation_catalog(&activation_catalog(), &activation_action_ledger())
}

/// Validate one activation-shaped catalog against the family laws. Shared-model
/// laws (subject pin, stage bound, duplicate IDs, ledger membership,
/// cross-catalog ownership) run in [`super::validate_registry`]; the laws here
/// are the ones only this family can state.
pub fn validate_activation_catalog(catalog: &CellCatalog, ledger: &ScenarioLedger) -> Result<()> {
    ensure!(
        catalog.catalog_id == ACTIVATION_CATALOG_ID,
        "activation family catalog must keep its identity {ACTIVATION_CATALOG_ID}, found {}",
        catalog.catalog_id
    );
    ensure!(
        catalog.ledger_id == ACTIVATION_LEDGER_ID && ledger.ledger_id == ACTIVATION_LEDGER_ID,
        "activation family must bind ledger {ACTIVATION_LEDGER_ID}"
    );
    ensure!(
        catalog.coverage == CoverageRule::AdditiveFamily,
        "activation family catalog is additive, not a baseline-coverage catalog"
    );
    ensure!(
        catalog.core_profile.is_none(),
        "activation family assigns no core profile; profiles consume cells, catalogs do not assign them"
    );
    ensure!(
        catalog.allowed_stages.len() == 1
            && catalog.allowed_stages[0] == EvidenceStage::ExactSourceLocal,
        "activation family stage bound is exact_source_local only; an exact-source cell cannot inherit a maintained/public stage"
    );
    let declared: BTreeSet<&str> =
        catalog.allowed_result_vocabulary.iter().map(String::as_str).collect();
    let expected: BTreeSet<&str> = ACTIVATION_RESULT_VOCABULARY.iter().copied().collect();
    ensure!(
        declared == expected,
        "activation result vocabulary drifted from the #11388 dispositions"
    );

    let actions = activation_action_ids();
    let scenarios: BTreeSet<&str> = ledger.scenarios.iter().map(|s| s.id.as_str()).collect();
    ensure!(
        scenarios == actions,
        "activation ledger must mirror exactly the landed #11380 activation actions; ledger has {} rows, vocabulary has {}",
        scenarios.len(),
        actions.len()
    );

    // The registered (row, aspect) pairs must be exactly denominator x aspects.
    let mut registered: BTreeSet<(String, &str)> = BTreeSet::new();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for cell in &catalog.cells {
        ensure!(
            cell.cell_id.starts_with(CELL_PREFIX),
            "cell {} is outside the activation family namespace {CELL_PREFIX}",
            cell.cell_id
        );
        let name = &cell.cell_id[CELL_PREFIX.len()..];
        let aspect_index = ACTIVATION_ASPECTS
            .iter()
            .position(|candidate| name.ends_with(&format!("_{}", candidate)))
            .with_context(|| {
                format!("cell {} does not end in a known #11388 activation aspect", cell.cell_id)
            })?;
        let aspect = ACTIVATION_ASPECTS[aspect_index];
        let slug = &name[..name.len() - aspect.len() - 1];
        let row = row_by_slug(slug).with_context(|| {
            format!(
                "cell {} names row {slug} outside the finite #7762 activation-root denominator; an ad hoc file-family row cannot register here",
                cell.cell_id
            )
        })?;
        ensure!(
            registered.insert((slug.to_string(), aspect)),
            "duplicate activation row-aspect registration {slug}::{aspect}"
        );

        ensure!(
            actions.contains(cell.observation_class.as_str()),
            "cell {} observation class {} is not a landed activation action; another family's action or an invented token cannot classify an activation cell",
            cell.cell_id,
            cell.observation_class
        );
        // Each aspect is classified by its one pinned action — not merely a
        // landed action the cell cites — so an attachment observation can
        // never classify a semantic cell even through a reviewed row edit.
        let required_class = ASPECT_OBSERVATION_CLASSES[aspect_index];
        ensure!(
            cell.observation_class == required_class,
            "cell {} must be classified by {required_class} for aspect {aspect}, found {}; the wrong activation proposition cannot satisfy this cell",
            cell.cell_id,
            cell.observation_class
        );
        ensure!(
            cell.scenario_owners.contains(&cell.observation_class),
            "cell {} observation class {} must be one of its own scenario owners",
            cell.cell_id,
            cell.observation_class
        );
        ensure!(
            cell.allowed_results.iter().any(|result| result == "fail")
                && cell.allowed_results.iter().any(|result| result == "not_proven"),
            "cell {} must admit fail and not_proven; honest failure and honest incompleteness are always expressible",
            cell.cell_id
        );
        for dimension in REQUIRED_DIMENSIONS {
            ensure!(
                cell.subject_dimensions.iter().any(|token| token == dimension),
                "cell {} must bind required dimension {dimension}",
                cell.cell_id
            );
        }

        // Row identity: exactly one activation.row.* dimension, matching the
        // cell's own slug, plus the row's artifact expectation — one row's
        // observation cannot inherit another row's identity.
        let row_dimensions: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(ROW_DIMENSION_PREFIX))
            .collect();
        ensure!(
            row_dimensions.len() == 1,
            "cell {} must bind exactly one {ROW_DIMENSION_PREFIX}* dimension",
            cell.cell_id
        );
        ensure!(
            row_dimensions[0].as_str() == format!("{ROW_DIMENSION_PREFIX}{slug}"),
            "cell {} binds row dimension {} which does not match its own row",
            cell.cell_id,
            row_dimensions[0]
        );
        ensure!(
            cell.subject_dimensions
                .iter()
                .any(|token| token == &format!("{EXPECT_DIMENSION_PREFIX}{}", row.expect)),
            "cell {} must bind the #7762 expectation dimension {EXPECT_DIMENSION_PREFIX}{} of its row {}",
            cell.cell_id,
            row.expect,
            row.slug
        );
        // Row authority identity: exactly one binding dimension, equal to the
        // digest over the row's full #7762 authority content, so denominator
        // edits of any field (path, controls, boundaries) are digest-visible.
        let binding = row_binding_identity(row);
        let bindings: Vec<&String> = cell
            .subject_dimensions
            .iter()
            .filter(|token| token.starts_with(ROW_BINDING_PREFIX))
            .collect();
        ensure!(
            bindings.len() == 1 && bindings[0].as_str() == binding,
            "cell {} must bind exactly one {ROW_BINDING_PREFIX}* dimension equal to its row's authority identity {binding}; a #7762 denominator edit cannot stay digest-invisible",
            cell.cell_id
        );

        // Semantic honesty: a semantic cell of a row whose #7762 expectation
        // is not perl never admits a semantic-support-affirming result, so an
        // adjacent-language false subject that attaches successfully still
        // fails the semantic proposition. (An ambiguity cell's
        // `native_supported` is the preservation disposition — the adjacent
        // language survived — and stays admitted there.)
        if aspect == "semantic_result" && row.expect != PERL_EXPECTATION {
            for token in SEMANTIC_AFFIRMING_RESULTS {
                ensure!(
                    !cell.allowed_results.iter().any(|result| result == token),
                    "cell {} of non-perl row {} admits the semantic-support-affirming result {token}; filetype/attachment/override can never be relabeled semantic support",
                    cell.cell_id,
                    row.slug
                );
            }
        }

        // Aspect vocabularies are pinned: filetype, override, attachment,
        // semantic, and ambiguity dispositions cannot stand in for each other.
        let expected_results: BTreeSet<&str> = match aspect {
            "native_filetype" => NATIVE_FILETYPE_RESULTS.iter().copied().collect(),
            "override" => OVERRIDE_RESULTS.iter().copied().collect(),
            "attachment" => ATTACHMENT_RESULTS.iter().copied().collect(),
            "semantic_result" => {
                if row.expect == PERL_EXPECTATION {
                    SEMANTIC_CLAIMED_RESULTS.iter().copied().collect()
                } else {
                    SEMANTIC_UNCLAIMED_RESULTS.iter().copied().collect()
                }
            }
            "ambiguity_preserved" => AMBIGUITY_RESULTS.iter().copied().collect(),
            _ => unreachable!("aspect checked above"),
        };
        let declared_results: BTreeSet<&str> =
            cell.allowed_results.iter().map(String::as_str).collect();
        ensure!(
            declared_results == expected_results,
            "cell {} allowed results drifted from the pinned {aspect} aspect vocabulary of row {}",
            cell.cell_id,
            row.slug
        );

        // An override row carrying a manual_override boundary keeps its
        // not-authorized limitation.
        if aspect == "override"
            && let Some(boundary) = row.manual_override
        {
            ensure!(
                cell.allowed_limitations.iter().any(|token| token == boundary),
                "cell {} must keep the {boundary} limitation of its #7762 row; an extension-only override without its receipt cannot lose the boundary",
                cell.cell_id
            );
        }

        // Cells citing the between-rows reset action keep cleanup evidence
        // independently load-bearing.
        for owner in CLEANUP_REQUIRING_OWNERS {
            if cell.scenario_owners.iter().any(|token| token == owner) {
                ensure!(
                    cell.instrument_evidence.contains(&InstrumentEvidence::CleanupObservation),
                    "cell {} cites the between-rows reset action {owner} and must require cleanup evidence; activation rows cannot contaminate each other silently",
                    cell.cell_id
                );
            }
        }

        ensure!(
            cell.allowed_profiles.len() == 1 && cell.allowed_profiles[0] == ACTIVATION_PROFILE,
            "cell {} may feed only {ACTIVATION_PROFILE}",
            cell.cell_id
        );
        covered.extend(cell.scenario_owners.iter().cloned());
    }

    let mut expected_pairs: BTreeSet<(String, &str)> = BTreeSet::new();
    for row in ACTIVATION_DENOMINATOR {
        for &aspect in ACTIVATION_ASPECTS {
            expected_pairs.insert((row.slug.to_string(), aspect));
        }
    }
    let missing: Vec<(String, &str)> = expected_pairs.difference(&registered).cloned().collect();
    ensure!(
        missing.is_empty(),
        "denominator row-aspect cells missing from the #11388 activation family: {missing:?}"
    );

    let uncovered: Vec<&str> =
        actions.iter().filter(|action| !covered.contains(**action)).copied().collect();
    ensure!(
        uncovered.is_empty(),
        "landed activation actions without a pre-registered cell: {uncovered:?}"
    );
    Ok(())
}
