use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{error::Error, fs, io, path::Path};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;
const SCHEMA: &str = "schemas/standalone_source_build.v1.schema.json";
const FIXTURES: &str = "fixtures/experience/standalone_source_build";

fn require(ok: bool, message: impl Into<String>) -> TestResult {
    if !ok {
        return Err(io::Error::other(message.into()).into());
    }
    Ok(())
}
fn read(path: &str) -> TestResult<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| io::Error::other("xtask has no repository root"))?;
    Ok(fs::read_to_string(root.join(path))?)
}
fn fixture(name: &str) -> TestResult<Value> {
    Ok(serde_json::from_str(&read(&format!("{FIXTURES}/{name}"))?)?)
}
fn at<'a>(value: &'a Value, pointer: &str) -> TestResult<&'a Value> {
    value.pointer(pointer).ok_or_else(|| io::Error::other(format!("missing {pointer}")).into())
}
fn replace(value: &mut Value, pointer: &str, replacement: Value) -> TestResult {
    let slot = value
        .pointer_mut(pointer)
        .ok_or_else(|| io::Error::other(format!("missing mutation slot {pointer}")))?;
    *slot = replacement;
    Ok(())
}
fn validators() -> TestResult<(jsonschema::Validator, jsonschema::Validator)> {
    let schema: Value = serde_json::from_str(&read(SCHEMA)?)?;
    let evidence = json!({"$schema":"https://json-schema.org/draft/2020-12/schema",
        "$defs":at(&schema,"/$defs")?, "$ref":"#/$defs/evidence"});
    Ok((jsonschema::validator_for(&schema)?, jsonschema::validator_for(&evidence)?))
}

// This is a contract-test oracle, not an installer or authenticated receipt verifier.
// Shape is validated by JSON Schema, never duplicated in the semantic comparison.
#[derive(Debug, PartialEq)]
enum Consistency {
    InternallyConsistent,
    Rejected(&'static str),
}
fn plan_consistency(plan: &Value) -> TestResult<Consistency> {
    let roots = at(plan, "/roots")?
        .as_object()
        .ok_or_else(|| io::Error::other("roots must be an object"))?;
    let mut identities = std::collections::BTreeSet::new();
    for identity in roots.values() {
        require(identity.is_string(), "root identity must be a string")?;
        if !identities.insert(identity.as_str()) {
            return Ok(Consistency::Rejected("root_role_alias"));
        }
    }
    Ok(Consistency::InternallyConsistent)
}
fn compare(plan: &Value, evidence: &Value) -> TestResult<Consistency> {
    let planned = plan_consistency(plan)?;
    if planned != Consistency::InternallyConsistent {
        return Ok(planned);
    }
    for field in [
        "transaction_id",
        "attempt_id",
        "resolved_subject_sha256",
        "target",
        "product_unit",
        "roots",
    ] {
        if at(plan, &format!("/{field}"))? != at(evidence, &format!("/parent/{field}"))? {
            return Ok(Consistency::Rejected("parent_subject_mismatch"));
        }
    }
    let digest = Value::String(plan_digest(plan)?);
    for (expected, observed, reason) in [
        ("/transaction_id", "/transaction_id", "evidence_subject_mismatch"),
        ("/attempt_id", "/attempt_id", "evidence_subject_mismatch"),
        ("/source/resolved_source_sha256", "/resolved_source_sha256", "evidence_subject_mismatch"),
        (
            "/source/resolved_source_sha256",
            "/parent/resolved_source_sha256",
            "parent_subject_mismatch",
        ),
        ("/package/checksum", "/package_checksum", "source_checksum_mismatch"),
        ("/package/manifest_sha256", "/manifest_sha256", "source_checksum_mismatch"),
        ("/source/lockfile_sha256", "/lockfile_sha256", "lockfile_missing_or_mismatch"),
    ] {
        if at(plan, expected)? != at(evidence, observed)? {
            return Ok(Consistency::Rejected(reason));
        }
    }
    if at(evidence, "/plan_sha256")? != &digest {
        return Ok(Consistency::Rejected("evidence_subject_mismatch"));
    }
    for (status, expected, observed, reason) in [
        (
            "/dependency_graph/status",
            "/source/dependency_graph_sha256",
            "/dependency_graph/identity_sha256",
            "dependency_graph_not_proven",
        ),
        (
            "/toolchain/status",
            "/toolchain",
            "/toolchain/identity",
            "toolchain_unavailable_or_mismatch",
        ),
    ] {
        if at(evidence, status)? != "matched"
            || at(plan, expected)?.is_null()
            || at(plan, expected)? != at(evidence, observed)?
        {
            return Ok(Consistency::Rejected(reason));
        }
    }
    if at(evidence, "/configuration_enforcement/status")? == "missing"
        || at(evidence, "/configuration_enforcement/identity_sha256")?.is_null()
    {
        return Ok(Consistency::Rejected("not_proven"));
    }
    let configuration_digest =
        digest_value("standalone_source_build.configuration.v1\n", at(plan, "/configuration")?)?;
    if at(evidence, "/configuration_enforcement/status")? != "matched"
        || at(evidence, "/configuration_enforcement/identity_sha256")?
            != &Value::String(configuration_digest)
    {
        return Ok(Consistency::Rejected("ambient_configuration_rejected"));
    }
    if at(evidence, "/instrument/status")? != "matched"
        || at(evidence, "/instrument/identity_sha256")?.is_null()
    {
        return Ok(Consistency::Rejected("instrument_failure"));
    }
    if at(evidence, "/consent/state")? != "recorded"
        || at(evidence, "/consent/plan_sha256")? != &digest
    {
        return Ok(Consistency::Rejected("consent_required"));
    }
    for (expected, observed) in [
        ("/transaction_id", "/consent/transaction_id"),
        ("/attempt_id", "/consent/attempt_id"),
        ("/execution/disclosure_sha256", "/consent/disclosure_sha256"),
        ("/execution/consent_policy_sha256", "/consent/consent_policy_sha256"),
    ] {
        if at(plan, expected)? != at(evidence, observed)? {
            return Ok(Consistency::Rejected("consent_required"));
        }
    }
    if at(evidence, "/materialization_network")? != "canonical_registry_only"
        || at(evidence, "/build_network")? != "none"
    {
        return Ok(Consistency::Rejected("network_policy_contradiction"));
    }
    Ok(Consistency::InternallyConsistent)
}

// Deliberately bounded canonical JSON: ASCII strings/keys, unsigned integers,
// booleans/null; sorted keys, preserved array order, no whitespace or self digest.
fn canonical(value: &Value) -> TestResult<String> {
    Ok(match value {
        Value::Object(fields) => {
            let mut keys: Vec<_> = fields.keys().collect();
            keys.sort();
            let mut pairs = Vec::new();
            for key in keys {
                require(key.is_ascii(), "non-ASCII key outside digest subset")?;
                let child =
                    fields.get(key).ok_or_else(|| io::Error::other("missing sorted key"))?;
                pairs.push(format!("{}:{}", serde_json::to_string(key)?, canonical(child)?));
            }
            format!("{{{}}}", pairs.join(","))
        }
        Value::Array(items) => {
            format!("[{}]", items.iter().map(canonical).collect::<TestResult<Vec<_>>>()?.join(","))
        }
        Value::String(text) => {
            require(text.is_ascii(), "non-ASCII string outside digest subset")?;
            serde_json::to_string(text)?
        }
        Value::Number(number) => {
            require(number.is_u64(), "non-integer outside digest subset")?;
            number.to_string()
        }
        _ => serde_json::to_string(value)?,
    })
}
fn plan_digest(plan: &Value) -> TestResult<String> {
    digest_value("standalone_source_build.v1\n", plan)
}
fn digest_value(domain: &str, value: &Value) -> TestResult<String> {
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update(canonical(value)?.as_bytes());
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

#[test]
fn fixtures_distinguish_schema_rejection_from_semantic_mismatch() -> TestResult {
    let (schema, evidence_schema) = validators()?;
    let exact = fixture("01_exact_plan.json")?;
    let plan = at(&exact, "/plan")?;
    let evidence = at(&exact, "/evidence")?;
    require(schema.is_valid(plan), "positive plan schema")?;
    require(evidence_schema.is_valid(evidence), "positive evidence schema")?;
    require(compare(plan, evidence)? == Consistency::InternallyConsistent, "positive consistency")?;
    require(
        at(&exact, "/expectation/result")? == "plan_valid",
        "planning result is only plan_valid",
    )?;
    for (name, pointer, reason) in [
        ("02_latest_selector_rejected.json", "/plan/package/version", "schema_rejected"),
        ("03_workspace_source_rejected.json", "/plan/source/kind", "schema_rejected"),
        ("04_checksum_mismatch.json", "/evidence/package_checksum", "source_checksum_mismatch"),
        ("05_lockfile_missing.json", "/evidence/lockfile_sha256", "lockfile_missing_or_mismatch"),
        ("06_source_as_pair_rejected.json", "/plan/product_unit", "schema_rejected"),
        (
            "07_implicit_fallback_rejected.json",
            "/plan/execution/archive_fallback",
            "schema_rejected",
        ),
        ("08_no_consent_rejected.json", "/evidence/consent/state", "consent_required"),
        (
            "09_ambient_configuration_rejected.json",
            "/plan/configuration/credential_helpers",
            "schema_rejected",
        ),
        ("10_directory_is_not_sandbox.json", "/plan/execution/isolation_claim", "schema_rejected"),
        ("11_private_receipt_rejected.json", "/plan/receipt_path", "schema_rejected"),
    ] {
        let vector = fixture(name)?;
        require(at(&vector, "/base")? == "01_exact_plan.json", "single canonical base")?;
        require(at(&vector, "/mutation/pointer")? == pointer, "fixture mutation identity")?;
        let mut packet = exact.clone();
        let (parent, key) =
            pointer.rsplit_once('/').ok_or_else(|| io::Error::other("mutation pointer"))?;
        packet
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| io::Error::other("mutation parent"))?
            .insert(key.to_string(), at(&vector, "/mutation/value")?.clone());
        replace(&mut packet, "/expectation", at(&vector, "/expectation")?.clone())?;
        require(at(&packet, "/expectation/result")? == reason, format!("{name} label"))?;
        let candidate_plan = at(&packet, "/plan")?;
        let candidate_evidence = at(&packet, "/evidence")?;
        require(evidence_schema.is_valid(candidate_evidence), format!("{name} evidence shape"))?;
        if reason == "schema_rejected" {
            require(!schema.is_valid(candidate_plan), format!("{name} must actually fail schema"))?;
        } else {
            require(schema.is_valid(candidate_plan), format!("{name} structurally valid plan"))?;
            require(
                compare(candidate_plan, candidate_evidence)? == Consistency::Rejected(reason),
                format!("{name} semantic reason"),
            )?;
        }
        if let Some(original) = exact.pointer(pointer) {
            replace(&mut packet, pointer, original.clone())?;
        } else {
            let (parent, key) =
                pointer.rsplit_once('/').ok_or_else(|| io::Error::other("mutation pointer"))?;
            packet
                .pointer_mut(parent)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| io::Error::other("mutation parent"))?
                .remove(key);
        }
        require(
            at(&packet, "/plan")? == plan && at(&packet, "/evidence")? == evidence,
            format!("{name} single-field repaired control"),
        )?;
        require(
            schema.is_valid(at(&packet, "/plan")?)
                && compare(at(&packet, "/plan")?, at(&packet, "/evidence")?)?
                    == Consistency::InternallyConsistent,
            format!("{name} repair passes"),
        )?;
    }
    Ok(())
}

#[test]
fn structural_mutations_cannot_claim_supported_inputs() -> TestResult {
    let (schema, _) = validators()?;
    let packet = fixture("01_exact_plan.json")?;
    let plan = at(&packet, "/plan")?;
    for (pointer, value) in [
        ("/registry/index", json!("https://mirror.invalid/index")),
        ("/registry/identity", json!("mirror")),
        ("/registry/protocol", json!("crates.io_git")),
        ("/package/version", json!("^0.18.0")),
        ("/package/version", json!("0.18.0\n")),
        ("/package/version", json!("1.0.0-01")),
        ("/package/checksum", json!(format!("{}\n", "a".repeat(64)))),
        ("/transaction_id", json!("tx-1\n")),
        ("/features/enabled", json!(["feature\n"])),
        ("/target/triple", json!("x86_64-unknown-linux-gnu\n")),
        ("/toolchain/rustc", json!("stable")),
        ("/target/libc", json!("musl")),
        ("/execution/authority", json!("elevated_system")),
        ("/network/build", json!("allowed")),
        ("/network/materialization", json!("allowed")),
        ("/configuration/cargo_config", json!("ambient")),
        ("/configuration/rustc_wrapper", json!("ambient")),
        ("/configuration/rustflags", json!("ambient")),
        ("/configuration/linker", json!("ambient")),
        ("/configuration/runner", json!("ambient")),
        ("/configuration/proxy_ca", json!("custom")),
        ("/configuration/inherited_environment_allowlist", json!(["SECRET_TOKEN"])),
    ] {
        let mut changed = plan.clone();
        replace(&mut changed, pointer, value)?;
        require(!schema.is_valid(&changed), format!("must structurally reject {pointer}"))?;
        replace(&mut changed, pointer, at(plan, pointer)?.clone())?;
        require(schema.is_valid(&changed), format!("repair {pointer}"))?;
    }
    for version in ["1.0.0-0", "1.0.0+01", "1.0.0-rc.1"] {
        let mut valid = plan.clone();
        replace(&mut valid, "/package/version", json!(version))?;
        require(schema.is_valid(&valid), format!("valid exact SemVer {version}"))?;
    }
    let mut git = plan.clone();
    replace(&mut git, "/registry/protocol", json!("crates.io_git"))?;
    replace(&mut git, "/registry/index", json!("https://github.com/rust-lang/crates.io-index"))?;
    require(schema.is_valid(&git), "canonical git counterpart")?;
    Ok(())
}

#[test]
fn stale_missing_cross_subject_and_failed_observations_are_not_success() -> TestResult {
    let (schema, evidence_schema) = validators()?;
    let packet = fixture("01_exact_plan.json")?;
    let plan = at(&packet, "/plan")?;
    let evidence = at(&packet, "/evidence")?;
    for (pointer, value, reason) in [
        ("/lockfile_sha256", json!("0".repeat(64)), "lockfile_missing_or_mismatch"),
        ("/resolved_source_sha256", json!("0".repeat(64)), "evidence_subject_mismatch"),
        ("/attempt_id", json!("stale-attempt"), "evidence_subject_mismatch"),
        ("/plan_sha256", json!("0".repeat(64)), "evidence_subject_mismatch"),
        ("/parent/resolved_subject_sha256", json!("0".repeat(64)), "parent_subject_mismatch"),
        ("/parent/roots/destination", json!("different-destination"), "parent_subject_mismatch"),
        ("/consent/plan_sha256", json!("0".repeat(64)), "consent_required"),
        ("/consent/attempt_id", json!("stale-attempt"), "consent_required"),
        ("/consent/disclosure_sha256", json!("0".repeat(64)), "consent_required"),
        ("/dependency_graph/identity_sha256", Value::Null, "dependency_graph_not_proven"),
        ("/dependency_graph/identity_sha256", json!("0".repeat(64)), "dependency_graph_not_proven"),
        ("/dependency_graph/status", json!("failed"), "dependency_graph_not_proven"),
        ("/toolchain/identity", Value::Null, "toolchain_unavailable_or_mismatch"),
        ("/toolchain/status", json!("missing"), "toolchain_unavailable_or_mismatch"),
        (
            "/toolchain/identity/cargo_sha256",
            json!("0".repeat(64)),
            "toolchain_unavailable_or_mismatch",
        ),
        ("/instrument/identity_sha256", Value::Null, "instrument_failure"),
        ("/instrument/status", json!("failed"), "instrument_failure"),
        ("/configuration_enforcement/status", json!("missing"), "not_proven"),
        ("/configuration_enforcement/identity_sha256", Value::Null, "not_proven"),
        (
            "/configuration_enforcement/identity_sha256",
            json!("0".repeat(64)),
            "ambient_configuration_rejected",
        ),
        ("/build_network", json!("accessed"), "network_policy_contradiction"),
        ("/materialization_network", json!("other"), "network_policy_contradiction"),
    ] {
        let mut changed = evidence.clone();
        replace(&mut changed, pointer, value)?;
        require(
            schema.is_valid(plan) && evidence_schema.is_valid(&changed),
            format!("semantic control shape {pointer}"),
        )?;
        require(
            compare(plan, &changed)? == Consistency::Rejected(reason),
            format!("semantic control {pointer}"),
        )?;
        replace(&mut changed, pointer, at(evidence, pointer)?.clone())?;
        require(
            compare(plan, &changed)? == Consistency::InternallyConsistent,
            format!("repair {pointer}"),
        )?;
    }
    Ok(())
}

#[test]
fn canonical_digest_is_order_invariant_and_binds_immutable_inputs() -> TestResult {
    let (schema, _) = validators()?;
    let packet = fixture("01_exact_plan.json")?;
    let plan = at(&packet, "/plan")?;
    let digest = plan_digest(plan)?;
    require(
        digest == "ba0f37b0763ac81e7adc286a4d5325d517297502b70c32226bf7847ac458f5c6",
        "independent Python hashlib sorted-JSON vector",
    )?;
    require(
        plan_digest(&json!({"b":1,"a":2}))?
            == plan_digest(&serde_json::from_str::<Value>(r#"{"a":2,"b":1}"#)?)?,
        "key order invariant",
    )?;
    require(
        canonical(&json!(1.5)).is_err() && canonical(&json!("é")).is_err(),
        "unsupported digest subset rejected",
    )?;
    for (pointer, value) in [
        ("/package/version", json!("0.19.0")),
        ("/package/checksum", json!("0".repeat(64))),
        ("/package/manifest_sha256", json!("0".repeat(64))),
        ("/source/lockfile_sha256", json!("0".repeat(64))),
        ("/target/triple", json!("aarch64-unknown-linux-gnu")),
        ("/toolchain/rustc_sha256", json!("0".repeat(64))),
        ("/features/default_features", json!(false)),
        ("/features/enabled", json!(["feature_a"])),
        ("/profile", json!("dev")),
        ("/configuration/toolchain_lookup_sha256", json!("0".repeat(64))),
        ("/execution/consent_policy_sha256", json!("0".repeat(64))),
        ("/execution/disclosure_sha256", json!("0".repeat(64))),
        ("/roots/staging", json!("different-staging")),
        ("/resource_policy/deadline_seconds", json!(60)),
    ] {
        let mut changed = plan.clone();
        replace(&mut changed, pointer, value)?;
        require(plan_digest(&changed)? != digest, format!("digest sensitivity {pointer}"))?;
        if pointer != "/profile" {
            require(schema.is_valid(&changed), format!("supported digest mutation {pointer}"))?;
        }
    }
    let policy = read("policy/standalone-source-build.v1.toml")?;
    for pin in [
        "no_executor_consumes_this_contract",
        "internal_consistency_only",
        "not_proven_by_directory_isolation",
        "#10243",
        "attempt_private_only_preserve_destination",
    ] {
        require(policy.contains(pin), format!("policy boundary {pin}"))?;
    }
    Ok(())
}

#[test]
fn planning_never_manufactures_stage_or_authority_evidence() -> TestResult {
    let (schema, evidence_schema) = validators()?;
    let packet = fixture("01_exact_plan.json")?;
    let plan = at(&packet, "/plan")?;
    require(
        schema.is_valid(plan) && plan_consistency(plan)? == Consistency::InternallyConsistent,
        "plan alone is valid without consent or stage observations",
    )?;
    require(
        !evidence_schema.is_valid(&Value::Null),
        "missing observations cannot become supplied evidence",
    )?;
    let mut aliased = plan.clone();
    replace(&mut aliased, "/roots/target", at(plan, "/roots/build")?.clone())?;
    require(schema.is_valid(&aliased), "root alias is a cross-field semantic constraint")?;
    require(
        plan_consistency(&aliased)? == Consistency::Rejected("root_role_alias"),
        "private root roles must not alias",
    )?;
    replace(&mut aliased, "/roots/target", at(plan, "/roots/target")?.clone())?;
    require(plan_consistency(&aliased)? == Consistency::InternallyConsistent, "root alias repair")?;
    let mut no_graph = plan.clone();
    replace(&mut no_graph, "/source/dependency_graph_sha256", Value::Null)?;
    require(schema.is_valid(&no_graph), "unknown graph does not prevent planning")?;
    let mut evidence = at(&packet, "/evidence")?.clone();
    let digest = Value::String(plan_digest(&no_graph)?);
    replace(&mut evidence, "/plan_sha256", digest.clone())?;
    replace(&mut evidence, "/consent/plan_sha256", digest)?;
    replace(&mut evidence, "/dependency_graph/identity_sha256", Value::Null)?;
    require(evidence_schema.is_valid(&evidence), "missing graph control shape")?;
    require(
        compare(&no_graph, &evidence)? == Consistency::Rejected("dependency_graph_not_proven"),
        "equal null graph identities are not proof",
    )?;
    Ok(())
}
