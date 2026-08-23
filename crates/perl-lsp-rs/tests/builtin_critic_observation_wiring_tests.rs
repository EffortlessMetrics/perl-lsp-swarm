//! Wiring gate for producer-owned built-in critic overlap observations (#11918).
//!
//! The reviewed overlap cohort may only enter critic normalization through
//! named checked constructors declared at the owning lint emitter branch, and
//! both production cut sites must chain those observation candidates into the
//! single native-critic normalization call. If a site reverts to reconstructing
//! identities from finished diagnostics or to a second, separate normalize
//! pass, these gates turn that regression red instead.

use std::fs;
use std::path::Path;

fn production_source(rel_path: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("..").join("perl-lsp-rs-core").join("src").join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("production source {} must be readable: {error}", path.display())
    })
}

fn runtime_source(rel_path: &str) -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = manifest_dir.join("src").join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("production source {} must be readable: {error}", path.display())
    })
}

#[test]
fn every_admitted_cohort_member_has_a_named_emitter_constructor() {
    let security = production_source("providers/diagnostics/lints/security.rs");
    for constructor in [
        "BuiltInCriticObservation::backtick_exec(",
        "BuiltInCriticObservation::system_call(",
        "BuiltInCriticObservation::exec_call(",
        "BuiltInCriticObservation::readpipe_exec(",
    ] {
        assert!(
            security.contains(constructor),
            "the PL601/PL603/PL604/PL606 emitter branches must construct through {constructor}"
        );
    }

    let common_mistakes = production_source("providers/diagnostics/lints/common_mistakes.rs");
    for constructor in [
        "BuiltInCriticObservation::literal_undef_comparison(",
        "BuiltInCriticObservation::potentially_undef_comparison(",
    ] {
        assert!(
            common_mistakes.contains(constructor),
            "the PL404 emitter branch must choose its reviewed shape via {constructor}"
        );
    }
}

#[test]
fn emitter_branches_declare_the_critic_severity_literally() {
    // The declaration must sit at the emission branch as a literal variant,
    // never derived from an LSP-scale diagnostic severity value.
    let security = production_source("providers/diagnostics/lints/security.rs");
    for (constructor, severity) in [
        ("BuiltInCriticObservation::backtick_exec(", "Severity::Harsh"),
        ("BuiltInCriticObservation::system_call(", "Severity::Harsh"),
        ("BuiltInCriticObservation::exec_call(", "Severity::Harsh"),
        ("BuiltInCriticObservation::readpipe_exec(", "Severity::Harsh"),
    ] {
        let position = security
            .find(constructor)
            .unwrap_or_else(|| panic!("{constructor} must exist in the security emitter"));
        let tail = &security[position..position + constructor.len() + 80];
        assert!(
            tail.contains(severity),
            "{constructor} must declare {severity} literally at the branch"
        );
    }

    let common_mistakes = production_source("providers/diagnostics/lints/common_mistakes.rs");
    for constructor in [
        "BuiltInCriticObservation::literal_undef_comparison(",
        "BuiltInCriticObservation::potentially_undef_comparison(",
    ] {
        let position = common_mistakes
            .find(constructor)
            .unwrap_or_else(|| panic!("{constructor} must exist in the common-mistakes emitter"));
        let tail = &common_mistakes[position..position + constructor.len() + 80];
        assert!(
            tail.contains("Severity::Stern"),
            "{constructor} must declare Severity::Stern literally at the branch"
        );
    }
}

#[test]
fn observation_type_exposes_no_general_or_diagnostic_derived_construction() {
    let module = production_source("tooling/perl_critic/built_in_observation.rs");
    assert!(
        !module.contains("DiagnosticSeverity"),
        "the observation type must not know any LSP-scale severity vocabulary"
    );
    assert!(
        !module.contains("fn from_diagnostic"),
        "observations must never be reconstructed from finished diagnostics"
    );
}

#[test]
fn built_in_observation_defines_exactly_the_seven_documented_named_constructors() {
    let module = production_source("tooling/perl_critic/built_in_observation.rs");
    const CONSTRUCTORS: [&str; 7] = [
        "literal_undef_comparison",
        "potentially_undef_comparison",
        "backtick_exec",
        "qx_exec",
        "readpipe_exec",
        "system_call",
        "exec_call",
    ];
    // The fixed non-constructor public surface: accessors plus the one
    // normalization binding. Anything else declared `pub fn` is an undeclared
    // eighth constructor and must turn this gate red until this list is
    // consciously updated.
    const NON_CONSTRUCTOR_PUBLIC_FNS: [&str; 6] =
        ["identity", "severity", "range", "message", "explanation", "into_candidate"];

    for constructor in CONSTRUCTORS {
        assert!(
            module.contains(&format!("pub fn {constructor}(")),
            "the documented constructor {constructor} must exist on BuiltInCriticObservation"
        );
    }

    let mut declared_public_fns: Vec<&str> = module
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            trimmed
                .strip_prefix("pub fn ")
                .or_else(|| trimmed.strip_prefix("pub const fn "))?
                .split('(')
                .next()
        })
        .collect();
    declared_public_fns.sort_unstable();

    let mut expected_public_fns: Vec<&str> = Vec::new();
    expected_public_fns.extend_from_slice(&CONSTRUCTORS);
    expected_public_fns.extend_from_slice(&NON_CONSTRUCTOR_PUBLIC_FNS);
    expected_public_fns.sort_unstable();

    assert_eq!(
        declared_public_fns.len(),
        expected_public_fns.len(),
        "BuiltInCriticObservation must declare exactly {} public functions (7 constructors + 6 accessors); found {declared_public_fns:?}",
        expected_public_fns.len()
    );
    assert_eq!(
        declared_public_fns, expected_public_fns,
        "the reviewed overlap cohort admits exactly the seven documented constructors; \
         a new public constructor or accessor must update this explicit list consciously"
    );
}

#[test]
fn push_cut_chains_builtin_candidates_into_one_native_normalization() {
    let source = runtime_source("runtime/diagnostics.rs");
    assert!(
        source.contains("builtin_critic_overlap_candidates("),
        "the push cut must bind built-in overlap observations to candidates"
    );
    assert!(
        source.contains("surviving_builtin_promotions("),
        "the push cut must retire superseded ordinary twins by surviving contributor keys"
    );
}

#[test]
fn pull_cut_chains_builtin_candidates_into_one_native_normalization() {
    let source = runtime_source("features/diagnostics/pull.rs");
    assert!(
        source.contains("builtin_critic_overlap_observations("),
        "the pull cut must bind built-in overlap observations to candidates"
    );
    assert!(
        source.contains("surviving_builtin_promotions("),
        "the pull cut must retire superseded ordinary twins by surviving contributor keys"
    );
}

#[test]
fn promotion_matching_never_uses_severity_or_message_coincidence() {
    let semantic = production_source("tooling/perl_critic/semantic.rs");
    let function_start = semantic
        .find("pub fn surviving_builtin_promotions")
        .expect("surviving_builtin_promotions must exist in the semantic seam");
    let remainder = &semantic[function_start..];
    let function_end = remainder.find("\n}\n").unwrap_or(remainder.len());
    let body = &remainder[..function_end];
    assert!(
        !body.contains("severity") && !body.contains("message"),
        "promotion keys must come from producer-declared identity and range only"
    );
}
