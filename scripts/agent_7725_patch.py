from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"{path}: expected one replacement, found {count}\n--- old ---\n{old}"
        )
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def insert_before_once(path: Path, marker: str, addition: str) -> None:
    text = path.read_text(encoding="utf-8")
    count = text.count(marker)
    if count != 1:
        raise SystemExit(f"{path}: expected one insertion marker, found {count}: {marker}")
    path.write_text(text.replace(marker, addition + marker, 1), encoding="utf-8")


discovery = Path("crates/perl-core-harness/src/observed_discovery/build.rs")
replace_once(
    discovery,
    '    validate_reference(&input.subject.repository_commit, "repository commit", 40, 64, true)?;\n',
    '    validate_git_object_id(&input.subject.repository_commit, "repository commit")?;\n',
)
insert_before_once(
    discovery,
    "pub(crate) fn validate_reference(\n",
    '''/// Validate one full Git object identity.
///
/// SHA-1 object IDs are exactly 40 hexadecimal characters and SHA-256
/// object IDs are exactly 64. Intermediate widths are neither format.
pub(crate) fn validate_git_object_id(value: &str, label: &str) -> Result<(), String> {
    if !matches!(value.len(), 40 | 64) {
        return Err(format!("{label} must be exactly 40 or 64 characters"));
    }
    if !value.bytes().all(crate::is_lower_case_hex_byte) {
        return Err(format!("{label} must be lower-case hexadecimal"));
    }
    Ok(())
}

''',
)
replace_once(
    discovery,
    "        validate_environment_value, validate_reference, validate_sha256_field, validate_target_id,\n",
    "        validate_environment_value, validate_git_object_id, validate_reference,\n        validate_sha256_field, validate_target_id,\n",
)
insert_before_once(
    discovery,
    "    #[test]\n    fn references_enforce_their_declared_bounds_and_alphabet() {\n",
    '''    /// #7725: repository identities are complete SHA-1 or SHA-256 object
    /// IDs, never arbitrary lower-case hex inside a 40..=64 range.
    #[test]
    fn git_object_ids_require_full_sha1_or_sha256_widths() {
        assert!(validate_git_object_id(&"a".repeat(40), "repository commit").is_ok());
        assert!(validate_git_object_id(&"a".repeat(64), "repository commit").is_ok());
        for width in [39, 41, 63, 65] {
            assert!(rejected_as(
                validate_git_object_id(&"a".repeat(width), "repository commit"),
                "exactly 40 or 64 characters"
            ));
        }
        assert!(rejected_as(
            validate_git_object_id(&"A".repeat(40), "repository commit"),
            "lower-case hexadecimal"
        ));
    }

''',
)

subject = Path("crates/perl-core-harness/src/observed_subject/build.rs")
replace_once(
    subject,
    "    sha256_json, validate_reference, validate_sha256_field, validate_target_id,\n",
    "    sha256_json, validate_git_object_id, validate_reference, validate_sha256_field,\n    validate_target_id,\n",
)
replace_once(
    subject,
    '    validate_reference(&producer.repository_commit, "repository commit", 40, 64, true)?;\n',
    '    validate_git_object_id(&producer.repository_commit, "repository commit")?;\n',
)
subject_text = subject.read_text(encoding="utf-8")
if "mod git_object_identity_tests" in subject_text:
    raise SystemExit(f"{subject}: duplicate Git object identity test module")
subject_text = subject_text.rstrip() + r'''

#[cfg(test)]
mod git_object_identity_tests {
    use super::validate_producer_shape;
    use crate::observed_discovery::model::RunnerArtifactIdentity;
    use crate::observed_subject::model::ProducerSubjectIdentity;
    use crate::runner_model::RunnerKind;

    fn producer_with_commit(repository_commit: String) -> ProducerSubjectIdentity {
        ProducerSubjectIdentity {
            repository_commit,
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree_identity: "prepared-tree-1".to_string(),
            host_perl_identity: "host-perl-5.42.2".to_string(),
            matrix_fingerprint: "a".repeat(64),
            target_id: "component_base".to_string(),
            target_contract_digest: "b".repeat(64),
            variant_target_id: None,
            runner: RunnerKind::Test,
            runner_artifact: RunnerArtifactIdentity {
                canonical_path: "t/TEST".to_string(),
                content_sha256: "c".repeat(64),
            },
            working_directory: "t".to_string(),
            environment_sha256: "d".repeat(64),
        }
    }

    #[test]
    fn producer_subject_accepts_only_full_git_object_id_widths() {
        for width in [40, 64] {
            assert!(validate_producer_shape(&producer_with_commit("a".repeat(width))).is_ok());
        }
        for width in [39, 41, 63, 65] {
            assert!(matches!(
                validate_producer_shape(&producer_with_commit("a".repeat(width))),
                Err(message) if message.contains("exactly 40 or 64 characters")
            ));
        }
        assert!(matches!(
            validate_producer_shape(&producer_with_commit("A".repeat(40))),
            Err(message) if message.contains("lower-case hexadecimal")
        ));
    }
}
'''
subject.write_text(subject_text.lstrip("\n") + "\n", encoding="utf-8")

trace = Path("crates/perl-core-harness/src/invocation_trace/build.rs")
replace_once(
    trace,
    '    validate_reference(&subject.repository_commit, "repository commit", 40, 64, true)?;\n',
    '    crate::observed_discovery::build::validate_git_object_id(\n        &subject.repository_commit,\n        "repository commit",\n    )?;\n',
)
replace_once(
    trace,
    "        RunnerArtifactIdentity, required_limitations, validate_artifact, validate_reference,\n        validate_sha256_field, validate_target_id, validate_trace_session_id,\n",
    "        RunnerArtifactIdentity, required_limitations, validate_artifact, validate_reference,\n        validate_sha256_field, validate_subject, validate_target_id, validate_trace_session_id,\n",
)
replace_once(
    trace,
    "    use crate::invocation_trace::model::SUBJECT_VALIDATIONS_PER_CONSTRUCTION;\n",
    "    use crate::invocation_trace::model::{\n        SUBJECT_VALIDATIONS_PER_CONSTRUCTION, TraceSubjectIdentity,\n    };\n",
)
insert_before_once(
    trace,
    "    #[test]\n    fn sha256_fields_must_be_exactly_64_hex_characters() {\n",
    '''    fn trace_subject_with_commit(repository_commit: String) -> TraceSubjectIdentity {
        TraceSubjectIdentity {
            repository_commit,
            perl_ref: "perl-5.42.2".to_string(),
            prepared_tree_identity: "prepared-tree-1".to_string(),
            host_perl_identity: "host-perl-5.42.2".to_string(),
            matrix_fingerprint: "a".repeat(64),
            target_id: "component_base".to_string(),
            target_contract_digest: "b".repeat(64),
            variant_target_id: None,
            instrumentation_id: None,
            trace_session_id: "trace-session-1".to_string(),
            parent_process_nonce: "process-1".to_string(),
            parent_receipt_digest: "c".repeat(64),
        }
    }

    #[test]
    fn trace_subject_accepts_only_full_git_object_id_widths() {
        for width in [40, 64] {
            assert!(validate_subject(&trace_subject_with_commit("a".repeat(width))).is_ok());
        }
        for width in [39, 41, 63, 65] {
            assert!(rejected_as(
                validate_subject(&trace_subject_with_commit("a".repeat(width))),
                "exactly 40 or 64 characters"
            ));
        }
    }

''',
)

exact_pattern = "^(?:[0-9a-f]{40}|[0-9a-f]{64})$"
old_pattern = "^[0-9a-f]{40,64}$"
patched_schemas = set()
for path in sorted(Path("schemas").glob("perl_core_harness*.schema.json")):
    text = path.read_text(encoding="utf-8")
    if '"repository_commit"' not in text:
        continue
    old = f'"pattern": "{old_pattern}"'
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"{path}: expected one repository_commit range pattern, found {count}"
        )
    path.write_text(
        text.replace(old, f'"pattern": "{exact_pattern}"', 1),
        encoding="utf-8",
    )
    patched_schemas.add(path.name)

required_schemas = {
    "perl_core_harness_upstream_effective_invocation_trace.v1.schema.json",
    "perl_core_harness_upstream_runner_discovery.v1.schema.json",
}
if not required_schemas.issubset(patched_schemas):
    missing = sorted(required_schemas - patched_schemas)
    raise SystemExit(f"missing expected compiler-harness identity schemas: {missing}")

schema_test = Path("crates/perl-core-harness/tests/git_object_identity_contract.rs")
if schema_test.exists():
    raise SystemExit(f"{schema_test}: refusing to overwrite an existing contract test")
schema_test.write_text(
    r'''use serde_json::Value;
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;

const EXACT_GIT_OBJECT_PATTERN: &str = "^(?:[0-9a-f]{40}|[0-9a-f]{64})$";
const REQUIRED_SCHEMA_FILES: [&str; 2] = [
    "perl_core_harness_upstream_effective_invocation_trace.v1.schema.json",
    "perl_core_harness_upstream_runner_discovery.v1.schema.json",
];

fn collect_repository_commit_patterns(
    value: &Value,
    patterns: &mut Vec<String>,
) -> io::Result<()> {
    match value {
        Value::Object(fields) => {
            if let Some(repository_commit) = fields.get("repository_commit") {
                let pattern = repository_commit
                    .get("pattern")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        io::Error::other("repository_commit schema lacks a string pattern")
                    })?;
                patterns.push(pattern.to_string());
            }
            for child in fields.values() {
                collect_repository_commit_patterns(child, patterns)?;
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_repository_commit_patterns(child, patterns)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[test]
fn compiler_harness_schemas_match_the_exact_git_object_width_law(
) -> Result<(), Box<dyn Error>> {
    let schema_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    let mut governed_files = BTreeSet::new();

    for entry in fs::read_dir(&schema_dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("perl_core_harness") || !name.ends_with(".schema.json") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        if !text.contains("\"repository_commit\"") {
            continue;
        }
        let schema: Value = serde_json::from_str(&text)?;
        let mut patterns = Vec::new();
        collect_repository_commit_patterns(&schema, &mut patterns)?;
        if patterns.is_empty() {
            return Err(io::Error::other(format!(
                "{name} declares repository_commit without a pattern"
            ))
            .into());
        }
        if !patterns
            .iter()
            .all(|pattern| pattern == EXACT_GIT_OBJECT_PATTERN)
        {
            return Err(io::Error::other(format!(
                "{name} admits a repository_commit outside the exact 40/64 lower-case law: {patterns:?}"
            ))
            .into());
        }
        governed_files.insert(name.to_string());
    }

    for required in REQUIRED_SCHEMA_FILES {
        if !governed_files.contains(required) {
            return Err(io::Error::other(format!(
                "required compiler-harness identity schema was not governed: {required}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn intermediate_git_object_widths_are_never_canonical() {
    let canonical = |value: &str| {
        matches!(value.len(), 40 | 64)
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    };

    assert!(canonical(&"a".repeat(40)));
    assert!(canonical(&"a".repeat(64)));
    for width in 41..64 {
        assert!(!canonical(&"a".repeat(width)), "width {width} must reject");
    }
    assert!(!canonical(&"A".repeat(40)));
}
''',
    encoding="utf-8",
)

for path in [discovery, subject, trace]:
    text = path.read_text(encoding="utf-8")
    if "repository_commit" in text and "40, 64, true" in text:
        raise SystemExit(f"{path}: residual range-based repository commit validation")
