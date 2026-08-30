//! Contract tests for the governed Emacs host-journey and fixture/cell
//! manifest (#11768).
//!
//! Falsifier coverage, keyed to the issue's "First falsifiers":
//!
//! 1. one diagnostic cohort cannot inherit another's cells (pull-protocol
//!    surfaces are exclusive to the standalone-Eglot-pull cohort);
//! 2. a protocol frame cannot count as host-visible semantic success
//!    (protocol-membership rows must carry their mandatory limitation and can
//!    never present host-visible surfaces, and vice versa);
//! 3. cell membership is registration only: validating proves no behavior;
//! 4. an Emacs-local expected answer cannot replace canonical truth: unknown
//!    expectation ids fail closed;
//! 5. an actual-host leaf cannot introduce an unregistered free-form passing
//!    cell (ids are namespace- and class-bound; classes are registered);
//! 6. optional #9413 feature depth cannot silently become core-required;
//! 7. root material cannot be duplicated or manual roots pass as stock (only
//!    `root_11366.<role>` reference tokens are expressible);
//! 8. wrong server/client/version/source state cannot satisfy a cell
//!    (unknown controls/dimensions/coordinates/limitations fail closed);
//! 9. digest identity covers every binding field, so membership or control
//!    edits are visible and second-run output is byte-stable.

use anyhow::{Result, bail, ensure};
use xtask::editor_client_compat::EvidenceStage;
use xtask::emacs_host_journeys::{
    self, DepthClass, DiagnosticCohort, EvidenceKind, ExpectationRef, HostSurface, JourneyCell,
    ROOT_ROLE_TOKENS, RootReference,
};

fn compiled() -> Vec<JourneyCell> {
    emacs_host_journeys::registry()
}

fn find<'a>(cells: &'a [JourneyCell], id: &str) -> Result<&'a JourneyCell> {
    cells
        .iter()
        .find(|cell| cell.cell_id == id)
        .ok_or_else(|| anyhow::anyhow!("test bug: missing known published cell {id}"))
}

#[test]
fn compiled_registry_validates_and_is_second_run_clean() -> Result<()> {
    let first = emacs_host_journeys::validate_compiled_registry()?;
    let second = emacs_host_journeys::validate_compiled_registry()?;
    ensure!(
        first == second,
        "registry validation output changed between runs: {:?} vs {second:?}",
        first
    );
    // Cohort independence is published explicitly rather than inherited.
    for (_, count) in &first.cohort_membership {
        ensure!(*count > 0, "a diagnostic cohort holds zero membership");
    }
    ensure!(
        first.optional_cell_count >= 3,
        "the #9413 optional documented-feature families must stay registered"
    );
    Ok(())
}

#[test]
fn every_fixture_owner_resolves_to_the_landed_subject_authority() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("xtask must live below the repository root"))?;
    for cell in compiled() {
        for fixture in &cell.fixture_owners {
            let path = root.join(".ci/editor-clients").join(format!("{fixture}.json"));
            ensure!(
                path.exists(),
                "cell {} binds fixture authority {} that is absent from the tree",
                cell.cell_id,
                fixture
            );
        }
    }
    Ok(())
}

#[test]
fn pull_protocol_surfaces_reject_push_and_lsp_mode_cohorts() -> Result<()> {
    let mut cells = compiled();
    let poll_cell_id = "emacs.diagnostics_pull_protocol.poll_request_full_result_id";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == poll_cell_id)
        .ok_or_else(|| anyhow::anyhow!("pull poll cell vanished from the registry"))?;
    cells[position].cohorts =
        vec![DiagnosticCohort::StandaloneEglotPull, DiagnosticCohort::BundledEglotPush];
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("push cohort inherited a pull-protocol cell"),
    };
    ensure!(
        error.contains("standalone_eglot_pull"),
        "unexpected rejection for mixed-cohort pull cell: {error}"
    );
    Ok(())
}

#[test]
fn protocol_membership_can_never_present_host_visible_semantics() -> Result<()> {
    let mut cells = compiled();
    let poll_cell_id = "emacs.diagnostics_pull_protocol.final_clear";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == poll_cell_id)
        .ok_or_else(|| anyhow::anyhow!("final-clear cell vanished from the registry"))?;
    // Dropping the mandatory protocol limitation manufactures a semantic-pass
    // capable protocol row.
    cells[position].allowed_limitations.clear();
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("protocol-membership cell survived without its limitation"),
    };
    ensure!(error.contains("protocol"), "unexpected rejection text: {error}");

    // The inverse lie: a host-visible cell claiming only a protocol surface.
    // Mutate one published row in place (and confine it to the pull cohort so
    // the pull-cohort law passes) so the rejection can only come from the
    // host-visible surface law at `validate_cell`, never from a
    // baseline-coverage law triggered by removing rows.
    let mut cells = compiled();
    let host_visible = "emacs.eldoc_hover_observation.hover_rendered";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == host_visible)
        .ok_or_else(|| anyhow::anyhow!("hover cell vanished from the registry"))?;
    cells[position].host_surfaces = vec![HostSurface::DiagnosticsPollProtocol];
    cells[position].cohorts = vec![DiagnosticCohort::StandaloneEglotPull];
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("host-visible row accepted with protocol-only surfaces"),
    };
    ensure!(
        error.contains("requires at least one host-visible surface"),
        "unexpected rejection for protocol-only host-visible row: {error}"
    );
    Ok(())
}

#[test]
fn protocol_membership_rejects_host_visible_surface() -> Result<()> {
    let mut cells = compiled();
    let poll_cell_id = "emacs.diagnostics_pull_protocol.final_clear";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == poll_cell_id)
        .ok_or_else(|| anyhow::anyhow!("final-clear cell vanished from the registry"))?;
    cells[position].host_surfaces.push(HostSurface::EldocHoverObservation);
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("protocol-membership cell accepted a host-visible surface"),
    };
    ensure!(
        error.contains("must not expose host-visible surfaces"),
        "unexpected rejection for host-visible protocol surface: {error}"
    );
    Ok(())
}

#[test]
fn claim_ceiling_must_match_registered_depth_and_evidence_kind() -> Result<()> {
    let cells = compiled();
    let core_position = cells
        .iter()
        .position(|cell| cell.depth == DepthClass::Core)
        .ok_or_else(|| anyhow::anyhow!("core cell vanished from the registry"))?;
    let optional_ceiling = cells
        .iter()
        .find(|cell| cell.depth == DepthClass::Optional)
        .map(|cell| cell.claim_ceiling.clone())
        .ok_or_else(|| anyhow::anyhow!("optional cell vanished from the registry"))?;

    let mut cells = cells;
    cells[core_position].claim_ceiling =
        "registration only: plausible but unregistered ceiling".to_string();
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("core cell accepted an unregistered claim ceiling"),
    };
    ensure!(
        error.contains("not the registered ceiling for its depth/evidence kind"),
        "unexpected rejection for an unregistered claim ceiling: {error}"
    );

    let mut cells = compiled();
    let core_position = cells
        .iter()
        .position(|cell| cell.depth == DepthClass::Core)
        .ok_or_else(|| anyhow::anyhow!("core cell vanished from the registry"))?;
    cells[core_position].claim_ceiling = optional_ceiling;
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("core cell accepted an optional claim ceiling"),
    };
    ensure!(
        error.contains("not the registered ceiling for its depth/evidence kind"),
        "unexpected rejection for an optional claim ceiling on a core cell: {error}"
    );
    Ok(())
}

#[test]
fn public_artifact_evidence_is_not_admitted() -> Result<()> {
    let mut cells = compiled();
    cells[0].max_stage = EvidenceStage::PublicArtifact;
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("cell accepted public-artifact evidence"),
    };
    ensure!(
        error.contains("may not claim public-artifact evidence"),
        "unexpected rejection for public-artifact evidence: {error}"
    );
    Ok(())
}

#[test]
fn root_roles_are_closed_and_registered_roles_are_accepted() -> Result<()> {
    let mut cells = compiled();
    let position = cells
        .iter()
        .position(|cell| cell.root_reference.is_some())
        .ok_or_else(|| anyhow::anyhow!("root-sensitive cell vanished from the registry"))?;
    cells[position].root_reference =
        Some(RootReference { role_token: "root_11366.stok_project".to_string() });
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("misspelled root role passed validation"),
    };
    ensure!(
        error.contains("unregistered root_11366 role stok_project"),
        "unexpected rejection for a misspelled root role: {error}"
    );

    ensure!(ROOT_ROLE_TOKENS.len() == 2, "root role vocabulary changed unexpectedly");
    for role in ROOT_ROLE_TOKENS {
        let mut cells = compiled();
        let position = cells
            .iter()
            .position(|cell| cell.root_reference.is_some())
            .ok_or_else(|| anyhow::anyhow!("root-sensitive cell vanished from the registry"))?;
        cells[position].root_reference =
            Some(RootReference { role_token: format!("root_11366.{role}") });
        emacs_host_journeys::validate_registry(&cells)?;
    }
    Ok(())
}

#[test]
fn missing_canonical_truth_blocks_the_cell() -> Result<()> {
    let mut cells = compiled();
    cells[0].expectation_owner = ExpectationRef {
        set_id: "perl-agent-client-v1".to_string(),
        ids: vec!["invented.emacs_local_answer".to_string()],
    };
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("an Emacs-local invented expectation satisfied a cell"),
    };
    ensure!(
        error.contains("unknown canonical expectation id"),
        "unexpected rejection for invented expectation: {error}"
    );

    let mut cells = compiled();
    cells[0].expectation_owner.set_id = "emacs-local-oracle-v9".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a private expectation set replaced the canonical owner"
    );
    Ok(())
}

#[test]
fn unregistered_free_form_cells_are_unrepresentable() -> Result<()> {
    let mut cells = compiled();
    cells[0].cell_id = "emacs.not_a_registered_class.free_form_leaf".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "an unregistered journey class entered the manifest"
    );

    let mut cells = compiled();
    cells[0].journey_class = "workspace_readiness".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a row whose id segment disagrees with its declared class validated"
    );

    let mut cells = compiled();
    cells[0].producer_mapping = "local.mapping.not_11361".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a producer mapping outside #11361 entered a cell"
    );
    Ok(())
}

#[test]
fn optional_feature_depth_cannot_silently_become_core_required() -> Result<()> {
    let mut cells = compiled();
    let opt_id = "emacs.opt_native_formatting.format_document_depth";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == opt_id)
        .ok_or_else(|| anyhow::anyhow!("optional formatting cell vanished"))?;
    cells[position].depth = DepthClass::Core;
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("optional depth crossed into a core class"),
    };
    ensure!(error.contains("mixes depth"), "unexpected rejection: {error}");
    Ok(())
}

#[test]
fn root_references_stay_role_tokens_owned_by_11366() -> Result<()> {
    let mut cells = compiled();
    cells[0].root_reference =
        Some(RootReference { role_token: "../../fixtures/stock-project/root.toml".to_string() });
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "root fixture material leaked into the manifest as a path"
    );

    let mut cells = compiled();
    cells[0].root_reference = Some(RootReference { role_token: "manual_root".to_string() });
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a root token outside the root_11366 namespace passed as authority"
    );

    // Every registered root-sensitive row stays inside the #11366 namespace.
    for cell in compiled() {
        if let Some(root) = &cell.root_reference {
            ensure!(
                root.role_token.starts_with("root_11366."),
                "cell {} carries non-authority root token {}",
                cell.cell_id,
                root.role_token
            );
        }
    }
    Ok(())
}

#[test]
fn wrong_subject_state_vocabulary_fails_closed() -> Result<()> {
    let mut cells = compiled();
    cells[0].false_subject_controls.push("totally_unknown_control".to_string());
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "an unknown false-subject control entered a cell"
    );

    let mut cells = compiled();
    cells[0].dimensions.push("document.invented_dimension".to_string());
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "an unknown generation dimension entered a cell"
    );

    let mut cells = compiled();
    cells[0].allowed_limitations.push("synthetic_success".to_string());
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a synthetic-success limitation escaped the bounded vocabulary"
    );

    let mut cells = compiled();
    cells[0].false_subject_controls.clear();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a cell without any false-subject control validated"
    );
    Ok(())
}

#[test]
fn stale_generation_and_partial_edit_controls_are_registered_on_their_classes() -> Result<()> {
    for cell in compiled() {
        if cell.journey_class == "stale_generation_rejection" {
            ensure!(
                cell.false_subject_controls
                    .iter()
                    .any(|control| control == "prior_generation_stale_result"),
                "{} lost its prior-generation control",
                cell.cell_id
            );
            ensure!(
                cell.host_surfaces.contains(&HostSurface::StaleResultRejection),
                "{} lost its required host surface",
                cell.cell_id
            );
        }
        if cell.journey_class == "multi_file_rename_workspace_edit" {
            ensure!(
                cell.false_subject_controls
                    .iter()
                    .any(|control| control == "partial_multi_file_edit_or_result"),
                "{} lost its partial-edit control",
                cell.cell_id
            );
        }
        if cell.journey_class == "diagnostics_host_visibility" {
            ensure!(
                !cell.cohorts.is_empty()
                    && cell.host_surfaces.contains(&HostSurface::FlymakeDiagnosticLifecycle),
                "{} is not bound to host-visible Flymake state",
                cell.cell_id
            );
        }
    }
    // The pull contract keeps its exact protocol cells present.
    let pull_ids = [
        "emacs.diagnostics_pull_protocol.poll_request_full_result_id",
        "emacs.diagnostics_pull_protocol.previous_result_id_roundtrip",
        "emacs.diagnostics_pull_protocol.unchanged_result_reported",
        "emacs.diagnostics_pull_protocol.edit_invalidation_new_identity",
        "emacs.diagnostics_pull_protocol.final_clear",
    ];
    for id in pull_ids {
        let cells = compiled();
        let cell = find(&cells, id)?;
        ensure!(
            cell.evidence_kind == EvidenceKind::ProtocolMembershipOnly,
            "{id} lost protocol-membership kind"
        );
    }
    Ok(())
}

#[test]
fn digests_cover_bindings_and_survive_row_ordering_only_changes() -> Result<()> {
    let cells = compiled();
    let baseline = emacs_host_journeys::registry_digest(&cells)?;

    // Reordering list fields inside one row is not a semantic change...
    let mut reordered = cells.clone();
    reordered[0].false_subject_controls.reverse();
    ensure!(
        emacs_host_journeys::cell_digest(&reordered[0])?
            == emacs_host_journeys::cell_digest(&cells[0])?,
        "digest depends on control ordering"
    );

    // ...but any binding edit is a visible identity change.
    let mut edited = cells.clone();
    edited[0].positive_discriminator.insert_str(0, "altered ");
    ensure!(
        emacs_host_journeys::cell_digest(&edited[0])?
            != emacs_host_journeys::cell_digest(&cells[0])?,
        "digest ignored a discriminator edit"
    );

    let mut removed = cells.clone();
    removed.remove(0);
    ensure!(
        emacs_host_journeys::registry_digest(&removed)? != baseline,
        "registry digest ignored a removed row"
    );
    Ok(())
}

#[test]
fn lookup_resolves_cells_classes_and_rejects_unknown_subjects() -> Result<()> {
    let cells = compiled();
    let (class, matched) =
        emacs_host_journeys::lookup(&cells, "emacs.mode_attachment.perl_mode_language_id")?;
    ensure!(class.as_deref() == Some("mode_attachment"));
    ensure!(matched.len() == 1);

    let (class, matched) = emacs_host_journeys::lookup(&cells, "diagnostics_pull_protocol")?;
    ensure!(class.as_deref() == Some("diagnostics_pull_protocol"));
    ensure!(matched.len() >= 5, "pull protocol class lost its exact cells");

    ensure!(
        emacs_host_journeys::lookup(&cells, "emacs.nope.unknown").is_err(),
        "lookup accepted an unknown cell"
    );
    Ok(())
}

#[test]
fn root_sensitive_cells_fail_closed_without_a_root_reference() -> Result<()> {
    let mut cells = compiled();
    let stock_root = "emacs.workspace_readiness.stock_root_ready";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == stock_root)
        .ok_or_else(|| anyhow::anyhow!("stock-root cell vanished from the registry"))?;
    // Removing the reference must be a rejection, not a silent loss of root
    // governance: the cell requires a discovery-generation distinction.
    cells[position].root_reference = None;
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("root-sensitive row accepted without a root_11366 reference"),
    };
    ensure!(
        error.contains("root.discovery_generation"),
        "unexpected rejection for reference-less root-sensitive row: {error}"
    );
    Ok(())
}

#[test]
fn unknown_producer_mappings_fail_closed() -> Result<()> {
    let mut cells = compiled();
    let hover = "emacs.eldoc_hover_observation.hover_rendered";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == hover)
        .ok_or_else(|| anyhow::anyhow!("hover cell vanished from the registry"))?;
    // Namespace syntax alone must not bless an invented mapping: only the
    // registered #11361 observation → receipt vocabulary is citable.
    cells[position].producer_mapping = "11361.unowned_mapping".to_string();
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("cell cited an unregistered #11361 producer mapping"),
    };
    ensure!(
        error.contains("unregistered #11361 producer mapping"),
        "unexpected rejection for invented producer mapping: {error}"
    );
    Ok(())
}

#[test]
fn production_check_fails_closed_without_landed_subject_authority() -> Result<()> {
    use xtask::emacs_host_journeys::validate_compiled_registry_against;

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("xtask must live below the repository root"))?;
    let manifest_bytes = std::fs::read(repo_root.join(".ci/editor-clients/emacs-subjects.v1.json"))
        .map_err(|error| anyhow::anyhow!("subject authority vanished from the tree: {error}"))?;

    // Missing manifest: the production path must reject, not bless.
    let empty = tempfile::tempdir()?;
    let error = match validate_compiled_registry_against(empty.path()) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("production check succeeded with the subject manifest deleted"),
    };
    ensure!(
        error.contains("missing or unreadable"),
        "unexpected rejection for a deleted subject manifest: {error}"
    );

    // Malformed manifest: the production path must reject, not bless.
    let malformed = tempfile::tempdir()?;
    let clients = malformed.path().join(".ci/editor-clients");
    std::fs::create_dir_all(&clients)?;
    std::fs::write(clients.join("emacs-subjects.v1.json"), b"{ not json")?;
    let error = match validate_compiled_registry_against(malformed.path()) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("production check succeeded with a malformed subject manifest"),
    };
    ensure!(
        error.contains("not valid JSON"),
        "unexpected rejection for a malformed subject manifest: {error}"
    );

    // The landed authority itself still validates end to end.
    let landed = tempfile::tempdir()?;
    let clients = landed.path().join(".ci/editor-clients");
    std::fs::create_dir_all(&clients)?;
    std::fs::write(clients.join("emacs-subjects.v1.json"), &manifest_bytes)?;
    validate_compiled_registry_against(landed.path())?;
    Ok(())
}
