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
    MANIFEST_SCHEMA_VERSION, PLATFORM_APPLICABILITY_TOKENS, ROOT_ROLE_TOKENS, RootReference,
};

fn compiled() -> Result<Vec<JourneyCell>> {
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
    for count in first.cohort_membership.values() {
        ensure!(*count > 0, "a diagnostic cohort holds zero membership");
    }
    ensure!(
        first.optional_cell_count == 1,
        "only code-action optional depth has landed canonical truth"
    );
    Ok(())
}

#[test]
fn every_fixture_owner_resolves_to_the_landed_subject_authority() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("xtask must live below the repository root"))?;
    for cell in &compiled()? {
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
    let mut cells = compiled()?;
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
    let mut cells = compiled()?;
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
    let mut cells = compiled()?;
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
    let mut cells = compiled()?;
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
fn host_visible_cell_rejects_a_mixed_protocol_surface() -> Result<()> {
    // The mixed lie the protocol-only inverse cannot catch: a genuinely
    // host-visible cell that also binds the protocol-only surface, so a
    // protocol frame could supply evidence for a host-visible claim.
    let mut cells = compiled()?;
    let host_visible = "emacs.eldoc_hover_observation.hover_rendered";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == host_visible)
        .ok_or_else(|| anyhow::anyhow!("hover cell vanished from the registry"))?;
    cells[position].host_surfaces.push(HostSurface::DiagnosticsPollProtocol);
    // Confine it to the pull cohort so the pull-cohort law cannot be the
    // rejecting law; only the evidence-partition law can fire here.
    cells[position].cohorts = vec![DiagnosticCohort::StandaloneEglotPull];
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("host-visible row accepted a protocol-only surface"),
    };
    ensure!(
        error.contains("must not expose protocol-only surfaces"),
        "unexpected rejection for mixed host-visible row: {error}"
    );
    Ok(())
}

#[test]
fn core_cell_rejects_deferred_schema_surfaces() -> Result<()> {
    // Formatting and inlay hints have host-surface variants but no registered
    // class: the generic host-visible checks accept them, and the
    // feature-specific surface guard runs only for optional depth. A core row
    // binding one would certify a surface the schema explicitly defers.
    let mut cells = compiled()?;
    let host_visible = "emacs.eldoc_hover_observation.hover_rendered";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == host_visible)
        .ok_or_else(|| anyhow::anyhow!("hover cell vanished from the registry"))?;
    cells[position].host_surfaces = vec![HostSurface::DocumentFormattingApplication];
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("core row accepted a deferred-schema surface"),
    };
    ensure!(
        error.contains("deferred-schema"),
        "unexpected rejection for deferred-schema surface on a core row: {error}"
    );
    Ok(())
}

#[test]
fn platform_applicability_uses_a_closed_vocabulary() -> Result<()> {
    ensure!(
        PLATFORM_APPLICABILITY_TOKENS == ["all"],
        "platform applicability vocabulary changed unexpectedly"
    );

    let mut cells = compiled()?;
    cells[0].platform_applicability = "x86_64-unknown-linux-gnu".to_string();
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("an unregistered platform applicability token passed validation"),
    };
    ensure!(
        error.contains("unregistered platform applicability")
            && error.contains("x86_64-unknown-linux-gnu"),
        "unexpected rejection for an unregistered platform applicability: {error}"
    );

    let mut cells = compiled()?;
    cells[0].platform_applicability = "all".to_string();
    emacs_host_journeys::validate_registry(&cells)?;
    Ok(())
}

#[test]
fn claim_ceiling_must_match_registered_depth_and_evidence_kind() -> Result<()> {
    let cells = compiled()?;
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

    let mut cells = compiled()?;
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
    let mut cells = compiled()?;
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
    let mut cells = compiled()?;
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
        let mut cells = compiled()?;
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
    let mut cells = compiled()?;
    cells[0].expectation_owner = ExpectationRef {
        set_id: "perl-agent-client-v1".to_string(),
        set_digest: cells[0].expectation_owner.set_digest.clone(),
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

    let mut cells = compiled()?;
    cells[0].expectation_owner.set_id = "emacs-local-oracle-v9".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a private expectation set replaced the canonical owner"
    );
    Ok(())
}

#[test]
fn stale_expectation_digest_blocks_the_cell() -> Result<()> {
    let mut cells = compiled()?;
    cells[0].expectation_owner.set_digest =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("a stale expectation-set digest passed validation"),
    };
    ensure!(
        error.contains("stale or foreign canonical expectation-set digest"),
        "unexpected rejection for a stale expectation-set digest: {error}"
    );
    Ok(())
}

#[test]
fn unregistered_free_form_cells_are_unrepresentable() -> Result<()> {
    let mut cells = compiled()?;
    cells[0].cell_id = "emacs.not_a_registered_class.free_form_leaf".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "an unregistered journey class entered the manifest"
    );

    let mut cells = compiled()?;
    cells[0].journey_class = "workspace_readiness".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a row whose id segment disagrees with its declared class validated"
    );

    let mut cells = compiled()?;
    cells[0].producer_mapping = "local.mapping.not_11361".to_string();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a producer mapping outside #11361 entered a cell"
    );
    Ok(())
}

#[test]
fn optional_feature_depth_cannot_silently_become_core_required() -> Result<()> {
    let mut cells = compiled()?;
    let opt_id = "emacs.opt_code_action_application.apply_or_refuse_depth";
    let position = cells
        .iter()
        .position(|cell| cell.cell_id == opt_id)
        .ok_or_else(|| anyhow::anyhow!("optional code-action cell vanished"))?;
    cells[position].depth = DepthClass::Core;
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("optional depth crossed into a core class"),
    };
    ensure!(error.contains("mixes depth"), "unexpected rejection: {error}");
    Ok(())
}

#[test]
fn core_only_registry_does_not_require_optional_depth() -> Result<()> {
    let core_only: Vec<_> =
        compiled()?.into_iter().filter(|cell| cell.depth == DepthClass::Core).collect();
    let summary = emacs_host_journeys::validate_registry(&core_only)?;
    ensure!(
        summary.optional_cell_count == 0 && summary.core_cell_count == 21,
        "core-only registry must not require optional feature depth"
    );
    Ok(())
}

#[test]
fn root_references_stay_role_tokens_owned_by_11366() -> Result<()> {
    let mut cells = compiled()?;
    cells[0].root_reference =
        Some(RootReference { role_token: "../../fixtures/stock-project/root.toml".to_string() });
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "root fixture material leaked into the manifest as a path"
    );

    let mut cells = compiled()?;
    cells[0].root_reference = Some(RootReference { role_token: "manual_root".to_string() });
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a root token outside the root_11366 namespace passed as authority"
    );

    // Every registered root-sensitive row stays inside the #11366 namespace.
    for cell in &compiled()? {
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
    let mut cells = compiled()?;
    cells[0].false_subject_controls.push("totally_unknown_control".to_string());
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "an unknown false-subject control entered a cell"
    );

    let mut cells = compiled()?;
    cells[0].dimensions.push("document.invented_dimension".to_string());
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "an unknown generation dimension entered a cell"
    );

    let mut cells = compiled()?;
    cells[0].allowed_limitations.push("synthetic_success".to_string());
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a synthetic-success limitation escaped the bounded vocabulary"
    );

    let mut cells = compiled()?;
    cells[0].false_subject_controls.clear();
    ensure!(
        emacs_host_journeys::validate_registry(&cells).is_err(),
        "a cell without any false-subject control validated"
    );
    Ok(())
}

#[test]
fn stale_generation_and_partial_edit_controls_are_registered_on_their_classes() -> Result<()> {
    for cell in &compiled()? {
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
        let cells = compiled()?;
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
    let cells = compiled()?;
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
fn cell_digest_moves_for_every_identity_bearing_field() -> Result<()> {
    // The digest is the registry's identity. `cell_digest` builds a private
    // view struct, so a future edit could drop a field from that view while
    // leaving `JourneyCell` intact and no other test would go red. Mutate each
    // field exactly once and require the digest to move.
    type Mutator = fn(&mut JourneyCell);
    let mutations: &[(&str, Mutator)] = &[
        ("cell_id", |c| c.cell_id.insert_str(0, "altered.")),
        ("cell_version", |c| c.cell_version += 1),
        ("journey_class", |c| c.journey_class.insert_str(0, "altered_")),
        ("depth", |c| {
            c.depth = match c.depth {
                DepthClass::Core => DepthClass::Optional,
                DepthClass::Optional => DepthClass::Core,
            }
        }),
        ("cohorts", |c| c.cohorts.clear()),
        ("fixture_owners", |c| c.fixture_owners.push("altered_fixture".to_string())),
        ("fixture_set_digest", |c| c.fixture_set_digest.push_str("altered")),
        ("expectation_owner.set_id", |c| c.expectation_owner.set_id.push_str(".altered")),
        ("expectation_owner.ids", |c| {
            c.expectation_owner.ids.push("altered.expectation".to_string())
        }),
        ("expectation_owner.set_digest", |c| {
            c.expectation_owner.set_digest.insert_str(0, "altered")
        }),
        ("root_reference", |c| {
            c.root_reference = match c.root_reference.take() {
                Some(_) => None,
                None => Some(RootReference { role_token: ROOT_ROLE_TOKENS[0].to_string() }),
            }
        }),
        ("dimensions", |c| c.dimensions.push("altered_dimension".to_string())),
        ("host_surfaces", |c| c.host_surfaces.push(HostSurface::StaleResultRejection)),
        ("evidence_kind", |c| {
            c.evidence_kind = match c.evidence_kind {
                EvidenceKind::HostVisibleObservation => EvidenceKind::ProtocolMembershipOnly,
                EvidenceKind::ProtocolMembershipOnly => EvidenceKind::HostVisibleObservation,
            }
        }),
        ("positive_discriminator", |c| c.positive_discriminator.insert_str(0, "altered ")),
        ("false_subject_controls", |c| {
            c.false_subject_controls.push("altered_control".to_string())
        }),
        ("coordinate_domains", |c| c.coordinate_domains.push("altered_domain".to_string())),
        ("platform_applicability", |c| c.platform_applicability.insert_str(0, "altered_")),
        ("allowed_limitations", |c| c.allowed_limitations.push("altered_limitation".to_string())),
        ("max_stage", |c| {
            c.max_stage = match c.max_stage {
                EvidenceStage::PublicArtifact => EvidenceStage::ReleaseCandidate,
                _ => EvidenceStage::PublicArtifact,
            }
        }),
        ("claim_ceiling", |c| c.claim_ceiling.insert_str(0, "altered ")),
        ("producer_mapping", |c| c.producer_mapping.insert_str(0, "altered_")),
    ];

    let cells = compiled()?;
    let baseline_registry = emacs_host_journeys::registry_digest(&cells)?;
    for (field, mutate) in mutations {
        for index in 0..cells.len() {
            let baseline_cell = emacs_host_journeys::cell_digest(&cells[index])?;
            let mut edited = cells.clone();
            mutate(&mut edited[index]);
            if edited[index] == cells[index] {
                // The mutation was a no-op on this row (an already-absent
                // option, an identical stage); it proves nothing here.
                continue;
            }
            ensure!(
                emacs_host_journeys::cell_digest(&edited[index])? != baseline_cell,
                "cell digest ignored a change to {field} on row {}",
                cells[index].cell_id
            );
            ensure!(
                emacs_host_journeys::registry_digest(&edited)? != baseline_registry,
                "registry digest ignored a change to {field} on row {}",
                cells[index].cell_id
            );
            break;
        }
    }
    Ok(())
}

#[test]
fn registry_digest_covers_expectation_set_digest() -> Result<()> {
    let cells = compiled()?;
    let baseline = emacs_host_journeys::registry_digest(&cells)?;
    let mut edited = cells;
    edited[0].expectation_owner.set_digest =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111".to_string();
    ensure!(
        emacs_host_journeys::registry_digest(&edited)? != baseline,
        "registry digest ignored an expectation-set digest edit"
    );
    Ok(())
}

#[test]
fn lookup_resolves_cells_classes_and_rejects_unknown_subjects() -> Result<()> {
    let cells = compiled()?;
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
    let mut cells = compiled()?;
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
    let mut cells = compiled()?;
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

    // A structurally valid manifest that omits one governed subject must be
    // rejected by the denominator gate rather than merely accepted as
    // parseable subject authority.
    let incomplete = tempfile::tempdir()?;
    let clients = incomplete.path().join(".ci/editor-clients");
    std::fs::create_dir_all(&clients)?;
    let mut manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    let subjects = manifest
        .get_mut("subjects")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("subject manifest has no subjects array"))?;
    let missing_subject = "bundled_eglot_emacs_29_4";
    let position = subjects
        .iter()
        .position(|subject| {
            subject.get("subject_id").and_then(serde_json::Value::as_str) == Some(missing_subject)
        })
        .ok_or_else(|| anyhow::anyhow!("governed subject {missing_subject} vanished"))?;
    subjects.remove(position);
    std::fs::write(clients.join("emacs-subjects.v1.json"), serde_json::to_vec_pretty(&manifest)?)?;
    let error = match validate_compiled_registry_against(incomplete.path()) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("production check accepted an incomplete subject denominator"),
    };
    ensure!(
        error.contains("does not certify the governed client set")
            && !error.contains("missing or unreadable")
            && !error.contains("not valid JSON"),
        "unexpected rejection for an incomplete subject denominator: {error}"
    );

    // The landed authority itself still validates end to end.
    let landed = tempfile::tempdir()?;
    let clients = landed.path().join(".ci/editor-clients");
    std::fs::create_dir_all(&clients)?;
    std::fs::write(clients.join("emacs-subjects.v1.json"), &manifest_bytes)?;
    validate_compiled_registry_against(landed.path())?;
    Ok(())
}

#[test]
fn cells_must_carry_a_positive_version() -> Result<()> {
    // The `cell_version >= 1` law is otherwise unfalsified: every compiled row
    // carries version 1, so a validator that dropped the check entirely would
    // still satisfy the rest of this suite.
    let mut cells = compiled()?;
    cells[0].cell_version = 0;
    let error = match emacs_host_journeys::validate_registry(&cells) {
        Err(error) => error.to_string(),
        Ok(_) => bail!("a cell carrying version 0 passed validation"),
    };
    ensure!(
        error.contains("must carry a positive version") && error.contains(&cells[0].cell_id),
        "unexpected rejection for a zero cell version: {error}"
    );

    let mut cells = compiled()?;
    cells[0].cell_version = 1;
    emacs_host_journeys::validate_registry(&cells)?;
    Ok(())
}

/// The advertised CLI seam is a production path, not a convenience: `check`
/// must route to the fail-closed validator, exit 0, emit the schema-tagged
/// summary shape, and stay byte-stable across runs; `explain` must validate
/// registry laws before printing and reject an unregistered subject with a
/// non-zero exit. Without this, routing, exit status, and emitted JSON shape
/// are proven only by hand-run commands.
#[test]
fn journeys_cli_seam_routes_checks_and_fails_closed_on_unknown_subjects() -> Result<()> {
    use std::process::Command;

    let xtask_bin = env!("CARGO_BIN_EXE_xtask");
    let run = |args: &[&str]| -> Result<(bool, String, String)> {
        let output = Command::new(xtask_bin).args(args).output()?;
        Ok((
            output.status.success(),
            String::from_utf8(output.stdout)?,
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    };

    // `check` routes to the validated path and publishes the summary receipt.
    let (ok, first_stdout, stderr) = run(&["integration", "emacs", "journeys", "check"])?;
    ensure!(ok, "journeys check did not exit 0: {stderr}");
    let summary: serde_json::Value = serde_json::from_str(&first_stdout)
        .map_err(|error| anyhow::anyhow!("check stdout is not JSON: {error}: {first_stdout}"))?;
    ensure!(
        summary.get("schema_version").and_then(serde_json::Value::as_str)
            == Some(MANIFEST_SCHEMA_VERSION),
        "check summary does not carry the manifest schema version: {summary}"
    );
    for field in ["cell_count", "core_cell_count", "optional_cell_count", "digest"] {
        ensure!(summary.get(field).is_some(), "check summary omits {field}: {summary}");
    }
    ensure!(
        summary
            .get("cohort_membership")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|membership| membership.len() == DiagnosticCohort::ALL.len()),
        "check summary does not publish every registered cohort: {summary}"
    );

    // Determinism is a claim of the command, so prove it through the command.
    let (ok, second_stdout, stderr) = run(&["integration", "emacs", "journeys", "check"])?;
    ensure!(ok, "second journeys check did not exit 0: {stderr}");
    ensure!(first_stdout == second_stdout, "journeys check stdout is not byte-stable across runs");

    // `explain summary` reaches every registered cell through the validated path.
    let (ok, explained, stderr) = run(&["integration", "emacs", "journeys", "explain", "summary"])?;
    ensure!(ok, "journeys explain summary did not exit 0: {stderr}");
    let explained: serde_json::Value = serde_json::from_str(&explained)?;
    let rows = explained
        .get("cells")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("explain summary published no cells array"))?;
    ensure!(
        rows.len() as u64
            == summary.get("cell_count").and_then(serde_json::Value::as_u64).unwrap_or_default(),
        "explain summary row count disagrees with the check summary cell count"
    );

    // An unregistered subject is a non-zero exit, not an empty success.
    let (ok, stdout, _) =
        run(&["integration", "emacs", "journeys", "explain", "emacs.not_a_registered_cell"])?;
    ensure!(!ok, "journeys explain accepted an unregistered subject: {stdout}");
    Ok(())
}

#[test]
fn optional_cells_require_feature_specific_canonical_truth() -> Result<()> {
    let mut cells = compiled()?;
    for surfaces in [
        vec![HostSurface::DocumentFormattingApplication],
        vec![HostSurface::InlayHintRequestRefresh],
        vec![HostSurface::CodeActionApplicationRefusal, HostSurface::DocumentFormattingApplication],
        vec![HostSurface::CodeActionApplicationRefusal, HostSurface::InlayHintRequestRefresh],
    ] {
        let description = format!("{surfaces:?}");
        let mut substituted = cells.clone();
        let cell = substituted
            .iter_mut()
            .find(|cell| cell.depth == DepthClass::Optional)
            .ok_or_else(|| anyhow::anyhow!("code-action optional cell is absent"))?;
        cell.host_surfaces = surfaces;
        let error =
            emacs_host_journeys::validate_registry(&substituted).err().ok_or_else(|| {
                anyhow::anyhow!("code-action cell accepted unsupported surfaces {description}")
            })?;
        ensure!(
            error.to_string().contains("feature-specific canonical surface"),
            "unexpected rejection for {description}: {error}"
        );
    }
    for unsupported in ["opt_native_formatting", "opt_inlay_hints"] {
        ensure!(
            emacs_host_journeys::lookup(&cells, unsupported).is_err(),
            "unsupported optional class {unsupported} has no canonical truth"
        );
    }
    let cell = cells
        .iter_mut()
        .find(|cell| cell.depth == DepthClass::Optional)
        .ok_or_else(|| anyhow::anyhow!("code-action optional cell is absent"))?;
    cell.expectation_owner.ids = vec!["hover.widget_name".to_string()];
    let error = emacs_host_journeys::validate_registry(&cells)
        .err()
        .ok_or_else(|| anyhow::anyhow!("hover truth certified code-action behavior"))?;
    ensure!(
        error.to_string().contains("feature-specific canonical expectations"),
        "wrong rejection: {error}"
    );
    Ok(())
}

#[test]
fn cells_bind_the_declared_subject_authority() -> Result<()> {
    let cells = compiled()?;
    let mut changed = cells.clone();
    let cell = changed.first_mut().ok_or_else(|| anyhow::anyhow!("empty registry"))?;
    cell.fixture_set_digest = format!("sha256:{}", "a".repeat(64));
    let error = emacs_host_journeys::validate_registry(&changed)
        .err()
        .ok_or_else(|| anyhow::anyhow!("foreign fixture authority was accepted"))?;
    ensure!(
        error.to_string().contains("declared subject-manifest digest"),
        "wrong rejection: {error}"
    );
    ensure!(
        emacs_host_journeys::registry_digest(&cells)?
            != emacs_host_journeys::registry_digest(&changed)?,
        "changed fixture identity did not move registry identity"
    );
    let mut wire =
        serde_json::to_value(cells.first().ok_or_else(|| anyhow::anyhow!("empty registry"))?)?;
    wire.as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("cell is not an object"))?
        .remove("fixture_set_digest");
    ensure!(
        serde_json::from_value::<JourneyCell>(wire).is_err(),
        "missing fixture identity deserialized"
    );
    Ok(())
}

#[test]
fn production_check_binds_content_but_ignores_subject_presentation_order() -> Result<()> {
    use xtask::emacs_host_journeys::validate_compiled_registry_against;
    use xtask::emacs_subject_manifest::SubjectManifest;
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing repository root"))?;
    let mut manifest = SubjectManifest::load(repo)?;
    let fixture = tempfile::tempdir()?;
    let clients = fixture.path().join(".ci/editor-clients");
    std::fs::create_dir_all(&clients)?;
    let path = clients.join("emacs-subjects.v1.json");
    manifest.subjects.reverse();
    std::fs::write(&path, serde_json::to_vec(&manifest)?)?;
    ensure!(
        validate_compiled_registry_against(fixture.path())?
            == emacs_host_journeys::validate_compiled_registry()?,
        "presentation order or JSON formatting changed the declared identity"
    );
    // Preferred library form order carries meaning and must remain bound.
    let mut reordered_forms = manifest.clone();
    let subject = reordered_forms
        .subjects
        .iter_mut()
        .find(|subject| subject.client_library_forms.len() > 1)
        .ok_or_else(|| anyhow::anyhow!("no ordered library preference fixture"))?;
    subject.client_library_forms.reverse();
    std::fs::write(&path, serde_json::to_vec(&reordered_forms)?)?;
    let error = validate_compiled_registry_against(fixture.path())
        .err()
        .ok_or_else(|| anyhow::anyhow!("changed library preference order was accepted"))?;
    ensure!(error.to_string().contains("identity differs"), "wrong rejection: {error}");

    let subject = manifest
        .subjects
        .iter_mut()
        .find(|subject| subject.subject_id == "bundled_eglot_emacs_29_4")
        .ok_or_else(|| anyhow::anyhow!("bundled authority fixture is missing"))?;
    subject.client_source_sha256 = format!("sha256:{}", "a".repeat(64));
    manifest
        .validate()
        .map_err(|error| anyhow::anyhow!("changed fixture must remain valid: {error}"))?;
    xtask::emacs_subject_fan_in::validate_subject_lane_denominator(&manifest)
        .map_err(|error| anyhow::anyhow!("changed fixture must retain denominator: {error}"))?;
    std::fs::write(&path, serde_json::to_vec_pretty(&manifest)?)?;
    let error = validate_compiled_registry_against(fixture.path())
        .err()
        .ok_or_else(|| anyhow::anyhow!("changed declared content was accepted"))?;
    ensure!(error.to_string().contains("identity differs"), "wrong rejection: {error}");
    Ok(())
}
