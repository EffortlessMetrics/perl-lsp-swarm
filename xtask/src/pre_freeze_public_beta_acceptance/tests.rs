use super::first_hour_profile::{
    FirstHourFacts, ProfileObserver, WINDOW_FIRST_15, WINDOW_FIRST_60, index_first_hour,
};
use super::*;

fn fixture() -> (PacketV2, TopologyRequirements) {
    let subject = Subject {
        candidate_id: "synthetic-only".into(),
        repository_sha: "a".repeat(40),
        topology_digest: format!("sha256:{}", "b".repeat(64)),
        artifact_set_id: "synthetic-artifacts".into(),
    };
    let mut artifacts = Vec::new();
    for (platform, digit) in [("linux", 'c'), ("windows", 'd')] {
        for (role, name) in
            [(Role::Perllsp, "server"), (Role::PerlDap, "dap"), (Role::Vsix, "vsix")]
        {
            artifacts.push(Artifact {
                id: format!("{platform}-{name}"),
                role,
                target: format!("{platform}-x64"),
                path: format!("synthetic/{platform}/{name}"),
                sha256: digit.to_string().repeat(64),
                subject: subject.clone(),
                provenance: Provenance::ReleaseShaped,
            });
        }
    }
    let evidence = |id: &str, row: Option<&str>, ids: Vec<String>| EvidenceRef {
        id: id.into(),
        locator: format!("synthetic/{id}"),
        sha256: "e".repeat(64),
        subject: subject.clone(),
        row_id: row.map(str::to_owned),
        artifact_ids: ids,
    };
    let rows: Vec<JourneyRow> = ROWS
        .into_iter()
        .map(|id| {
            let platform = if id.starts_with("linux") { "linux" } else { "windows" };
            let selection = ArtifactSelection {
                perllsp: format!("{platform}-server"),
                perl_dap: format!("{platform}-dap"),
                vsix: format!("{platform}-vsix"),
            };
            JourneyRow {
                id: id.into(),
                platform: platform.into(),
                architecture: "x64".into(),
                host_role: if id.ends_with("minimum_supported") {
                    "minimum_supported"
                } else {
                    "current_stable"
                }
                .into(),
                vscode_version: "1.100.0".into(),
                host_selection: evidence("host", Some(id), selection.ids()),
                clean_profile_id: format!("synthetic-profile-{id}"),
                configuration_identity: "synthetic-config".into(),
                fixtures: vec![Fixture {
                    id: "synthetic-fixture".into(),
                    content_sha256: "f".repeat(64),
                }],
                subject: subject.clone(),
                cells: CELLS
                    .into_iter()
                    .map(|cell| Cell {
                        id: cell.into(),
                        status: Status::Pass,
                        proposition: Proposition::Executed,
                        evidence: vec![evidence(cell, Some(id), selection.ids())],
                        reason: None,
                    })
                    .collect(),
                artifacts: selection,
            }
        })
        .collect();
    let requirements = TopologyRequirements {
        subject: subject.clone(),
        required_preparation_targets: vec!["linux-x64".into(), "windows-x64".into()],
        rows: rows
            .iter()
            .map(|r| RowTargets {
                row_id: r.id.clone(),
                perllsp: vec![format!("{}-x64", r.platform)],
                perl_dap: vec![format!("{}-x64", r.platform)],
                vsix: vec![format!("{}-x64", r.platform)],
            })
            .collect(),
    };
    let all_ids: Vec<String> = artifacts.iter().map(|a| a.id.clone()).collect();
    let packet = PacketV2 {
        check: "pre-freeze-public-beta-acceptance".into(),
        schema_version: SCHEMA.into(),
        phase: "pre_freeze_product".into(),
        subject: subject.clone(),
        source_version: "0.17.0".into(),
        target_release: "0.18.0".into(),
        artifacts,
        first_ten_minutes: Observation {
            status: Status::Pass,
            observed_rows: vec!["linux_x64_current_stable".into()],
            evidence: vec![evidence(
                "observation",
                Some("linux_x64_current_stable"),
                vec!["linux-server".into(), "linux-dap".into(), "linux-vsix".into()],
            )],
            reason: None,
        },
        preparation: ["linux", "windows"]
            .into_iter()
            .map(|p| {
                let ids = vec![format!("{p}-server"), format!("{p}-dap"), format!("{p}-vsix")];
                PreparationRow {
                    target: format!("{p}-x64"),
                    status: Status::Pass,
                    evidence: vec![evidence("prep", None, ids.clone())],
                    artifact_ids: ids,
                    reason: None,
                }
            })
            .collect(),
        mechanisms: MECHANISMS
            .into_iter()
            .map(|issue| Mechanism {
                issue: issue.into(),
                status: Status::Pass,
                evidence: vec![evidence(issue, None, all_ids.clone())],
                reason: None,
            })
            .collect(),
        rows,
        zero_budget_counts: ZeroBudgetCounts::default(),
        product_blockers: vec![],
        expected_beta_limitations: vec!["synthetic only; DAP preview".into()],
        friction_findings: vec![],
        freeze_recommendation: Recommendation::Ready,
        claim_boundary: "synthetic validator controls; no installed execution".into(),
    };
    (packet, requirements)
}
fn row(packet: &mut PacketV2) -> Result<&mut JourneyRow> {
    packet.rows.first_mut().context("fixture row")
}
fn cell(packet: &mut PacketV2) -> Result<&mut Cell> {
    row(packet)?.cells.first_mut().context("fixture cell")
}
fn rejected(packet: &PacketV2, requirements: &TopologyRequirements) -> Result<()> {
    ensure!(validate_v2(packet, requirements).is_err(), "invalid bundle accepted");
    Ok(())
}
#[test]
fn complete_bundle_is_ready_but_never_installed_qualification() -> Result<()> {
    let (packet, requirements) = fixture();
    let parsed = parse_v2(&serde_json::to_vec(&packet)?)?;
    let report = validate_v2(&parsed, &requirements)?;
    ensure!(report.bundle_recommendation == Recommendation::Ready, "complete synthetic bundle");
    ensure!(
        report.installed_qualification == InstalledQualification::NotProven,
        "cannot qualify execution"
    );
    for category in [
        AdapterCategory::TopologyBinding,
        AdapterCategory::ArtifactProvenance,
        AdapterCategory::HostSelection,
        AdapterCategory::InstalledJourney,
        AdapterCategory::FirstTenMinutes,
        AdapterCategory::Preparation,
        AdapterCategory::Mechanism,
    ] {
        ensure!(
            report.evidence_requirements.iter().any(|e| e.category == category),
            "lost obligation {category:?}"
        );
    }
    Ok(())
}
#[test]
fn every_row_and_cell_is_mandatory_and_unique() -> Result<()> {
    let (base, requirements) = fixture();
    for row_index in 0..base.rows.len() {
        let mut p = base.clone();
        p.rows.remove(row_index);
        rejected(&p, &requirements)?;
        let mut p = base.clone();
        let duplicate = p.rows.get(row_index).context("row")?.clone();
        p.rows.push(duplicate);
        rejected(&p, &requirements)?;
        for cell_index in 0..CELLS.len() {
            let mut p = base.clone();
            p.rows.get_mut(row_index).context("row")?.cells.remove(cell_index);
            rejected(&p, &requirements)?;
            let mut p = base.clone();
            let r = p.rows.get_mut(row_index).context("row")?;
            let duplicate = r.cells.get(cell_index).context("cell")?.clone();
            r.cells.push(duplicate);
            rejected(&p, &requirements)?;
            for (status, expected) in [
                (Status::Limited, Recommendation::NotProven),
                (Status::NotProven, Recommendation::NotProven),
                (Status::Blocked, Recommendation::Blocked),
            ] {
                let mut p = base.clone();
                let c = p
                    .rows
                    .get_mut(row_index)
                    .context("row")?
                    .cells
                    .get_mut(cell_index)
                    .context("cell")?;
                c.status = status;
                c.reason = Some("synthetic non-success".into());
                rejected(&p, &requirements)?;
                p.freeze_recommendation = expected;
                ensure!(
                    validate_v2(&p, &requirements)?.bundle_recommendation == expected,
                    "status distinction"
                );
            }
        }
    }
    Ok(())
}
#[test]
fn cross_subject_row_artifact_and_target_joins_fail_closed() -> Result<()> {
    let (base, requirements) = fixture();
    let mut p = base.clone();
    cell(&mut p)?.evidence.first_mut().context("ref")?.subject.candidate_id = "other".into();
    rejected(&p, &requirements)?;
    let mut p = base.clone();
    cell(&mut p)?.evidence.first_mut().context("ref")?.row_id =
        Some("windows_x64_current_stable".into());
    rejected(&p, &requirements)?;
    let mut p = base.clone();
    cell(&mut p)?.evidence.first_mut().context("ref")?.artifact_ids = vec!["windows-server".into()];
    rejected(&p, &requirements)?;
    let mut p = base.clone();
    p.artifacts.first_mut().context("artifact")?.target = "windows-x64".into();
    rejected(&p, &requirements)?;
    let mut p = base.clone();
    p.artifacts.first_mut().context("artifact")?.provenance = Provenance::WorkspaceOutput;
    rejected(&p, &requirements)?;
    let mut p = base.clone();
    row(&mut p)?.subject.topology_digest = format!("sha256:{}", "c".repeat(64));
    rejected(&p, &requirements)?;
    let mut p = base;
    row(&mut p)?.host_role = "current_stable".into();
    rejected(&p, &requirements)
}
#[test]
fn topology_denominator_and_host_selection_cannot_be_omitted() -> Result<()> {
    let (base, requirements) = fixture();
    let mut r = requirements.clone();
    r.rows.pop();
    rejected(&base, &r)?;
    let mut r = requirements.clone();
    r.required_preparation_targets.push("other".into());
    rejected(&base, &r)?;
    let mut r = requirements.clone();
    r.subject.repository_sha = "c".repeat(40);
    rejected(&base, &r)?;
    let mut p = base.clone();
    row(&mut p)?.host_selection.locator.clear();
    rejected(&p, &requirements)?;
    let mut p = base.clone();
    p.preparation.pop();
    rejected(&p, &requirements)?;
    let mut p = base;
    p.first_ten_minutes.observed_rows.push("windows_x64_current_stable".into());
    rejected(&p, &requirements)
}
#[test]
fn every_counter_is_required_and_nonzero_blocks_without_overflow() -> Result<()> {
    let (base, requirements) = fixture();
    let value = serde_json::to_value(&base.zero_budget_counts)?;
    let names: Vec<String> = value.as_object().context("counter object")?.keys().cloned().collect();
    ensure!(names.len() == 12, "twelve trust counters");
    for name in names {
        let mut value = serde_json::to_value(&base)?;
        let object = value
            .get_mut("zero_budget_counts")
            .and_then(serde_json::Value::as_object_mut)
            .context("counters")?;
        object.insert(name.clone(), serde_json::json!(u64::MAX));
        let mut p = parse_v2(&serde_json::to_vec(&value)?)?;
        rejected(&p, &requirements)?;
        p.freeze_recommendation = Recommendation::Blocked;
        ensure!(
            validate_v2(&p, &requirements)?.bundle_recommendation == Recommendation::Blocked,
            "counter {name}"
        );
        value
            .get_mut("zero_budget_counts")
            .and_then(serde_json::Value::as_object_mut)
            .context("counters")?
            .remove(&name);
        ensure!(parse_v2(&serde_json::to_vec(&value)?).is_err(), "missing counter {name}");
    }
    Ok(())
}
#[test]
fn refusals_require_narrow_propositions_and_accepted_claim_obligations() -> Result<()> {
    let (base, requirements) = fixture();
    for id in CELLS {
        for proposition in [Proposition::SafeRefusal, Proposition::ClaimWithdrawn] {
            let mut p = base.clone();
            let c = row(&mut p)?.cells.iter_mut().find(|c| c.id == id).context("cell")?;
            c.proposition = proposition;
            c.reason = Some("synthetic accepted-claim obligation".into());
            let allowed = match proposition {
                Proposition::SafeRefusal => [
                    "safe_rename_or_refusal",
                    "whole_document_formatting",
                    "retained_test_entry",
                    "dap_preview",
                ]
                .contains(&id),
                Proposition::ClaimWithdrawn => id == "retained_test_entry",
                Proposition::Executed => false,
            };
            if allowed {
                let report = validate_v2(&p, &requirements)?;
                ensure!(
                    report
                        .evidence_requirements
                        .iter()
                        .any(|e| e.owner_id == id && e.category == AdapterCategory::AcceptedClaim),
                    "claim adapter omitted"
                );
                row(&mut p)?.cells.iter_mut().find(|c| c.id == id).context("cell")?.reason = None;
                rejected(&p, &requirements)?;
            } else {
                rejected(&p, &requirements)?;
            }
        }
    }
    Ok(())
}
#[test]
fn wire_is_closed_nullable_fields_required_and_v1_not_upgraded() -> Result<()> {
    let (packet, _) = fixture();
    let mut value = serde_json::to_value(&packet)?;
    value.as_object_mut().context("packet")?.insert("unexpected".into(), serde_json::json!(true));
    ensure!(parse_v2(&serde_json::to_vec(&value)?).is_err(), "unknown field");
    let mut value = serde_json::to_value(&packet)?;
    value
        .as_object_mut()
        .context("packet")?
        .insert("schema_version".into(), serde_json::json!("pre_freeze_public_beta_acceptance.v1"));
    ensure!(parse_v2(&serde_json::to_vec(&value)?).is_err(), "legacy admission");
    let mut value = serde_json::to_value(&packet)?;
    value
        .get_mut("first_ten_minutes")
        .and_then(serde_json::Value::as_object_mut)
        .context("observation")?
        .remove("reason");
    ensure!(parse_v2(&serde_json::to_vec(&value)?).is_err(), "nullable reason must be present");
    Ok(())
}
#[test]
fn noncell_statuses_and_blockers_are_not_waived() -> Result<()> {
    let (base, requirements) = fixture();
    for status in [Status::Limited, Status::NotProven, Status::Blocked] {
        let recommendation = if status == Status::Blocked {
            Recommendation::Blocked
        } else {
            Recommendation::NotProven
        };
        for owner in 0..3 {
            let mut p = base.clone();
            match owner {
                0 => {
                    p.first_ten_minutes.status = status;
                    p.first_ten_minutes.reason = Some("synthetic".into());
                }
                1 => {
                    let v = p.mechanisms.first_mut().context("mechanism")?;
                    v.status = status;
                    v.reason = Some("synthetic".into());
                }
                _ => {
                    let v = p.preparation.first_mut().context("preparation")?;
                    v.status = status;
                    v.reason = Some("synthetic".into());
                }
            }
            rejected(&p, &requirements)?;
            p.freeze_recommendation = recommendation;
            ensure!(
                validate_v2(&p, &requirements)?.bundle_recommendation == recommendation,
                "status lost"
            );
        }
    }
    let mut p = base;
    p.product_blockers.push("synthetic product blocker".into());
    rejected(&p, &requirements)?;
    p.freeze_recommendation = Recommendation::Blocked;
    validate_v2(&p, &requirements)?;
    Ok(())
}
#[test]
fn report_order_and_equal_host_versions_preserve_explicit_roles() -> Result<()> {
    let (mut p, requirements) = fixture();
    let original = serde_json::to_value(validate_v2(&p, &requirements)?)?;
    p.rows.reverse();
    p.artifacts.reverse();
    p.mechanisms.reverse();
    p.preparation.reverse();
    for r in &mut p.rows {
        r.cells.reverse();
    }
    ensure!(
        serde_json::to_value(validate_v2(&p, &requirements)?)? == original,
        "report depends on input ordering"
    );
    p.rows.retain(|r| r.id != "linux_x64_minimum_supported");
    rejected(&p, &requirements)
}

#[test]
fn exact_versions_and_shared_universal_vsix_are_explicit() -> Result<()> {
    let (base, requirements) = fixture();
    for version in ["stable", "^1.100.0", "1.100", "1.100.0-insider", ""] {
        let mut p = base.clone();
        row(&mut p)?.vscode_version = version.into();
        rejected(&p, &requirements)?;
    }
    let mut p = base;
    let mut r = requirements;
    p.artifacts.retain(|a| a.id != "windows-vsix");
    p.artifacts.iter_mut().find(|a| a.id == "linux-vsix").context("VSIX")?.target =
        "universal".into();
    for targets in &mut r.rows {
        targets.vsix = vec!["universal".into()];
    }
    for journey in &mut p.rows {
        journey.artifacts.vsix = "linux-vsix".into();
        let ids = journey.artifacts.ids();
        journey.host_selection.artifact_ids = ids.clone();
        for c in &mut journey.cells {
            for e in &mut c.evidence {
                e.artifact_ids = ids.clone();
            }
        }
    }
    // Universal VSIX is not a native preparation target. Keep native pair checks.
    for prep in &mut p.preparation {
        prep.artifact_ids.retain(|id| !id.ends_with("-vsix"));
        for e in &mut prep.evidence {
            e.artifact_ids = prep.artifact_ids.clone();
        }
    }
    let all_ids: Vec<String> = p.artifacts.iter().map(|a| a.id.clone()).collect();
    for m in &mut p.mechanisms {
        for e in &mut m.evidence {
            e.artifact_ids = all_ids.clone();
        }
    }
    ensure!(
        validate_v2(&p, &r)?.bundle_recommendation == Recommendation::Ready,
        "explicit universal VSIX must be allowed"
    );
    r.rows
        .iter_mut()
        .find(|t| t.row_id == "windows_x64_current_stable")
        .context("Windows targets")?
        .vsix = vec!["windows-x64".into()];
    rejected(&p, &r)
}

#[test]
fn preparation_requires_every_same_target_artifact() -> Result<()> {
    let (packet, requirements) = fixture();
    validate_v2(&packet, &requirements)?;
    for target in ["linux-x64", "windows-x64"] {
        for role in ["server", "dap", "vsix"] {
            let mut incomplete = packet.clone();
            let prep = incomplete
                .preparation
                .iter_mut()
                .find(|prep| prep.target == target)
                .context("preparation fixture")?;
            prep.artifact_ids.retain(|id| !id.ends_with(role));
            for evidence in &mut prep.evidence {
                evidence.artifact_ids = prep.artifact_ids.clone();
            }
            rejected(&incomplete, &requirements)?;
        }
    }
    Ok(())
}

fn set_status_without_reason(packet: &mut PacketV2, surface: &str, status: Status) -> Result<()> {
    let (target_status, reason) = match surface {
        "cell" => {
            let value = cell(packet)?;
            (&mut value.status, &mut value.reason)
        }
        "observation" => {
            (&mut packet.first_ten_minutes.status, &mut packet.first_ten_minutes.reason)
        }
        "preparation" => {
            let value = packet.preparation.first_mut().context("preparation")?;
            (&mut value.status, &mut value.reason)
        }
        "mechanism" => {
            let value = packet.mechanisms.first_mut().context("mechanism")?;
            (&mut value.status, &mut value.reason)
        }
        _ => anyhow::bail!("unknown status surface: {surface}"),
    };
    *target_status = status;
    *reason = None;
    Ok(())
}

#[test]
fn blocked_without_reason_is_a_valid_blocked_recommendation_on_every_surface() -> Result<()> {
    let (base, requirements) = fixture();
    validate_v2(&base, &requirements)?;
    for surface in ["cell", "observation", "preparation", "mechanism"] {
        let mut packet = base.clone();
        set_status_without_reason(&mut packet, surface, Status::Blocked)?;
        packet.freeze_recommendation = Recommendation::Blocked;
        let report = validate_v2(&packet, &requirements)
            .with_context(|| format!("blocked without reason rejected on {surface}"))?;
        ensure!(
            report.bundle_recommendation == Recommendation::Blocked,
            "lost blocked status on {surface}"
        );
    }
    Ok(())
}

#[test]
fn limited_and_not_proven_still_require_reasons_on_every_surface() -> Result<()> {
    let (base, requirements) = fixture();
    for status in [Status::Limited, Status::NotProven] {
        for surface in ["cell", "observation", "preparation", "mechanism"] {
            let mut packet = base.clone();
            set_status_without_reason(&mut packet, surface, status)?;
            packet.freeze_recommendation = Recommendation::NotProven;
            rejected(&packet, &requirements)?;
        }
    }
    Ok(())
}

#[test]
fn blocked_refusals_and_withdrawals_still_require_proposition_reasons() -> Result<()> {
    let (base, requirements) = fixture();
    for proposition in [Proposition::SafeRefusal, Proposition::ClaimWithdrawn] {
        let mut packet = base.clone();
        packet.freeze_recommendation = Recommendation::Blocked;
        let value = row(&mut packet)?
            .cells
            .iter_mut()
            .find(|value| value.id == "retained_test_entry")
            .context("retained test cell")?;
        value.status = Status::Blocked;
        value.proposition = proposition;
        value.reason = Some("synthetic accepted-claim obligation".into());
        ensure!(
            validate_v2(&packet, &requirements)?.bundle_recommendation == Recommendation::Blocked,
            "valid blocked proposition"
        );
        row(&mut packet)?
            .cells
            .iter_mut()
            .find(|value| value.id == "retained_test_entry")
            .context("retained test cell")?
            .reason = None;
        rejected(&packet, &requirements)?;
    }
    Ok(())
}

fn wrapper_rejected(
    packet: &PacketV2,
    requirements: &TopologyRequirements,
    facts: &FirstHourFacts,
) -> Result<()> {
    ensure!(index_first_hour(packet, requirements, facts).is_err(), "invalid bundle indexed");
    Ok(())
}

#[test]
fn first_hour_wrapper_retains_complete_canonical_report_and_permanent_ceiling() -> Result<()> {
    let (packet, requirements) = fixture();
    let facts = FirstHourFacts::unobserved();
    let index = index_first_hour(&packet, &requirements, &facts)?;
    let canonical = validate_v2(&packet, &requirements)?;
    ensure!(index.bundle_recommendation == Recommendation::Ready, "wrapper must project Ready");
    ensure!(
        index.installed_qualification == InstalledQualification::NotProven,
        "wrapper must never qualify installed execution"
    );
    ensure!(
        index.canonical_report.bundle_recommendation == canonical.bundle_recommendation,
        "wrapper must retain exact canonical recommendation"
    );
    ensure!(
        serde_json::to_value(&index.canonical_report.evidence_requirements)?
            == serde_json::to_value(&canonical.evidence_requirements)?,
        "wrapper must retain exact canonical obligations"
    );
    for category in [
        AdapterCategory::TopologyBinding,
        AdapterCategory::ArtifactProvenance,
        AdapterCategory::HostSelection,
        AdapterCategory::InstalledJourney,
        AdapterCategory::FirstTenMinutes,
        AdapterCategory::Preparation,
        AdapterCategory::Mechanism,
    ] {
        ensure!(
            index.canonical_report.evidence_requirements.iter().any(|e| e.category == category),
            "lost canonical obligation {category:?}"
        );
    }
    ensure!(index.windows.len() == 6, "two observers times three windows");
    for window in &index.windows {
        ensure!(window.status == Status::NotProven, "unobserved window must stay not_proven");
        ensure!(!window.missing.is_empty(), "missing checklist required");
        for item in &window.missing {
            ensure!(
                !item.contains("missing:") && !item.contains("sha256") && !item.contains("000000"),
                "checklist must not impersonate a receipt: {item}"
            );
        }
        ensure!(
            window.evidence.iter().all(|o| o.owner_kind == "cell"),
            "window evidence is a cell selector only"
        );
    }
    let joined =
        index.windows.iter().flat_map(|w| w.missing.iter().cloned()).collect::<Vec<_>>().join("\n");
    for phrase in [
        "public instruction",
        "conventional AND dynamic",
        "server/DAP/VSIX",
        "clean-profile",
        "root/document/session",
        "first-useful",
        "no concrete",
    ] {
        ensure!(joined.contains(phrase), "missing checklist must name {phrase}");
    }
    ensure!(
        index.canonical_rows == ROWS.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        "wrapper lost canonical rows"
    );
    ensure!(
        index.canonical_cells == CELLS.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        "wrapper lost canonical cells"
    );
    Ok(())
}

#[test]
fn first_hour_wrapper_inherits_canonical_rejections_and_stale_ready() -> Result<()> {
    let (base, requirements) = fixture();
    let facts = FirstHourFacts::unobserved();
    let mut wrong_target = base.clone();
    wrong_target.artifacts.first_mut().context("artifact")?.target = "windows-x64".into();
    wrapper_rejected(&wrong_target, &requirements, &facts)?;
    let mut wrong_subject = base.clone();
    cell(&mut wrong_subject)?.evidence.first_mut().context("ref")?.subject.candidate_id =
        "other".into();
    wrapper_rejected(&wrong_subject, &requirements, &facts)?;
    let mut missing_row = base.clone();
    missing_row.rows.pop();
    wrapper_rejected(&missing_row, &requirements, &facts)?;
    let mut duplicate_cell = base.clone();
    let cloned = row(&mut duplicate_cell)?.cells.first().context("cell")?.clone();
    row(&mut duplicate_cell)?.cells.push(cloned);
    wrapper_rejected(&duplicate_cell, &requirements, &facts)?;
    let mut missing_observation = base.clone();
    missing_observation.first_ten_minutes.observed_rows.push("windows_x64_current_stable".into());
    wrapper_rejected(&missing_observation, &requirements, &facts)?;
    let mut wrong_provenance = base.clone();
    wrong_provenance.artifacts.first_mut().context("artifact")?.provenance =
        Provenance::WorkspaceOutput;
    wrapper_rejected(&wrong_provenance, &requirements, &facts)?;
    let mut wrong_dap = base.clone();
    row(&mut wrong_dap)?.artifacts.perl_dap = "windows-dap".into();
    wrapper_rejected(&wrong_dap, &requirements, &facts)?;
    let mut lost_floor = base.clone();
    lost_floor.rows.retain(|r| r.id != "linux_x64_minimum_supported");
    wrapper_rejected(&lost_floor, &requirements, &facts)?;

    for counter in [
        "wrong_binary_or_artifact",
        "partial_or_checksum_invalid_install",
        "false_exact",
        "stale_exact",
        "unsafe_edit",
        "unexplained_successful_empty",
        "mixed_generation_result",
        "cross_root_leakage",
        "orphaned_candidate_process",
        "silent_product_failure",
        "false_repair_diagnosis",
        "optional_tool_false_requirement",
    ] {
        let mut value = serde_json::to_value(&base)?;
        value
            .get_mut("zero_budget_counts")
            .and_then(serde_json::Value::as_object_mut)
            .context("counters")?
            .insert(counter.into(), serde_json::json!(1));
        let mut stale = parse_v2(&serde_json::to_vec(&value)?)?;
        ensure!(
            stale.freeze_recommendation == Recommendation::Ready,
            "stale fixture must keep declared Ready"
        );
        wrapper_rejected(&stale, &requirements, &facts)?;
        stale.freeze_recommendation = Recommendation::Blocked;
        let index = index_first_hour(&stale, &requirements, &facts)?;
        ensure!(
            index.bundle_recommendation == Recommendation::Blocked
                && index.canonical_report.bundle_recommendation == Recommendation::Blocked,
            "correct Blocked must project as Blocked for {counter}"
        );
        ensure!(
            index.windows.iter().all(|w| w.status == Status::NotProven),
            "blocked bundle must never qualify a window"
        );
    }
    Ok(())
}

#[test]
fn first_hour_wrapper_observer_negative_controls_and_3600s_ceiling() -> Result<()> {
    let (packet, requirements) = fixture();
    for observer in [ProfileObserver::NewHuman, ProfileObserver::FreshAgent] {
        let mut facts = FirstHourFacts::unobserved();
        match observer {
            ProfileObserver::NewHuman => facts.human.guided_expert = true,
            ProfileObserver::FreshAgent => facts.agent.guided_expert = true,
        }
        wrapper_rejected(&packet, &requirements, &facts)?;
    }
    let mut swapped = FirstHourFacts::unobserved();
    swapped.human.observer = ProfileObserver::FreshAgent;
    swapped.agent.observer = ProfileObserver::NewHuman;
    wrapper_rejected(&packet, &requirements, &swapped)?;

    for field in ["checkout", "rescue", "hidden"] {
        let mut facts = FirstHourFacts::unobserved();
        match field {
            "checkout" => facts.agent.checkout_inspection = true,
            "rescue" => facts.agent.private_rescue = true,
            _ => facts.agent.hidden_assistance = true,
        }
        wrapper_rejected(&packet, &requirements, &facts)?;
        facts.agent.recorded_interventions = 1;
        wrapper_rejected(&packet, &requirements, &facts)?;
        facts.agent.intervention_details =
            vec![format!("{field} assistance; unmet public instruction: use public route only")];
        let index = index_first_hour(&packet, &requirements, &facts)?;
        ensure!(
            index.windows.iter().all(|w| w.status == Status::NotProven),
            "{field} rescue must stay not_proven"
        );
        let summary = index
            .observers
            .iter()
            .find(|o| o.observer == ProfileObserver::FreshAgent)
            .context("agent summary")?;
        ensure!(
            summary.intervention_details.join(" ").contains("unmet public instruction"),
            "{field} must expose unmet public instruction"
        );
    }

    for (seconds, synthetic) in [(30, true), (0, true), (3599, false), (3600, false)] {
        let mut facts = FirstHourFacts::unobserved();
        facts.human.observed_60min_seconds = seconds;
        facts.human.synthetic_mechanism_only = synthetic;
        facts.agent.observed_60min_seconds = seconds;
        facts.agent.synthetic_mechanism_only = synthetic;
        let index = index_first_hour(&packet, &requirements, &facts)?;
        for window in index.windows.iter().filter(|w| w.window == WINDOW_FIRST_60) {
            ensure!(
                window.status == Status::NotProven && !window.missing.is_empty(),
                "{seconds}s synthetic={synthetic} filled 60-minute observation"
            );
        }
        ensure!(
            index.installed_qualification == InstalledQualification::NotProven,
            "elapsed seconds must never qualify installed execution"
        );
    }
    Ok(())
}

#[test]
fn first_hour_wrapper_safe_refusal_and_deterministic_order() -> Result<()> {
    let (base, requirements) = fixture();
    let mut packet = base.clone();
    let target = row(&mut packet)?
        .cells
        .iter_mut()
        .find(|c| c.id == "safe_rename_or_refusal")
        .context("refusal cell")?;
    target.proposition = Proposition::SafeRefusal;
    target.reason = Some("synthetic accepted-claim obligation".into());
    let index = index_first_hour(&packet, &requirements, &FirstHourFacts::unobserved())?;
    ensure!(
        index.bundle_recommendation == Recommendation::Ready,
        "safe refusal must not become failure"
    );
    ensure!(
        index
            .canonical_report
            .evidence_requirements
            .iter()
            .any(|e| e.owner_id == "safe_rename_or_refusal"
                && e.category == AdapterCategory::AcceptedClaim),
        "accepted-claim refusal obligation lost"
    );
    let refusal_windows: Vec<_> =
        index.windows.iter().filter(|w| w.window == WINDOW_FIRST_15).collect();
    ensure!(!refusal_windows.is_empty(), "15-minute window missing");
    for window in refusal_windows {
        ensure!(
            window.evidence.iter().any(|o| o.owner_id == "safe_rename_or_refusal"
                && o.category == AdapterCategory::AcceptedClaim),
            "accepted-claim refusal selector lost"
        );
    }

    let original = serde_json::to_value(index_first_hour(
        &base,
        &requirements,
        &FirstHourFacts::unobserved(),
    )?)?;
    let mut reordered = base.clone();
    reordered.rows.reverse();
    reordered.artifacts.reverse();
    reordered.mechanisms.reverse();
    reordered.preparation.reverse();
    for row in &mut reordered.rows {
        row.cells.reverse();
    }
    let reordered_index = serde_json::to_value(index_first_hour(
        &reordered,
        &requirements,
        &FirstHourFacts::unobserved(),
    )?)?;
    ensure!(original == reordered_index, "wrapper output depends on input ordering");
    Ok(())
}

#[test]
fn first_hour_wrapper_mechanism_consistency_not_runtime_acceptance() -> Result<()> {
    let (base, requirements) = fixture();
    let mut packet = base.clone();
    let mechanism = packet.mechanisms.first_mut().context("mechanism")?;
    mechanism.status = Status::Limited;
    mechanism.reason = Some("synthetic mechanism limitation".into());
    wrapper_rejected(&packet, &requirements, &FirstHourFacts::unobserved())?;
    packet.freeze_recommendation = Recommendation::NotProven;
    let index = index_first_hour(&packet, &requirements, &FirstHourFacts::unobserved())?;
    ensure!(
        index.bundle_recommendation == Recommendation::NotProven
            && index.canonical_report.bundle_recommendation == Recommendation::NotProven,
        "mechanism limitation must project as NotProven"
    );
    ensure!(
        index.windows.iter().all(|w| w.status == Status::NotProven)
            && index.installed_qualification == InstalledQualification::NotProven,
        "mechanism consistency is not runtime acceptance"
    );
    Ok(())
}
