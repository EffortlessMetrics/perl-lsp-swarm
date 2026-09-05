//! Closed-world proof for the target and runner evidence contracts (#7729).
//!
//! Every exact versioned object in the target-selection, matrix, topology-drift,
//! runner-plan, and runner-parity envelopes must reject a field the active
//! schema does not declare — at every nesting level, in serde *and* in the
//! registered JSON Schema — while the declared extension maps stay open.
//!
//! The annotations that implement this already exist. This suite exists because
//! an annotation is not evidence: `#[serde(deny_unknown_fields)]` can be dropped
//! in a refactor and `additionalProperties: false` can be edited out of a schema
//! without any current test noticing. The load-bearing fixture is therefore a
//! canonical payload plus exactly one unknown field, asserted rejected by both
//! decoders.
//!
//! Coverage is derived by walking each canonical payload against its registered
//! schema rather than from a hand-listed set of pointers, so an object added to
//! a contract later is proven — or reported as unclassified — without anyone
//! remembering to extend a list here.

use crate::build::build_runner_plan;
use crate::compare::compare_runner_plans_against;
use crate::io::{read_drift, read_matrix};
use crate::model::{
    TargetMatrixIndex, TargetMatrixPart, TargetTopologyDrift, UpstreamTargetMatrix,
};
use crate::runner_model::{RunnerKind, RunnerParityReport, RunnerPlan, RunnerScheduling};
use crate::schema_check;
use color_eyre::eyre::{Result, eyre};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const MATRIX_FIXTURE: &str = ".ci/perl-core-harness/upstream-targets-5.42.2.v1";
const DRIFT_FIXTURE: &str = ".ci/perl-core-harness/upstream-targets-blead-drift.v1.json";
const UNKNOWN_KEY: &str = "unexpected_field";

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn read_json(relative: &str) -> Result<Value> {
    let bytes = std::fs::read(repo_file(relative))
        .map_err(|error| eyre!("cannot read {relative}: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| eyre!("cannot parse {relative}: {error}"))
}

/// Loads a registered schema and inlines any cross-file `$ref`.
///
/// The part schema points at `perl_core_harness_target_matrix.v1.schema.json#/$defs/matrix_entry`,
/// and that entry in turn uses document-local refs. Merging the referenced
/// document's `$defs` into the host and rewriting the ref to a local pointer
/// keeps one self-contained document, so the proof checks the same definitions
/// production reads rather than a copy.
fn schema_of(name: &str) -> Result<Value> {
    let mut doc = read_json(&format!("schemas/{name}.schema.json"))?;
    let mut imported: Vec<(String, Value)> = Vec::new();
    collect_cross_file_refs(&mut doc, &mut imported)?;
    if !imported.is_empty() {
        let defs = doc
            .as_object_mut()
            .ok_or_else(|| eyre!("schema {name} is not an object"))?
            .entry("$defs")
            .or_insert_with(|| json!({}));
        let defs =
            defs.as_object_mut().ok_or_else(|| eyre!("schema {name} has a non-object $defs"))?;
        for (key, value) in imported {
            defs.entry(key).or_insert(value);
        }
    }
    Ok(doc)
}

/// Rewrites `<file>.schema.json#/<pointer>` refs to `#/<pointer>` and collects
/// the referenced document's definitions for inlining.
fn collect_cross_file_refs(node: &mut Value, imported: &mut Vec<(String, Value)>) -> Result<()> {
    match node {
        Value::Object(fields) => {
            if let Some(Value::String(reference)) = fields.get("$ref")
                && let Some((file, pointer)) = reference.split_once(".schema.json#")
            {
                let file = format!("{file}.schema.json");
                let local = pointer.to_string();
                let source = read_json(&format!("schemas/{file}"))?;
                for (key, value) in
                    source.get("$defs").and_then(Value::as_object).into_iter().flatten()
                {
                    imported.push((key.clone(), value.clone()));
                }
                fields.insert("$ref".to_string(), json!(format!("#{local}")));
            }
            for value in fields.values_mut() {
                collect_cross_file_refs(value, imported)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_cross_file_refs(item, imported)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| eyre!(error))
}

/// One registered evidence envelope: a canonical payload, the schema it is
/// registered against, and the exact Rust decoder that owns it.
struct Envelope {
    label: &'static str,
    schema: Value,
    payload: Value,
    /// Whether the owning Rust type accepts this JSON. The closed-world claim is
    /// about serde and the schema agreeing, so both sides are always asked.
    decodes: fn(&Value) -> bool,
}

fn decodes<T: serde::de::DeserializeOwned>(value: &Value) -> bool {
    serde_json::from_value::<T>(value.clone()).is_ok()
}

/// The canonical payloads. Real pinned fixtures are used wherever one exists so
/// the proof runs against the bytes production actually reads.
fn envelopes() -> Result<Vec<Envelope>> {
    let matrix = read_matrix(&repo_file(MATRIX_FIXTURE))?;
    let drift = read_drift(&repo_file(DRIFT_FIXTURE))?;

    // A plan and a parity receipt have no checked-in fixture; build them through
    // the production constructors so the payload is one a producer can emit.
    let test_raw = b"t/base/cond.t\nt/base/if.t\n";
    let harness_raw = b"t/base/if.t\nt/base/cond.t\n";
    let test_plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        test_raw,
        RunnerScheduling::default(),
    )
    .map_err(|error| eyre!(error))?;
    let harness_plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Harness,
        harness_raw,
        RunnerScheduling {
            jobs: Some(2),
            asap: false,
            state_ordering: true,
            properties: [("stress".to_string(), "off".to_string())].into_iter().collect(),
        },
    )
    .map_err(|error| eyre!(error))?;
    let parity =
        compare_runner_plans_against(&matrix, &test_plan, test_raw, &harness_plan, harness_raw)
            .map_err(|error| eyre!(error))?;

    Ok(vec![
        Envelope {
            label: "target_matrix.v1",
            schema: schema_of("perl_core_harness_target_matrix.v1")?,
            payload: to_value(&matrix)?,
            decodes: decodes::<UpstreamTargetMatrix>,
        },
        Envelope {
            label: "target_matrix_index.v1",
            schema: schema_of("perl_core_harness_target_matrix_index.v1")?,
            payload: read_json(&format!("{MATRIX_FIXTURE}/index.json"))?,
            decodes: decodes::<TargetMatrixIndex>,
        },
        Envelope {
            label: "target_matrix_part.v1",
            schema: schema_of("perl_core_harness_target_matrix_part.v1")?,
            payload: read_json(&format!("{MATRIX_FIXTURE}/01-components-a.json"))?,
            decodes: decodes::<TargetMatrixPart>,
        },
        Envelope {
            label: "target_topology_drift.v1",
            schema: schema_of("perl_core_harness_target_topology_drift.v1")?,
            payload: to_value(&drift)?,
            decodes: decodes::<TargetTopologyDrift>,
        },
        Envelope {
            label: "runner_plan.v2",
            schema: schema_of("perl_core_harness_runner_plan.v2")?,
            payload: to_value(&harness_plan)?,
            decodes: decodes::<RunnerPlan>,
        },
        Envelope {
            label: "runner_parity.v1",
            schema: schema_of("perl_core_harness_runner_parity.v1")?,
            payload: to_value(&parity)?,
            decodes: decodes::<RunnerParityReport>,
        },
    ])
}

// ---------------------------------------------------------------------------
// Schema-directed walk over the canonical payload
// ---------------------------------------------------------------------------

/// How the registered schema governs one object node of a canonical payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Closure {
    /// `additionalProperties: false` — an exact versioned object.
    Exact,
    /// `additionalProperties: <schema>` — a declared extension map whose keys
    /// are the data model and whose values still carry a shape.
    ExtensionMap,
    /// Neither: an accidental escape hatch. A bare `additionalProperties: true`
    /// lands here too — it admits anything without declaring a value shape, so
    /// it is an unbounded hole rather than a declared dictionary.
    Unclassified,
}

struct ObjectNode {
    pointer: String,
    closure: Closure,
}

fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(reference) => {
            let pointer = reference.strip_prefix('#').unwrap_or(reference);
            root.pointer(pointer).map_or(node, |target| resolve(root, target))
        }
        None => node,
    }
}

/// Narrows a `oneOf`/`anyOf` position to the single branch this instance
/// actually satisfies.
///
/// `selector` and `selection_authority` are unions whose *branches* carry the
/// closure rule, so classifying the union node itself would report a correctly
/// closed contract as unclassified. An ambiguous or unmatched union is left
/// as-is and surfaces as `Unclassified`, which is the honest answer.
fn select_branch<'a>(root: &'a Value, node: &'a Value, instance: &Value) -> &'a Value {
    let node = resolve(root, node);
    let Some(branches) = node.get("oneOf").or_else(|| node.get("anyOf")).and_then(Value::as_array)
    else {
        return node;
    };
    let mut matching = branches
        .iter()
        .filter(|branch| schema_check::validate_node(root, branch, instance).is_ok());
    match (matching.next(), matching.next()) {
        (Some(branch), None) => select_branch(root, branch, instance),
        _ => node,
    }
}

/// Collects every object node reachable in `payload`, paired with how its
/// schema position governs unknown keys.
fn walk(root: &Value, schema: &Value, payload: &Value, pointer: &str, out: &mut Vec<ObjectNode>) {
    let schema = select_branch(root, schema, payload);
    match payload {
        Value::Object(fields) => {
            let closure = match schema.get("additionalProperties") {
                Some(Value::Bool(false)) => Closure::Exact,
                Some(Value::Object(_)) => Closure::ExtensionMap,
                _ => Closure::Unclassified,
            };
            out.push(ObjectNode { pointer: pointer.to_string(), closure });
            let properties = schema.get("properties").and_then(Value::as_object);
            for (key, value) in fields {
                let child = properties
                    .and_then(|properties| properties.get(key))
                    .or_else(|| schema.get("additionalProperties").filter(|node| node.is_object()));
                if let Some(child) = child {
                    walk(root, child, value, &format!("{pointer}/{key}"), out);
                }
            }
        }
        Value::Array(items) => {
            if let Some(child) = schema.get("items") {
                for (index, item) in items.iter().enumerate() {
                    walk(root, child, item, &format!("{pointer}/{index}"), out);
                }
            }
        }
        _ => {}
    }
}

fn object_nodes(envelope: &Envelope) -> Vec<ObjectNode> {
    let mut out = Vec::new();
    walk(&envelope.schema, &envelope.schema, &envelope.payload, "", &mut out);
    out
}

fn insert_at(payload: &Value, pointer: &str, key: &str, value: Value) -> Result<Value> {
    let mut mutated = payload.clone();
    let cursor = match pointer.is_empty() {
        true => &mut mutated,
        false => {
            mutated.pointer_mut(pointer).ok_or_else(|| eyre!("payload has no node at {pointer}"))?
        }
    };
    let object =
        cursor.as_object_mut().ok_or_else(|| eyre!("node at {pointer} is not an object"))?;
    object.insert(key.to_string(), value);
    Ok(mutated)
}

// ---------------------------------------------------------------------------
// The canonical payloads themselves remain valid on both sides
// ---------------------------------------------------------------------------

#[test]
fn canonical_payloads_satisfy_schema_and_rust_decoding() -> Result<()> {
    for envelope in envelopes()? {
        schema_check::validate(&envelope.schema, &envelope.payload).map_err(|error| {
            eyre!("canonical {} payload violates its registered schema: {error}", envelope.label)
        })?;
        assert!(
            (envelope.decodes)(&envelope.payload),
            "canonical {} payload must decode through its owning Rust type",
            envelope.label
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: an unknown field survives somewhere in the envelope
// ---------------------------------------------------------------------------

/// The load-bearing fixture: a valid payload plus exactly one unknown field, at
/// every object the schema declares exact — root and nested alike. Both the
/// registered schema and serde must reject it.
///
/// This fails if a `deny_unknown_fields` annotation is dropped, if an
/// `additionalProperties: false` is edited out, or if a new nested object is
/// added to a contract without closing it on either side.
#[test]
fn one_unknown_field_is_rejected_at_every_exact_object() -> Result<()> {
    let mut proven = 0_usize;
    for envelope in envelopes()? {
        for node in object_nodes(&envelope) {
            if node.closure != Closure::Exact {
                continue;
            }
            let mutated =
                insert_at(&envelope.payload, &node.pointer, UNKNOWN_KEY, json!("interloper"))?;
            let where_ = match node.pointer.is_empty() {
                true => "(root)".to_string(),
                false => node.pointer.clone(),
            };
            assert!(
                schema_check::validate(&envelope.schema, &mutated).is_err(),
                "{} schema must reject an unknown field at {where_}",
                envelope.label
            );
            assert!(
                !(envelope.decodes)(&mutated),
                "{} serde decoding must reject an unknown field at {where_}",
                envelope.label
            );
            proven += 1;
        }
    }
    // A selector that silently matched nothing would make every assertion above
    // vacuous.
    assert!(
        proven >= 50,
        "expected a populated exact-object surface, proved only {proven} mutations"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Negative control: the closure rule must not swallow declared extension maps
// ---------------------------------------------------------------------------

/// `environment`, `variant_parameters`, `topology_sources`, and the scheduling
/// property bag are dictionaries whose keys *are* the declared data. A blanket
/// "close everything" repair would break them, so the proof asserts they stay
/// open on both sides.
#[test]
fn declared_extension_maps_remain_open_on_both_sides() -> Result<()> {
    let mut proven = 0_usize;
    for envelope in envelopes()? {
        for node in object_nodes(&envelope) {
            if node.closure != Closure::ExtensionMap {
                continue;
            }
            // The declared value shape still applies to a new key (several maps
            // hold git object IDs), so the proof reuses a value the map already
            // carries. An empty map has no shape to borrow and is skipped.
            let Some(existing) = envelope
                .payload
                .pointer(&node.pointer)
                .and_then(Value::as_object)
                .and_then(|map| map.values().next())
                .cloned()
            else {
                continue;
            };
            let mutated =
                insert_at(&envelope.payload, &node.pointer, "vendor.extra.key", existing)?;
            schema_check::validate(&envelope.schema, &mutated).map_err(|error| {
                eyre!(
                    "{} extension map at {} must accept a new key: {error}",
                    envelope.label,
                    node.pointer
                )
            })?;
            assert!(
                (envelope.decodes)(&mutated),
                "{} extension map at {} must accept a new key through serde",
                envelope.label,
                node.pointer
            );
            proven += 1;
        }
    }
    assert!(
        proven >= 5,
        "expected the declared extension maps to be covered, proved only {proven} mutations"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: an object escapes classification entirely
// ---------------------------------------------------------------------------

/// Every object in a canonical payload is either exact or a declared extension
/// map. A schema position with no `additionalProperties` rule at all is an
/// accidental escape hatch: it would accept unknown fields while looking
/// governed.
#[test]
fn every_object_in_a_canonical_payload_is_classified() -> Result<()> {
    let mut unclassified = Vec::new();
    for envelope in envelopes()? {
        for node in object_nodes(&envelope) {
            if node.closure == Closure::Unclassified {
                unclassified.push(format!("{}{}", envelope.label, node.pointer));
            }
        }
    }
    assert!(
        unclassified.is_empty(),
        "schema positions carry no additionalProperties rule: {unclassified:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: a misspelled known field is read as a harmless unknown
// ---------------------------------------------------------------------------

/// The dangerous shape is not a stray field but a *misspelled* one: the real
/// field is absent, its value is carried under a near-miss name, and a
/// permissive decoder would silently fall back to a default.
#[test]
fn a_misspelled_known_field_is_never_read_as_an_unknown_extra() -> Result<()> {
    let cases: [(&str, &str, &str); 3] = [
        ("target_matrix.v1", "", "claim_boundary"),
        ("runner_plan.v2", "", "matrix_fingerprint"),
        ("runner_parity.v1", "", "membership_status"),
    ];
    for (label, pointer, field) in cases {
        let envelope = envelopes()?
            .into_iter()
            .find(|envelope| envelope.label == label)
            .ok_or_else(|| eyre!("no envelope {label}"))?;
        let mut mutated = envelope.payload.clone();
        let object = match pointer.is_empty() {
            true => &mut mutated,
            false => mutated.pointer_mut(pointer).ok_or_else(|| eyre!("missing {pointer}"))?,
        }
        .as_object_mut()
        .ok_or_else(|| eyre!("{label}{pointer} is not an object"))?;
        let carried = object
            .remove(field)
            .ok_or_else(|| eyre!("{label} canonical payload has no {field} to misspell"))?;
        object.insert(format!("{field}_"), carried);

        assert!(
            schema_check::validate(&envelope.schema, &mutated).is_err(),
            "{label} schema must reject a misspelled {field}"
        );
        assert!(
            !(envelope.decodes)(&mutated),
            "{label} serde decoding must reject a misspelled {field}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: an unknown field that looks authoritative is silently ignored
// ---------------------------------------------------------------------------

/// An unknown key whose value is shaped like a real digest or authority
/// override is the field most likely to be waved through by a permissive
/// decoder, and the most damaging one to accept: a consumer that later learned
/// to read it would be reading unvalidated input.
#[test]
fn an_authoritative_looking_unknown_field_is_rejected() -> Result<()> {
    let digest = "f".repeat(64);
    let overrides: [(&str, &str, Value); 3] = [
        ("runner_plan.v2", "matrix_fingerprint_override", json!(digest)),
        ("target_matrix.v1", "topology_digest", json!(digest)),
        ("runner_parity.v1", "membership_status_override", json!("parity")),
    ];
    for (label, key, value) in overrides {
        let envelope = envelopes()?
            .into_iter()
            .find(|envelope| envelope.label == label)
            .ok_or_else(|| eyre!("no envelope {label}"))?;
        let mutated = insert_at(&envelope.payload, "", key, value)?;
        assert!(
            schema_check::validate(&envelope.schema, &mutated).is_err(),
            "{label} schema must reject the authoritative-looking key {key}"
        );
        assert!(
            !(envelope.decodes)(&mutated),
            "{label} serde decoding must reject the authoritative-looking key {key}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: a future schema version is interpreted as the current one
// ---------------------------------------------------------------------------

/// Closed-world decoding is only half of fail-closed evolution: a payload
/// carrying otherwise-valid current fields under a *later* schema version must
/// not be read as current.
#[test]
fn a_future_schema_version_is_never_accepted_as_current() -> Result<()> {
    for envelope in envelopes()? {
        let current = envelope
            .payload
            .get("schema_version")
            .and_then(Value::as_str)
            .ok_or_else(|| eyre!("{} payload has no schema_version", envelope.label))?;
        let future = format!("{current}9");
        let mut mutated = envelope.payload.clone();
        mutated["schema_version"] = json!(future);
        assert!(
            schema_check::validate(&envelope.schema, &mutated).is_err(),
            "{} schema must reject a future schema_version",
            envelope.label
        );
    }
    Ok(())
}

/// The Rust side of the same rule, where a validator rather than serde owns it:
/// `schema_version` is a `String` field, so the closed-world serde rule cannot
/// catch a version drift on its own and the plan validator must.
#[test]
fn the_plan_validator_rejects_a_drifted_schema_version() -> Result<()> {
    let matrix = read_matrix(&repo_file(MATRIX_FIXTURE))?;
    let plan = build_runner_plan(
        &matrix,
        "component_base",
        RunnerKind::Test,
        b"t/base/if.t\n",
        RunnerScheduling::default(),
    )
    .map_err(|error| eyre!(error))?;
    crate::build::validate_runner_plan(&plan).map_err(|error| eyre!(error))?;

    let mut drifted = plan;
    drifted.schema_version = "perl_core_harness.runner_plan.v9".to_string();
    assert!(
        crate::build::validate_runner_plan(&drifted).is_err(),
        "the plan validator must reject a drifted schema version"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: a declared constraint the validator does not implement
// ---------------------------------------------------------------------------

/// The proof instrument must not fail open.
///
/// A validator that skips a keyword it does not implement reports "valid" for a
/// payload that violates a constraint the schema genuinely declares — the
/// canonical-payload proofs above would then be weaker than they read. Raised
/// by review on the first revision of this suite, where `allOf`, `anyOf`, the
/// `if`/`then` conditional, `maxItems`, and `minProperties` were all ignored.
#[test]
fn an_unimplemented_schema_keyword_is_rejected_rather_than_skipped() -> Result<()> {
    // `multipleOf` is real JSON Schema this validator does not implement.
    let schema = json!({"type": "integer", "multipleOf": 2});
    let error = schema_check::validate(&schema, &json!(3))
        .err()
        .ok_or_else(|| eyre!("an unimplemented keyword must not be silently skipped"))?;
    assert!(
        error.contains("multipleOf"),
        "the failure must name the unimplemented keyword, got: {error}"
    );

    // The keyword guard must not reject schemas built only from implemented
    // keywords, or every proof above would pass vacuously on an error.
    schema_check::validate(&json!({"type": "integer", "minimum": 1}), &json!(2))
        .map_err(|error| eyre!("an implemented keyword must still validate: {error}"))?;

    // `anyOf` and `allOf` are exercised directly rather than through the drift
    // fixture. Both appear in that schema, but its status conditional pins the
    // same two fields harder in either arm, so a fixture mutation can never
    // isolate the union — asserting otherwise would claim a control this suite
    // does not actually have.
    let union = json!({"anyOf": [{"type": "string", "minLength": 1}, {"type": "null"}]});
    schema_check::validate(&union, &json!("value"))
        .map_err(|error| eyre!("anyOf must accept a matching branch: {error}"))?;
    schema_check::validate(&union, &json!(null))
        .map_err(|error| eyre!("anyOf must accept its null branch: {error}"))?;
    assert!(
        schema_check::validate(&union, &json!("")).is_err(),
        "anyOf must reject an instance matching no branch"
    );

    let conjunction = json!({"allOf": [{"type": "integer"}, {"minimum": 10}]});
    schema_check::validate(&conjunction, &json!(11))
        .map_err(|error| eyre!("allOf must accept an instance meeting every branch: {error}"))?;
    assert!(
        schema_check::validate(&conjunction, &json!(9)).is_err(),
        "allOf must reject an instance failing one branch"
    );
    Ok(())
}

/// The drift contract states its status invariants as an `if`/`then`
/// conditional: a `not_proven` receipt must carry a null fingerprint, empty
/// added/removed/changed ID arrays, and a non-empty reason. Those constraints
/// live only in `allOf`, so a validator that ignored applicators would report
/// a contradictory receipt as schema-valid.
#[test]
fn the_drift_status_conditional_is_enforced() -> Result<()> {
    let envelope = envelopes()?
        .into_iter()
        .find(|envelope| envelope.label == "target_topology_drift.v1")
        .ok_or_else(|| eyre!("no drift envelope"))?;
    assert_eq!(
        envelope.payload.get("status").and_then(Value::as_str),
        Some("not_proven"),
        "this proof assumes the pinned drift fixture is a not_proven receipt"
    );

    // Each mutation violates one keyword the validator previously ignored,
    // while leaving the rest of the receipt well-formed. The named keyword is
    // the only thing that can reject it, so each case pins one repair.
    let contradictions = [
        // `then` + `const null`: the fingerprint a not_proven receipt may not
        // carry. This is review's own worked example.
        ("/observed_matrix_fingerprint", json!("a".repeat(64)), "conditional const"),
        // `then` + `maxItems: 0`: drift IDs a not_proven receipt cannot claim.
        ("/added_target_ids", json!(["component_base"]), "conditional maxItems"),
        ("/removed_target_ids", json!(["component_base"]), "conditional maxItems"),
        ("/changed_target_ids", json!(["component_base"]), "conditional maxItems"),
        // `then` + `minLength: 1`: a not_proven receipt must say why.
        ("/not_proven_reason", json!(""), "conditional minLength"),
        // Also the conditional, not the property-level `anyOf`: under
        // `not_proven` the `then` arm pins the fingerprint to null, so it
        // rejects any string before the `sha256`-or-null union is consulted.
        ("/observed_matrix_fingerprint", json!("not-a-digest"), "conditional const"),
        // `minProperties: 1`: the observed topology cannot be empty.
        ("/observed_topology_sources", json!({}), "minProperties"),
    ];
    for (pointer, replacement, keyword) in contradictions {
        let mut mutated = envelope.payload.clone();
        let cursor = mutated
            .pointer_mut(pointer)
            .ok_or_else(|| eyre!("drift payload has no node at {pointer}"))?;
        *cursor = replacement;
        assert!(
            schema_check::validate(&envelope.schema, &mutated).is_err(),
            "the drift schema must reject {pointer} through {keyword}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Falsifier: a reader that decodes more loosely than the library types
// ---------------------------------------------------------------------------

/// The closed-world rule has to hold at the seam production actually reads
/// through, not only for a `serde_json::from_value` in a test.
///
/// `read_matrix` and `read_drift` are the file-facing readers the CLI units
/// use. They decode into the same strict types, but nothing asserted that, so a
/// reader that grew its own permissive mirror of a contract would not be
/// caught. Each case injects one unknown field into a real pinned artifact on
/// disk and requires the reader to refuse it.
#[test]
fn the_file_readers_decode_as_strictly_as_the_library_types() -> Result<()> {
    let scratch = tempfile::tempdir().map_err(|error| eyre!(error))?;

    // --- bundle reader: index and part members ---
    let bundle = scratch.path().join("matrix");
    std::fs::create_dir_all(&bundle).map_err(|error| eyre!(error))?;
    let source = repo_file(MATRIX_FIXTURE);
    for entry in std::fs::read_dir(&source).map_err(|error| eyre!(error))? {
        let entry = entry.map_err(|error| eyre!(error))?;
        std::fs::copy(entry.path(), bundle.join(entry.file_name()))
            .map_err(|error| eyre!(error))?;
    }
    // Control: the copy is readable, so a later rejection is caused by the
    // injected field rather than by the copy itself.
    read_matrix(&bundle).map_err(|error| eyre!("the copied bundle must read cleanly: {error}"))?;

    for member in ["index.json", "01-components-a.json"] {
        let path = bundle.join(member);
        let original = std::fs::read(&path).map_err(|error| eyre!(error))?;
        let mut document: Value =
            serde_json::from_slice(&original).map_err(|error| eyre!(error))?;
        document
            .as_object_mut()
            .ok_or_else(|| eyre!("{member} is not an object"))?
            .insert(UNKNOWN_KEY.to_string(), json!("interloper"));
        std::fs::write(&path, serde_json::to_vec(&document).map_err(|error| eyre!(error))?)
            .map_err(|error| eyre!(error))?;
        assert!(
            read_matrix(&bundle).is_err(),
            "read_matrix must refuse an unknown field in {member}"
        );
        std::fs::write(&path, &original).map_err(|error| eyre!(error))?;
    }
    // Restoring every member returns the bundle to a readable state, so the
    // rejections above were not cumulative damage.
    read_matrix(&bundle)
        .map_err(|error| eyre!("the restored bundle must read cleanly: {error}"))?;

    // --- drift reader ---
    let drift = scratch.path().join("drift.json");
    let original = std::fs::read(repo_file(DRIFT_FIXTURE)).map_err(|error| eyre!(error))?;
    std::fs::write(&drift, &original).map_err(|error| eyre!(error))?;
    read_drift(&drift)
        .map_err(|error| eyre!("the copied drift receipt must read cleanly: {error}"))?;

    let mut document: Value = serde_json::from_slice(&original).map_err(|error| eyre!(error))?;
    document
        .as_object_mut()
        .ok_or_else(|| eyre!("the drift receipt is not an object"))?
        .insert(UNKNOWN_KEY.to_string(), json!("interloper"));
    std::fs::write(&drift, serde_json::to_vec(&document).map_err(|error| eyre!(error))?)
        .map_err(|error| eyre!(error))?;
    assert!(read_drift(&drift).is_err(), "read_drift must refuse an unknown field");
    Ok(())
}

/// A schema the validator cannot evaluate must not be absorbed by a combinator.
///
/// `anyOf`, `oneOf`, and `if` all treat a failing branch as an ordinary
/// non-match. Before this control, an unsupported keyword inside a branch was
/// reported the same way, so it vanished whenever a sibling branch matched (or,
/// under `if`, silently selected the other arm). The fail-closed guarantee was
/// therefore only true at the top level. Raised by exact-head review as
/// `{"anyOf": [{"futureKeyword": true}, true]}` validating successfully.
#[test]
fn an_unimplemented_keyword_is_not_swallowed_by_a_combinator() -> Result<()> {
    let unsupported = json!({"futureKeyword": true});

    // Review's own falsifier, plus the reversed order so the result cannot
    // depend on the unevaluable branch being visited first.
    let cases: [(&str, Value); 6] = [
        ("anyOf, unsupported first", json!({"anyOf": [unsupported, true]})),
        ("anyOf, unsupported second", json!({"anyOf": [true, unsupported]})),
        ("oneOf, unsupported first", json!({"oneOf": [unsupported, true]})),
        ("oneOf, unsupported second", json!({"oneOf": [true, unsupported]})),
        ("if condition", json!({"if": unsupported, "then": true, "else": true})),
        ("allOf", json!({"allOf": [true, unsupported]})),
    ];
    for (label, schema) in cases {
        let error = schema_check::validate(&schema, &json!("anything")).err().ok_or_else(|| {
            eyre!("{label}: an unimplemented keyword inside a combinator must not be swallowed")
        })?;
        assert!(
            error.contains("futureKeyword"),
            "{label}: the failure must name the unimplemented keyword, got: {error}"
        );
    }

    // Negative control in the other direction: an ordinary instance mismatch is
    // still branch-local, or the repair would have turned every union into a
    // conjunction.
    let union = json!({"anyOf": [{"type": "string"}, {"type": "null"}]});
    schema_check::validate(&union, &json!("value"))
        .map_err(|error| eyre!("a non-matching sibling branch must stay local: {error}"))?;
    schema_check::validate(&union, &json!(null))
        .map_err(|error| eyre!("a non-matching sibling branch must stay local: {error}"))?;
    assert!(
        schema_check::validate(&union, &json!(7)).is_err(),
        "a union matching no branch must still be rejected"
    );

    // The same distinction for `if`: a condition that merely does not match is
    // a legitimate `else` selection, not an error.
    let conditional =
        json!({"if": {"type": "string"}, "then": {"minLength": 3}, "else": {"type": "integer"}});
    schema_check::validate(&conditional, &json!("abc"))
        .map_err(|error| eyre!("matching condition must take then: {error}"))?;
    schema_check::validate(&conditional, &json!(1))
        .map_err(|error| eyre!("non-matching condition must take else: {error}"))?;
    assert!(
        schema_check::validate(&conditional, &json!("ab")).is_err(),
        "the then arm must still reject a too-short string"
    );
    Ok(())
}

/// An unresolvable `$ref` is likewise a schema fault, not a non-match.
#[test]
fn an_unresolvable_ref_is_not_swallowed_by_a_combinator() -> Result<()> {
    let schema = json!({"anyOf": [{"$ref": "#/$defs/absent"}, true]});
    let error = schema_check::validate(&schema, &json!("anything"))
        .err()
        .ok_or_else(|| eyre!("an unresolved $ref inside anyOf must not be swallowed"))?;
    assert!(error.contains("unresolved"), "the failure must name the unresolved ref, got: {error}");
    Ok(())
}

/// A sibling of `$ref` is a real constraint, not decoration.
///
/// These schemas declare draft 2020-12, where `$ref` composes with its siblings
/// rather than replacing them. Treating `$ref` as terminal would silently skip
/// any sibling assertion — the same fail-open shape the keyword guard exists to
/// prevent. Raised by review; no envelope this suite validates pairs them
/// today, but `agent_review_packet.v1` already pairs `$ref` with `minItems`
/// elsewhere in `schemas/`, so the shape is live in the repository.
#[test]
fn a_sibling_of_a_ref_is_still_enforced() -> Result<()> {
    let schema = json!({
        "$defs": {"list": {"type": "array"}},
        "$ref": "#/$defs/list",
        "minItems": 2
    });

    // The referenced target and the sibling must both hold.
    schema_check::validate(&schema, &json!(["a", "b"]))
        .map_err(|error| eyre!("an instance satisfying ref and sibling must pass: {error}"))?;
    assert!(
        schema_check::validate(&schema, &json!(["a"])).is_err(),
        "the sibling constraint alongside $ref must be enforced, not skipped"
    );
    assert!(
        schema_check::validate(&schema, &json!("not-an-array")).is_err(),
        "the referenced target must still be enforced"
    );

    // A schema error inside the referenced target still propagates.
    let broken = json!({"$defs": {"bad": {"futureKeyword": true}}, "$ref": "#/$defs/bad"});
    let error = schema_check::validate(&broken, &json!("anything"))
        .err()
        .ok_or_else(|| eyre!("an unimplemented keyword behind a $ref must not be swallowed"))?;
    assert!(error.contains("futureKeyword"), "the failure must name the keyword, got: {error}");
    Ok(())
}
