//! Live reload-mechanism measurement (#10098, R02 measurement seam).
//!
//! The frozen contract (`reload/mechanism.rs`, ADR-0046 §7) states the
//! shared Perl runtime limitations *declaratively* and defers live
//! measurement to this harness: "Live measurement is #10098's harness;
//! this record states what each candidate can and cannot prove today."
//!
//! This module is that measurement seam. It records, as **typed data**,
//! what a controlled real-Perl measurement actually observed about the
//! candidate mechanisms' state limits, and which boundaries the harness
//! **cannot** measure (each becomes a typed [`UnmeasuredBoundary`] —
//! an honest limitation, never a silent guess).
//!
//! Laws inherited from the frozen record and enforced here:
//!
//! - The frozen vocabularies ([`ReloadMechanism`], the mechanism records,
//!   the transaction phases and outcome classes) are **not modified** by
//!   measurement. Measurement adds evidence; it never rewrites authority.
//! - Compile success is never reload success: a measured re-`require`/`do`
//!   that compiles still leaves old symbols and instance state in place,
//!   and the recorded facts must show exactly that.
//! - Class::Refresh stays a measured compatibility subject: when it is not
//!   installed, that is a typed unmeasured boundary, and installing it in
//!   the harness would itself be forbidden authority-by-availability.
//!
//! # What the harness measures
//!
//! One controlled program, one ordinary source-backed module, two
//! generations of its source:
//!
//! ```text
//! v1: value() = "A", old_only() defined, build() captures value() into data
//! require → observe captured value, build instance data
//! rewrite source to v2: value() = "B", old_only removed
//! apply mechanism (delete $INC + require | delete $INC + do $file)
//! observe: new calls, captured values, instance data, removed symbol, %INC
//! ```
//!
//! The observed markers map one-to-one onto [`MeasuredStateFact`] codes;
//! the harness refuses marker text that is not in the closed vocabulary.

use std::process::Command;

use super::mechanism::ReloadMechanism;

// ---------------------------------------------------------------------------
// Measured facts (closed vocabulary, owned by #10098)
// ---------------------------------------------------------------------------

/// One state limit actually observed by the measurement harness.
///
/// Facts are observations, never capability claims: none of these codes
/// can express "migrates instances", "removes old symbols", or "proves
/// package replacement" — the closed vocabulary makes such claims
/// unspelled rather than merely unverified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasuredStateFact {
    /// After the mechanism ran, a fresh `value()` call returns the new
    /// generation's value: re-execution redefines the sub in the symbol
    /// table for *new* resolutions.
    NewSubTakesEffectForNewCalls,
    /// A return value captured *before* the mechanism ran keeps the old
    /// generation's value afterwards.
    CapturedReturnValueKeepsOldCode,
    /// Data stored in an instance built before the mechanism ran is still
    /// readable afterwards, unchanged: instance state never migrates or
    /// recomputes.
    InstanceDataPersists,
    /// A sub absent from the new generation's source remains defined and
    /// callable: re-execution never removes symbols.
    RemovedSubRemainsCallable,
    /// `delete $INC{...}; require ...` repopulates the `%INC` entry.
    IncEntryRefreshedByRequire,
    /// `do $file` re-executes the file *without* any `%INC` bookkeeping:
    /// the deleted entry stays absent.
    IncEntryUnchangedByDo,
}

impl MeasuredStateFact {
    /// All facts in closed order.
    pub const ALL: [MeasuredStateFact; 6] = [
        MeasuredStateFact::NewSubTakesEffectForNewCalls,
        MeasuredStateFact::CapturedReturnValueKeepsOldCode,
        MeasuredStateFact::InstanceDataPersists,
        MeasuredStateFact::RemovedSubRemainsCallable,
        MeasuredStateFact::IncEntryRefreshedByRequire,
        MeasuredStateFact::IncEntryUnchangedByDo,
    ];

    /// Stable closed-vocabulary code emitted by the harness markers.
    pub const fn as_str(self) -> &'static str {
        match self {
            MeasuredStateFact::NewSubTakesEffectForNewCalls => "new_sub_takes_effect_for_new_calls",
            MeasuredStateFact::CapturedReturnValueKeepsOldCode => {
                "captured_return_value_keeps_old_code"
            }
            MeasuredStateFact::InstanceDataPersists => "instance_data_persists",
            MeasuredStateFact::RemovedSubRemainsCallable => "removed_sub_remains_callable",
            MeasuredStateFact::IncEntryRefreshedByRequire => "inc_entry_refreshed_by_require",
            MeasuredStateFact::IncEntryUnchangedByDo => "inc_entry_unchanged_by_do",
        }
    }

    /// Parse the closed vocabulary; unknown marker text is refused.
    pub fn parse(code: &str) -> Option<MeasuredStateFact> {
        MeasuredStateFact::ALL.into_iter().find(|fact| fact.as_str() == code)
    }
}

/// A boundary the harness cannot measure, recorded as a typed limitation.
///
/// An unmeasured boundary is honest output, not a gap to paper over: the
/// frozen record's statements about these boundaries remain declarative
/// until an owned harness (a live debugger session, an installed
/// compatibility subject, an inheritance fixture) can measure them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnmeasuredBoundary {
    /// Class::Refresh is not installed in the measurement environment.
    /// Installing it would grant authority by availability, which the
    /// frozen record forbids, so the compatibility subject stays
    /// unmeasured until a reviewed environment provides it.
    ClassRefreshNotInstalled,
    /// Frames active across a reload require a live debugger session with
    /// a suspended debuggee; the harness runs an ordinary process, so
    /// active-frame continuation is unmeasured here.
    ActiveFrameContinuation,
    /// Method-resolution cache invalidation after `@ISA`/`mro` changes
    /// needs an inheritance fixture with `mro::method_changed_in`
    /// interaction; the initial cohort is a single ordinary module, so
    /// this boundary stays unmeasured.
    MroCacheInvalidation,
    /// Source filters and compile hooks re-entering under a re-execution
    /// are outside the ordinary-module cohort and unmeasured here.
    SourceFilterReentry,
}

impl UnmeasuredBoundary {
    /// All boundaries in closed order.
    pub const ALL: [UnmeasuredBoundary; 4] = [
        UnmeasuredBoundary::ClassRefreshNotInstalled,
        UnmeasuredBoundary::ActiveFrameContinuation,
        UnmeasuredBoundary::MroCacheInvalidation,
        UnmeasuredBoundary::SourceFilterReentry,
    ];

    /// Stable closed-vocabulary code.
    pub const fn as_str(self) -> &'static str {
        match self {
            UnmeasuredBoundary::ClassRefreshNotInstalled => "class_refresh_not_installed",
            UnmeasuredBoundary::ActiveFrameContinuation => "active_frame_continuation",
            UnmeasuredBoundary::MroCacheInvalidation => "mro_cache_invalidation",
            UnmeasuredBoundary::SourceFilterReentry => "source_filter_reentry",
        }
    }

    /// Parse the closed vocabulary; unknown spellings are refused.
    pub fn parse(code: &str) -> Option<UnmeasuredBoundary> {
        UnmeasuredBoundary::ALL.into_iter().find(|boundary| boundary.as_str() == code)
    }
}

// ---------------------------------------------------------------------------
// Measurement record
// ---------------------------------------------------------------------------

/// The measured state limits of one mechanism on one real Perl, or its
/// typed unmeasured boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MechanismMeasurement {
    /// The measured mechanism family (frozen vocabulary, referenced not
    /// modified).
    pub mechanism: ReloadMechanism,
    /// Identity of the Perl that executed the measurement (`$]`), or
    /// `None` for an unexecuted measurement.
    pub perl_identity: Option<String>,
    /// State facts actually observed.
    pub facts: Vec<MeasuredStateFact>,
    /// Boundaries this harness could not measure.
    pub unmeasured: Vec<UnmeasuredBoundary>,
}

impl MechanismMeasurement {
    /// An unexecuted measurement: no facts, typed boundaries only.
    #[must_use]
    pub fn unmeasured(mechanism: ReloadMechanism, boundaries: Vec<UnmeasuredBoundary>) -> Self {
        Self { mechanism, perl_identity: None, facts: Vec::new(), unmeasured: boundaries }
    }

    /// An executed measurement with observed facts.
    #[must_use]
    pub fn measured(
        mechanism: ReloadMechanism,
        perl_identity: String,
        facts: Vec<MeasuredStateFact>,
        unmeasured: Vec<UnmeasuredBoundary>,
    ) -> Self {
        Self { mechanism, perl_identity: Some(perl_identity), facts, unmeasured }
    }
}

/// Why a measurement record disagrees with the frozen contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasurementRecordError {
    /// The frozen shared truths require this observed fact for the
    /// mechanism; its absence means the shared limitation is unproven for
    /// this record.
    MissingObservedFact {
        /// The fact the mechanism's measurement must contain.
        fact: MeasuredStateFact,
    },
    /// The record contains a fact the mechanism cannot produce (its
    /// opposite `%INC` behaviour belongs to the other mechanism).
    FactNotAllowedForMechanism {
        /// The foreign fact.
        fact: MeasuredStateFact,
        /// The mechanism it was recorded against.
        mechanism: ReloadMechanism,
    },
    /// A mechanism without executed facts must declare its typed
    /// boundaries; an empty unexecuted record proves nothing.
    UnmeasuredWithoutBoundaries {
        /// The mechanism with no facts and no boundaries.
        mechanism: ReloadMechanism,
    },
    /// A mechanism that the harness measures directly must carry a Perl
    /// identity; facts without an identity are unattributable.
    FactsWithoutPerlIdentity,
}

impl MeasurementRecordError {
    /// Stable closed-vocabulary codes for `.spec`-style consumption.
    pub const fn code(&self) -> &'static str {
        match self {
            MeasurementRecordError::MissingObservedFact { .. } => "missing_observed_fact",
            MeasurementRecordError::FactNotAllowedForMechanism { .. } => {
                "fact_not_allowed_for_mechanism"
            }
            MeasurementRecordError::UnmeasuredWithoutBoundaries { .. } => {
                "unmeasured_without_boundaries"
            }
            MeasurementRecordError::FactsWithoutPerlIdentity => "facts_without_perl_identity",
        }
    }
}

/// Facts every directly measured mechanism must observe — the executable
/// counterparts of the frozen shared Perl limitations.
const REQUIRED_FOR_MEASURED: [MeasuredStateFact; 4] = [
    MeasuredStateFact::NewSubTakesEffectForNewCalls,
    MeasuredStateFact::CapturedReturnValueKeepsOldCode,
    MeasuredStateFact::InstanceDataPersists,
    MeasuredStateFact::RemovedSubRemainsCallable,
];

/// Verify one measurement record against the frozen contract.
///
/// - A measured [`ReloadMechanism::IncDeletionAndRequire`] must observe
///   every shared-truth fact plus [`MeasuredStateFact::IncEntryRefreshedByRequire`].
/// - A measured [`ReloadMechanism::DoOrRequireHelper`] must observe every
///   shared-truth fact plus [`MeasuredStateFact::IncEntryUnchangedByDo`].
/// - Each mechanism's opposite `%INC` fact is foreign and refused.
/// - The workspace helper and the Class::Refresh subject are not measured
///   by this harness: they must carry typed unmeasured boundaries instead
///   of facts.
pub fn verify_measurement(
    measurement: &MechanismMeasurement,
) -> Result<(), MeasurementRecordError> {
    use MeasuredStateFact as Fact;
    use ReloadMechanism as M;

    if measurement.facts.is_empty() {
        if measurement.unmeasured.is_empty() {
            return Err(MeasurementRecordError::UnmeasuredWithoutBoundaries {
                mechanism: measurement.mechanism,
            });
        }
        return Ok(());
    }
    if measurement.perl_identity.is_none() {
        return Err(MeasurementRecordError::FactsWithoutPerlIdentity);
    }

    let (required_inc_fact, forbidden_inc_fact) = match measurement.mechanism {
        M::IncDeletionAndRequire => (Fact::IncEntryRefreshedByRequire, Fact::IncEntryUnchangedByDo),
        M::DoOrRequireHelper => (Fact::IncEntryUnchangedByDo, Fact::IncEntryRefreshedByRequire),
        // The workspace helper and the compatibility subject are not
        // directly executable by this harness; recording execution facts
        // for them would be fabricated evidence.
        mechanism @ (M::WorkspaceRuntimeHelperObserver | M::ClassRefreshCompatibilitySubject) => {
            return Err(MeasurementRecordError::FactNotAllowedForMechanism {
                fact: measurement.facts[0],
                mechanism,
            });
        }
    };

    let mut required = REQUIRED_FOR_MEASURED.to_vec();
    required.push(required_inc_fact);
    for fact in &required {
        if !measurement.facts.contains(fact) {
            return Err(MeasurementRecordError::MissingObservedFact { fact: *fact });
        }
    }
    if measurement.facts.contains(&forbidden_inc_fact) {
        return Err(MeasurementRecordError::FactNotAllowedForMechanism {
            fact: forbidden_inc_fact,
            mechanism: measurement.mechanism,
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Harness (test-only): runs the controlled real-Perl measurement
// ---------------------------------------------------------------------------

/// The controlled measurement program. One ordinary module, two source
/// generations, one mechanism application, marker output only.
const MEASUREMENT_PROGRAM: &str = r#"use strict;
use warnings;
my ($mode, $dir) = @ARGV;
my $pm = "$dir/DemoMeasured.pm";

sub write_module {
    my ($value, $with_old_only) = @_;
    open my $fh, '>', $pm or die "write: $!";
    print {$fh} "package DemoMeasured;\n";
    print {$fh} "sub build { my \$tag = value(); return { tag => \$tag } }\n";
    print {$fh} "sub value { return '$value' }\n";
    print {$fh} "sub old_only { return 'legacy' }\n" if $with_old_only;
    print {$fh} "1;\n";
    close $fh or die "close: $!";
}

printf "PERL %s\n", $];

write_module('A', 1);
require DemoMeasured;
my $captured_before = DemoMeasured::value();
my $instance = DemoMeasured::build();

write_module('B', 0);
delete $INC{'DemoMeasured.pm'};
if ($mode eq 'inc_deletion_and_require') {
    require DemoMeasured;
    print "FACT inc_entry_refreshed_by_require\n" if exists $INC{'DemoMeasured.pm'};
} elsif ($mode eq 'do_or_require_helper') {
    my $result = do $pm;
    print "FACT inc_entry_unchanged_by_do\n" if !exists $INC{'DemoMeasured.pm'} && $result;
} else {
    die "unknown mode $mode";
}

print "FACT new_sub_takes_effect_for_new_calls\n" if DemoMeasured::value() eq 'B';
print "FACT captured_return_value_keeps_old_code\n" if $captured_before eq 'A';
print "FACT instance_data_persists\n" if $instance->{tag} eq 'A';
print "FACT removed_sub_remains_callable\n" if defined &DemoMeasured::old_only;
"#;

/// Execute the controlled measurement for one directly measurable
/// mechanism on the real `perl` found on `PATH`.
///
/// Returns `None` when no `perl` is available (the harness then skips —
/// an environment without Perl cannot measure Perl, and pretending
/// otherwise would fabricate evidence).
///
/// # Errors
///
/// Fails when the program cannot run, exits nonzero, or emits a marker
/// outside the closed vocabulary.
pub fn measure_mechanism_on_real_perl(
    mechanism: ReloadMechanism,
) -> Option<Result<MechanismMeasurement, String>> {
    let mode = match mechanism {
        ReloadMechanism::IncDeletionAndRequire => "inc_deletion_and_require",
        ReloadMechanism::DoOrRequireHelper => "do_or_require_helper",
        // Only the two directly executable mechanisms are measured live.
        _ => return None,
    };

    // Probe once: no Perl on PATH means no measurement, never a guess.
    if Command::new("perl").arg("-e").arg("1").output().is_err() {
        return None;
    }

    // Scoped scratch directory under the system temp root (the harness
    // owns its lifecycle; no dev-only dependencies are used here). The
    // name is unique per invocation so concurrent measurements cannot
    // delete one another's scratch.
    let scratch = std::env::temp_dir().join(format!(
        "perl-reload-measure-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0),
        mode.replace(['/', '\\'], "_"),
    ));
    std::fs::create_dir_all(&scratch).map_err(|error| format!("create scratch: {error}")).ok()?;
    let result = (|| {
        let program_path = scratch.join("measurement.pl");
        std::fs::write(&program_path, MEASUREMENT_PROGRAM)
            .map_err(|error| format!("write program: {error}"))?;

        let output = Command::new("perl")
            .arg("-I")
            .arg(&scratch)
            .arg(&program_path)
            .arg(mode)
            .arg(&scratch)
            .env("PERL5OPT", "")
            .output()
            .map_err(|error| format!("run perl: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "perl exited {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        parse_measurement_output(mechanism, &stdout)
    })();
    // Cleanup is best-effort; a failed measurement keeps the scratch for
    // diagnosis only until the process exits.
    let _ = std::fs::remove_dir_all(&scratch);
    Some(result)
}

/// Parse the harness stdout into a typed measurement.
///
/// Errors when a marker is outside the closed vocabulary or the observed
/// fact count is not the complete set the program must emit.
fn parse_measurement_output(
    mechanism: ReloadMechanism,
    stdout: &str,
) -> Result<MechanismMeasurement, String> {
    let mut perl_identity = None;
    let mut facts = Vec::new();
    for line in stdout.lines() {
        if let Some(identity) = line.strip_prefix("PERL ") {
            perl_identity = Some(identity.to_string());
        } else if let Some(code) = line.strip_prefix("FACT ") {
            match MeasuredStateFact::parse(code) {
                Some(fact) => facts.push(fact),
                None => return Err(format!("harness emitted unknown fact code {code:?}")),
            }
        } else if !line.trim().is_empty() {
            return Err(format!("harness emitted unrecognized line {line:?}"));
        }
    }

    let identity = perl_identity.ok_or_else(|| "no PERL identity marker".to_string())?;
    if facts.len() != MeasuredStateFact::ALL.len() - 1 {
        return Err(format!(
            "expected {} facts, observed {}: {facts:?}",
            MeasuredStateFact::ALL.len() - 1,
            facts.len()
        ));
    }
    Ok(MechanismMeasurement::measured(
        mechanism,
        identity,
        facts,
        vec![
            UnmeasuredBoundary::ActiveFrameContinuation,
            UnmeasuredBoundary::MroCacheInvalidation,
            UnmeasuredBoundary::SourceFilterReentry,
        ],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured_record(
        mechanism: ReloadMechanism,
        inc_fact: MeasuredStateFact,
    ) -> MechanismMeasurement {
        let mut facts = REQUIRED_FOR_MEASURED.to_vec();
        facts.push(inc_fact);
        MechanismMeasurement::measured(
            mechanism,
            "5.038000".to_string(),
            facts,
            vec![UnmeasuredBoundary::ActiveFrameContinuation],
        )
    }

    #[test]
    fn fact_and_boundary_vocabularies_are_closed() {
        assert_eq!(MeasuredStateFact::ALL.len(), 6);
        for fact in MeasuredStateFact::ALL {
            assert_eq!(MeasuredStateFact::parse(fact.as_str()), Some(fact));
        }
        assert_eq!(MeasuredStateFact::parse("instances_migrate"), None);
        assert_eq!(MeasuredStateFact::parse("package_replaced"), None);

        assert_eq!(UnmeasuredBoundary::ALL.len(), 4);
        for boundary in UnmeasuredBoundary::ALL {
            assert_eq!(UnmeasuredBoundary::parse(boundary.as_str()), Some(boundary));
        }
        assert_eq!(UnmeasuredBoundary::parse("everything_measured"), None);
    }

    #[test]
    fn required_facts_mirror_the_frozen_shared_limitations() {
        // The executable counterparts of PERL_RUNTIME_LIMITATIONS: new code
        // for new calls, old code kept where captured, instance data
        // untouched, removed symbols still callable.
        for fact in &REQUIRED_FOR_MEASURED {
            assert!(
                MeasuredStateFact::ALL.contains(fact),
                "required fact must be in the closed vocabulary"
            );
        }
    }

    #[test]
    fn well_formed_measurements_verify_for_both_direct_mechanisms() {
        let inc = measured_record(
            ReloadMechanism::IncDeletionAndRequire,
            MeasuredStateFact::IncEntryRefreshedByRequire,
        );
        assert!(verify_measurement(&inc).is_ok());

        let do_helper = measured_record(
            ReloadMechanism::DoOrRequireHelper,
            MeasuredStateFact::IncEntryUnchangedByDo,
        );
        assert!(verify_measurement(&do_helper).is_ok());
    }

    #[test]
    fn missing_shared_truth_fact_fails_closed() {
        let mut record = measured_record(
            ReloadMechanism::IncDeletionAndRequire,
            MeasuredStateFact::IncEntryRefreshedByRequire,
        );
        // Drop the removed-symbol observation: the frozen limitation
        // "re-require does not remove old symbols" is then unproven.
        record.facts.retain(|fact| *fact != MeasuredStateFact::RemovedSubRemainsCallable);
        assert_eq!(
            verify_measurement(&record),
            Err(MeasurementRecordError::MissingObservedFact {
                fact: MeasuredStateFact::RemovedSubRemainsCallable,
            })
        );
    }

    #[test]
    fn opposite_inc_fact_is_foreign_and_refused() {
        let mut record = measured_record(
            ReloadMechanism::DoOrRequireHelper,
            MeasuredStateFact::IncEntryUnchangedByDo,
        );
        record.facts.push(MeasuredStateFact::IncEntryRefreshedByRequire);
        assert!(matches!(
            verify_measurement(&record),
            Err(MeasurementRecordError::FactNotAllowedForMechanism { .. })
        ));
    }

    #[test]
    fn unmeasured_mechanisms_carry_typed_boundaries_not_facts() {
        let class_refresh = MechanismMeasurement::unmeasured(
            ReloadMechanism::ClassRefreshCompatibilitySubject,
            vec![
                UnmeasuredBoundary::ClassRefreshNotInstalled,
                UnmeasuredBoundary::ActiveFrameContinuation,
                UnmeasuredBoundary::MroCacheInvalidation,
                UnmeasuredBoundary::SourceFilterReentry,
            ],
        );
        assert!(verify_measurement(&class_refresh).is_ok());

        let helper = MechanismMeasurement::unmeasured(
            ReloadMechanism::WorkspaceRuntimeHelperObserver,
            vec![UnmeasuredBoundary::ActiveFrameContinuation],
        );
        assert!(verify_measurement(&helper).is_ok());

        // Fabricated execution facts for an unmeasured mechanism are
        // refused outright.
        let fabricated = MechanismMeasurement::measured(
            ReloadMechanism::ClassRefreshCompatibilitySubject,
            "5.038000".to_string(),
            REQUIRED_FOR_MEASURED.to_vec(),
            vec![],
        );
        assert!(matches!(
            verify_measurement(&fabricated),
            Err(MeasurementRecordError::FactNotAllowedForMechanism { .. })
        ));
    }

    #[test]
    fn empty_unexecuted_records_are_refused() {
        let empty =
            MechanismMeasurement::unmeasured(ReloadMechanism::IncDeletionAndRequire, vec![]);
        assert!(matches!(
            verify_measurement(&empty),
            Err(MeasurementRecordError::UnmeasuredWithoutBoundaries { .. })
        ));
    }

    #[test]
    fn facts_without_perl_identity_are_unattributable() {
        let mut record = measured_record(
            ReloadMechanism::IncDeletionAndRequire,
            MeasuredStateFact::IncEntryRefreshedByRequire,
        );
        record.perl_identity = None;
        assert_eq!(
            verify_measurement(&record),
            Err(MeasurementRecordError::FactsWithoutPerlIdentity)
        );
    }

    #[test]
    fn frozen_mechanism_vocabulary_is_unchanged_by_measurement() {
        // Measurement references the frozen mechanisms; it must never
        // invent a fifth family or rename one.
        assert_eq!(ReloadMechanism::ALL.len(), 4);
        assert_eq!(
            ReloadMechanism::parse("inc_deletion_and_require"),
            Some(ReloadMechanism::IncDeletionAndRequire)
        );
    }

    // ── Live harness (skipped when no real perl is on PATH) ─────────────

    fn perl_available() -> bool {
        Command::new("perl")
            .arg("-e")
            .arg("1")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn live_perl_measurement_matches_the_frozen_record() {
        if !perl_available() {
            return;
        }
        for (mechanism, inc_fact) in [
            (ReloadMechanism::IncDeletionAndRequire, MeasuredStateFact::IncEntryRefreshedByRequire),
            (ReloadMechanism::DoOrRequireHelper, MeasuredStateFact::IncEntryUnchangedByDo),
        ] {
            let Some(result) = measure_mechanism_on_real_perl(mechanism) else {
                panic!("perl is available but the harness declined to run for {mechanism:?}");
            };
            let measurement = result.unwrap_or_else(|error| {
                panic!("live measurement failed for {mechanism:?}: {error}")
            });
            assert!(measurement.facts.contains(&inc_fact), "{mechanism:?}");
            verify_measurement(&measurement).unwrap_or_else(|error| {
                panic!(
                    "live measurement disagrees with the frozen record for {mechanism:?}: {error:?}"
                )
            });
            assert!(measurement.perl_identity.as_deref().is_some_and(|id| !id.is_empty()));
            // Unmeasured boundaries are typed, never silently absent.
            assert!(measurement.unmeasured.contains(&UnmeasuredBoundary::ActiveFrameContinuation));
        }
    }

    #[test]
    fn live_perl_observes_the_shared_state_limits_or_fails() {
        if !perl_available() {
            return;
        }
        let Some(result) = measure_mechanism_on_real_perl(ReloadMechanism::IncDeletionAndRequire)
        else {
            panic!("perl is available but the harness declined to run");
        };
        let measurement = result.expect("live measurement");
        // Each shared-truth executable counterpart must be present in the
        // observed facts: old symbols stay callable, instance data and
        // captured values keep old code, and new calls take effect.
        for fact in &REQUIRED_FOR_MEASURED {
            assert!(measurement.facts.contains(fact), "live perl did not observe {fact:?}");
        }
    }
}
