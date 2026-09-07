//! Deterministic projections for executor measurement records (#11639).
//!
//! JSON and human views both derive from the one typed
//! [`MeasurementRecord`]; there is no second interpretation layer. Rendering
//! the same record twice is byte-identical: all maps and sets in the model
//! are ordered collections and every list is canonically sorted before it
//! reaches a projection. `NOT_PROVEN` rows stay visible in both views — a
//! render that flatters one model would be a falsifier, not a convenience.

use super::model::{
    CacheAttribution, CellVerdict, LockObservation, MeasurementRecord, NotProvenReason,
    ProcessObservation, TimingVerdict, refusal_text,
};
use serde_json::{json, to_string_pretty};

/// Deterministic JSON projection of one record: the typed record plus its
/// admission verdict, both derived from the same value. The durable,
/// schema-checked artifact shape is the inner record.
pub fn render_json(record: &MeasurementRecord) -> String {
    match to_string_pretty(&json!({
        "admission": record.admit(),
        "record": record,
    })) {
        Ok(text) => text,
        // Serialization of a fully owned, plain-data record cannot fail; the
        // fallback keeps the function total without inventing facts.
        Err(_) => format!(
            "{{\"record\":{{\"protocol_version\":\"{}\"}},\"error\":\"serialization failed\"}}",
            record.protocol_version
        ),
    }
}

/// Deterministic human projection of one record. Every admission-relevant
/// row appears, with `not_proven` spelled out wherever it applies.
pub fn render_human(record: &MeasurementRecord) -> String {
    let mut out = String::new();
    let cell = &record.cell;

    out.push_str(&format!("protocol:           {}\n", record.protocol_version));
    out.push_str(&format!("cell:               {}\n", cell.canonical_id()));
    out.push_str(&format!("workflow_class:     {:?}\n", cell.workflow_class));
    out.push_str(&format!("execution_model:    {:?}\n", cell.execution_model));
    out.push_str(&format!("operation:          {:?}\n", cell.operation));
    out.push_str(&format!("host:               {:?}\n", cell.host));
    out.push_str(&format!(
        "subject:            {}/{} @ {} (package {}, toolchain {}, profile {})\n",
        cell.subject.repository,
        cell.subject.worktree,
        cell.subject.commit,
        cell.subject.package,
        cell.subject.toolchain,
        cell.subject.build_profile,
    ));
    out.push_str(&format!(
        "command:            {} {} ({} env vars)\n",
        record.command.program,
        record.command.args.join(" "),
        record.command.effective_env.len(),
    ));

    out.push_str("timings:            ");
    match record.timings.reconcile() {
        TimingVerdict::Complete => out.push_str(&format!(
            "preparation {}ns + admission_wait {}ns + execution {}ns + reporting {}ns = total {}ns (reconciled)\n",
            record.timings.preparation_nanos.unwrap_or(0),
            record.timings.admission_wait_nanos.unwrap_or(0),
            record.timings.execution_nanos.unwrap_or(0),
            record.timings.reporting_nanos.unwrap_or(0),
            record.timings.total_wall_nanos.unwrap_or(0),
        )),
        TimingVerdict::Incomplete { missing } => {
            out.push_str(&format!("not_proven (missing phases {missing:?})\n"));
        }
        TimingVerdict::Mismatch {
            computed_sum_nanos,
            declared_total_nanos,
        } => out.push_str(&format!(
            "not_proven (phase sum {computed_sum_nanos}ns != declared total {declared_total_nanos}ns beyond tolerance)\n"
        )),
        TimingVerdict::Overflow => {
            out.push_str("not_proven (phase arithmetic overflowed; timing block untrustworthy)\n")
        }
    }

    out.push_str(&format!("lock:               {}\n", lock_text(&record.lock)));
    out.push_str(&format!("disk admission:     {}\n", disk_text(record)));
    out.push_str(&format!("process:            {}\n", process_text(&record.process)));
    out.push_str(&format!("cache:              {}\n", cache_text(record)));
    out.push_str(&format!("work:               {}\n", work_text(record)));
    out.push_str(&format!("executed subject:   {}\n", executed_subject_text(record)));

    match record.admit() {
        CellVerdict::Admitted => out.push_str("verdict:            admitted\n"),
        CellVerdict::AdmittedPartialSubject { proven_dimensions } => out.push_str(&format!(
            "verdict:            admitted (partial executed-subject evidence: {} proven; host \
             lanes complete subject proof)\n",
            proven_dimensions.join(", ")
        )),
        CellVerdict::NotProven { reasons } => {
            out.push_str(&format!("verdict:            not_proven ({} reasons)\n", reasons.len()));
            for reason in &reasons {
                out.push_str(&format!("  - {}\n", reason_text(reason)));
            }
        }
    }

    out.push_str(&format!("raw_digest:         {}\n", record.raw_digest));
    out.push_str(&format!("normalized_digest:  {}\n", record.normalized_digest));
    out
}

fn lock_text(lock: &LockObservation) -> String {
    match lock {
        LockObservation::Held { primitive, wait_nanos } => format!(
            "held ({primitive:?}, wait {wait_nanos}ns — whole-cargo-process scope, not a compile lock)"
        ),
        LockObservation::PolicyDeclaresNone => {
            "policy_declares_none (honest unlocked run for this model)".to_string()
        }
        LockObservation::PrimitiveUnavailable => {
            "not_proven (lock primitive unavailable on this host)".to_string()
        }
        LockObservation::Unobserved => "not_proven (lock instrument unavailable)".to_string(),
    }
}

/// The disk row is derived from `DiskAdmission::covers` against the cell's
/// declared growth paths — the same law admission applies — so a rendered
/// "covered" can never contradict the verdict (#14739 review).
fn disk_text(record: &MeasurementRecord) -> String {
    let cell = &record.cell;
    if !cell.execution_model.declares_growth_paths() {
        return match record.disk_admission {
            None => "not_applicable (model declares no disk growth)".to_string(),
            Some(_) => {
                "not_proven (growth-path-free model carries a disk admission it never declared)"
                    .to_string()
            }
        };
    }
    let Some(admission) = record.disk_admission.as_ref() else {
        return "not_proven (no disk admission recorded)".to_string();
    };
    if let Err(refusal) = admission.covers(&cell.canonical().growth_paths) {
        return format!("not_proven ({})", refusal_text(&refusal));
    }
    let mut parts = Vec::new();
    for measurement in &admission.measurements {
        match measurement.free_bytes {
            Some(free) => parts.push(format!("{}: {free} bytes free", measurement.filesystem.0)),
            None => parts.push(format!(
                "{}: not_proven (free-space measurement failed)",
                measurement.filesystem.0
            )),
        }
    }
    format!("covered ({} filesystems: {})", admission.measurements.len(), parts.join("; "))
}

fn process_text(process: &ProcessObservation) -> String {
    match process {
        ProcessObservation::Observed { descendant_count, terminality } => {
            format!("observed (descendants {descendant_count}, terminality {terminality:?})")
        }
        ProcessObservation::InstrumentUnavailable => {
            "not_proven (process instrument unavailable — never reported as zero)".to_string()
        }
    }
}

fn cache_text(record: &MeasurementRecord) -> String {
    let cache = &record.cache;
    match &cache.attribution {
        CacheAttribution::Attributed => match cache.clean_delta() {
            Some(delta) => format!(
                "attributed (server {}, requests +{}, hits +{}, misses +{}, non_cacheable +{})",
                cache.server_identity.clone().unwrap_or_default(),
                delta.requests,
                delta.hits,
                delta.misses,
                delta.non_cacheable,
            ),
            None => "not_proven (attributed but delta unreconstructable)".to_string(),
        },
        CacheAttribution::Unattributed { reason } => {
            format!("not_proven (unattributed: {reason})")
        }
        CacheAttribution::Unobserved => "unobserved (no cache metrics instrument)".to_string(),
    }
}

fn work_text(record: &MeasurementRecord) -> String {
    let work = &record.work;
    let requires = record.cell.requires_selected_work();
    match (work.expected_selected, work.observed_selected) {
        (Some(expected), Some(observed)) => format!(
            "selected {observed}/{expected}, exit {}{}",
            work.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string()),
            if requires && (observed == 0 || observed != expected) {
                " — not_proven for this proof cell"
            } else {
                ""
            },
        ),
        (expected, observed) => format!(
            "not_proven (expected {expected:?}, observed {observed:?}){}",
            if requires { " — required for this proof cell" } else { "" },
        ),
    }
}

fn executed_subject_text(record: &MeasurementRecord) -> String {
    match &record.executed_subject_commit {
        None => "not_proven (executed candidate never proven)".to_string(),
        Some(executed_commit) => {
            if executed_commit == &record.cell.subject.commit {
                format!("proven (commit {executed_commit} matches declared subject)")
            } else {
                format!(
                    "not_proven (executed candidate {executed_commit} differs from declared commit {})",
                    record.cell.subject.commit
                )
            }
        }
    }
}

fn reason_text(reason: &NotProvenReason) -> String {
    match reason {
        NotProvenReason::TimingIncomplete { missing } => {
            format!("timing incomplete, missing phases {missing:?}")
        }
        NotProvenReason::TimingMismatch { computed_sum_nanos, declared_total_nanos } => format!(
            "timing mismatch: phase sum {computed_sum_nanos}ns vs declared total {declared_total_nanos}ns"
        ),
        NotProvenReason::TimingOverflow => {
            "timing arithmetic overflowed; phase block untrustworthy".to_string()
        }
        NotProvenReason::UnsupportedHost => "host profile declared unsupported".to_string(),
        NotProvenReason::LockNotAdmitted => {
            "lock row not admitted (does not match the declared policy/primitive)".to_string()
        }
        NotProvenReason::ProcessInstrumentUnavailable => {
            "process instrument unavailable".to_string()
        }
        NotProvenReason::ProcessResidual { descendant_count, terminality } => format!(
            "process tree residual ({descendant_count} descendants, terminality {terminality:?})"
        ),
        NotProvenReason::DiskAdmissionMissing => "disk admission missing".to_string(),
        NotProvenReason::DiskAdmissionRefused { detail } => {
            format!("disk admission refused: {detail}")
        }
        NotProvenReason::ExecutedSubjectUnproven => "executed candidate never proven".to_string(),
        NotProvenReason::ExecutedSubjectMismatch { detail } => {
            format!("executed candidate differs from declared subject: {detail}")
        }
        NotProvenReason::CommandExitUnproven => "command exit code never observed".to_string(),
        NotProvenReason::CommandFailed { exit_code } => {
            format!("command exited nonzero ({exit_code})")
        }
        NotProvenReason::SelectedWorkUnproven { expected, observed } => {
            format!("selected work unproven (expected {expected:?}, observed {observed:?})")
        }
        NotProvenReason::CacheEvidenceUnproven { reason } => {
            format!("cache evidence unproven: {reason}")
        }
        NotProvenReason::ProtocolMismatch { record_protocol } => {
            format!("record protocol {record_protocol} does not match the current protocol")
        }
        NotProvenReason::LockTimingInconsistent {
            wait_nanos,
            admission_wait_nanos,
            total_wall_nanos,
        } => format!(
            "lock wait {wait_nanos}ns exceeds the admission window ({admission_wait_nanos:?}) or \
             the total wall time ({total_wall_nanos:?})"
        ),
        NotProvenReason::CapacityUnexecutable { detail } => {
            format!("capacity policy cannot execute: {detail}")
        }
        NotProvenReason::DiskAdmissionSpurious { detail } => {
            format!("spurious disk admission: {detail}")
        }
    }
}
