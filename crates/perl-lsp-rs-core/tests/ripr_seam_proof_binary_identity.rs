//! Seam proofs for the binary identity protocol contract.
//!
//! The repo's ripr exposure policy designates tests/ripr_seam_proof_*.rs
//! as the home for focused seam proofs: this file holds the schema/projection
//! contract assertions together with the exact-message observation tests for
//! every error variant its helpers can produce, so the seams stay revealed by
//! assertions rather than flagged as reachable-but-unobserved drift.
use perl_lsp_rs_core::protocol::binary_identity::{
    BINARY_COMPATIBILITY_METHOD, BINARY_IDENTITY_FEATURE_VERSION, BINARY_IDENTITY_METHOD,
    BinaryCompatibilityReason, BinaryCompatibilityState, CANONICAL_DAP_POSTURE,
    CANONICAL_EXTENSION_ID, CANONICAL_EXTENSION_PACKAGE, CANONICAL_EXTENSION_PUBLISHER,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn repository_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let crates_dir = manifest_dir
        .parent()
        .ok_or("perl-lsp-rs-core must live below the workspace crates directory")?;
    let root = crates_dir
        .parent()
        .ok_or("workspace crates directory must live below the repository root")?;
    Ok(root.to_path_buf())
}

fn serde_name(value: impl Serialize) -> Result<String, Box<dyn std::error::Error>> {
    let encoded = serde_json::to_string(&value)?;
    let name = encoded
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or("serialized enum value must be a JSON string")?;
    Ok(name.to_owned())
}

fn read(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(path)?)
}

/// The formatter owns the TypeScript quote style, so accept either delimiter
/// while still requiring the exact literal content as a string token.
fn typescript_declares(typescript: &str, literal: &str) -> bool {
    typescript.contains(&format!("\"{literal}\"")) || typescript.contains(&format!("'{literal}'"))
}

/// Assert one schema token grammar that every Rust-emitted reason must satisfy.
fn assert_reason_admitted_by_schema_token(
    schema: &Value,
    name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let token = &schema["$defs"]["reasonToken"];
    let pattern = token["pattern"].as_str().ok_or("reasonToken pattern must be a string")?;
    if pattern != "^[a-z0-9_]+$" {
        return Err(format!("reasonToken grammar drifted: {pattern:?}").into());
    }
    let max_length =
        token["maxLength"].as_u64().ok_or("reasonToken maxLength must be an integer")?;
    if name.is_empty()
        || name.len() > usize::try_from(max_length).map_err(|error| error.to_string())?
        || !name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
    {
        return Err(
            format!("Rust reason {name:?} is not admissible as a schema reasonToken").into()
        );
    }
    let enumerated = token["enum"]
        .as_array()
        .ok_or("reasonToken enum must enumerate the current reason vocabulary")?;
    if !enumerated.iter().any(|value| value == name) {
        return Err(format!("reason {name:?} is not listed in the schema reasonToken enum").into());
    }
    Ok(())
}

fn schema_reason_enum(schema: &Value) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    schema["$defs"]["reasonToken"]["enum"]
        .as_array()
        .ok_or("reasonToken enum must be an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| "reasonToken enum entries must be strings".into())
                .map(str::to_owned)
        })
        .collect()
}

/// Observable oracle for schema reason parity against [`BinaryCompatibilityReason::ALL`].
fn assert_schema_reason_inventory_matches_rust(
    schema: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let enumerated = schema_reason_enum(schema)?;
    let enum_set: HashSet<&str> = enumerated.iter().map(String::as_str).collect();
    for &reason in BinaryCompatibilityReason::ALL {
        let name = serde_name(reason)?;
        if !enum_set.contains(name.as_str()) {
            return Err(format!("schema reasonToken enum is missing reason {name:?}").into());
        }
    }
    for name in &enumerated {
        if serde_json::from_str::<BinaryCompatibilityReason>(&format!("\"{name}\"")).is_err() {
            return Err(format!("schema reasonToken enum lists orphan reason {name:?}").into());
        }
    }
    if enumerated.len() != BinaryCompatibilityReason::ALL.len() {
        return Err(format!(
            "schema reasonToken enum lists {} reasons but Rust inventory has {}",
            enumerated.len(),
            BinaryCompatibilityReason::ALL.len()
        )
        .into());
    }
    Ok(())
}

fn extract_known_typescript_reason_literals(
    typescript: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    const START: &str = "export type KnownBinaryCompatibilityReason =";
    const END: &str = "export type BinaryCompatibilityReason =";
    let start = typescript
        .find(START)
        .ok_or("TypeScript projection is missing KnownBinaryCompatibilityReason")?;
    let end =
        typescript.find(END).ok_or("TypeScript projection is missing BinaryCompatibilityReason")?;
    if end <= start {
        return Err(
            "KnownBinaryCompatibilityReason must precede BinaryCompatibilityReason in the projection"
                .into(),
        );
    }
    let block = &typescript[start..end];
    let mut literals = Vec::new();
    for line in block.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix('|') else {
            continue;
        };
        let rest = rest.trim();
        let rest = rest.strip_suffix(';').unwrap_or(rest);
        let literal = rest
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
            .or_else(|| rest.strip_prefix('"').and_then(|value| value.strip_suffix('"')))
            .ok_or_else(|| format!("could not parse reason literal from line {trimmed:?}"))?;
        literals.push(literal.to_owned());
    }
    Ok(literals)
}

/// Observable oracle for reverse TypeScript reason parity: every
/// `KnownBinaryCompatibilityReason` literal must deserialize to a Rust variant,
/// and every Rust reason must appear in the union.
fn assert_typescript_projection_matches_rust_inventory(
    typescript: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let literals = extract_known_typescript_reason_literals(typescript)?;
    let literal_set: HashSet<&str> = literals.iter().map(String::as_str).collect();
    for &reason in BinaryCompatibilityReason::ALL {
        let name = serde_name(reason)?;
        if !literal_set.contains(name.as_str()) {
            return Err(
                format!("TypeScript projection is missing compatibility reason {name:?}").into()
            );
        }
    }
    for literal in &literals {
        if serde_json::from_str::<BinaryCompatibilityReason>(&format!("\"{literal}\"")).is_err() {
            return Err(format!(
                "TypeScript projection lists orphan compatibility reason {literal:?}"
            )
            .into());
        }
    }
    if literals.len() != BinaryCompatibilityReason::ALL.len() {
        return Err(format!(
            "TypeScript projection lists {} reasons but Rust inventory has {}",
            literals.len(),
            BinaryCompatibilityReason::ALL.len()
        )
        .into());
    }
    Ok(())
}

/// Observable oracle for reason parity: returns the drift message instead of
/// panicking inline so the missing-reason failure variant is assertable.
///
/// The enumeration is [`BinaryCompatibilityReason::ALL`], which the enum's own
/// declaration generates from the same list that names the variants. There is no
/// second copy here to fall behind it, so a newly added reason is covered by this
/// check from the moment the variant exists.
fn assert_projection_declares_every_reason(
    typescript: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for &reason in BinaryCompatibilityReason::ALL {
        let name = serde_name(reason)?;
        if !typescript_declares(typescript, &name) {
            return Err(
                format!("TypeScript projection is missing compatibility reason {name:?}").into()
            );
        }
    }
    Ok(())
}

#[test]
fn checked_typescript_projection_contains_every_current_literal()
-> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let typescript = read(&root.join("vscode-extension/src/binaryIdentityProtocol.generated.ts"))?;

    for literal in [
        BINARY_IDENTITY_METHOD,
        BINARY_COMPATIBILITY_METHOD,
        CANONICAL_EXTENSION_PUBLISHER,
        CANONICAL_EXTENSION_PACKAGE,
        CANONICAL_EXTENSION_ID,
        CANONICAL_DAP_POSTURE,
        "perl-lsp",
        "EffortlessMetrics/perl-lsp",
        "EffortlessMetrics/perl-lsp-swarm",
    ] {
        assert!(
            typescript_declares(&typescript, literal),
            "TypeScript projection is missing literal {literal:?}"
        );
    }
    assert!(
        typescript.contains(&format!(
            "BINARY_IDENTITY_FEATURE_VERSION = {BINARY_IDENTITY_FEATURE_VERSION}"
        )),
        "TypeScript feature-family version drifted"
    );

    for state in [
        BinaryCompatibilityState::ExactMatch,
        BinaryCompatibilityState::CompatiblePartial,
        BinaryCompatibilityState::Mismatch,
        BinaryCompatibilityState::Unsupported,
        BinaryCompatibilityState::Stale,
        BinaryCompatibilityState::NotProven,
    ] {
        let name = serde_name(state)?;
        assert!(
            typescript_declares(&typescript, &name),
            "TypeScript projection is missing compatibility state {name:?}"
        );
    }

    assert_projection_declares_every_reason(&typescript)?;
    assert_typescript_projection_matches_rust_inventory(&typescript)?;
    assert_projection_bounds_unknown_reasons_and_redaction(&typescript)?;
    Ok(())
}

#[test]
fn every_reason_dropped_from_the_projection_is_reported() -> Result<(), Box<dyn std::error::Error>>
{
    // Negative control for the parity oracle, run once per reason. A green
    // `checked_typescript_projection_contains_every_current_literal` only means
    // "no reason the check walked is missing"; on its own it cannot distinguish a
    // complete projection from a short enumeration. Removing each reason in turn
    // and requiring it to be named proves the walk is not vacuous for any of
    // them, so a reason that exists but is skipped would surface here.
    let root = repository_root()?;
    let typescript = read(&root.join("vscode-extension/src/binaryIdentityProtocol.generated.ts"))?;
    let placeholder = "reason_withheld_by_this_control";

    for &reason in BinaryCompatibilityReason::ALL {
        let name = serde_name(reason)?;
        let doctored = typescript
            .replace(&format!("'{name}'"), &format!("'{placeholder}'"))
            .replace(&format!("\"{name}\""), &format!("\"{placeholder}\""));
        assert!(
            !typescript_declares(&doctored, &name),
            "the control must actually withhold reason {name:?} from the projection"
        );

        let error = match assert_projection_declares_every_reason(&doctored) {
            Err(error) => error.to_string(),
            Ok(()) => {
                return Err(
                    format!("a projection withholding reason {name:?} must be rejected").into()
                );
            }
        };
        assert!(
            error.contains(&name),
            "the withheld reason must be named in the report, got: {error}"
        );
    }
    Ok(())
}

/// Observable oracle for the projection markers: returns the drift messages
/// instead of panicking inline so the failure variants are assertable.
fn assert_projection_bounds_unknown_reasons_and_redaction(
    typescript: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !typescript.contains("KnownBinaryCompatibilityReason | (string & {})") {
        return Err("TypeScript client lost its bounded unknown-reason representation".into());
    }
    if !typescript.contains("redacted: boolean") {
        return Err("TypeScript projection still treats redaction as unconditionally true".into());
    }
    Ok(())
}

#[test]
fn response_schema_preserves_mismatch_and_redaction_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;

    assert_eq!(schema["properties"]["feature_version"]["const"], BINARY_IDENTITY_FEATURE_VERSION);
    assert_eq!(schema["properties"]["redacted"]["type"], "boolean");
    assert_eq!(schema["properties"]["reasons"]["items"]["$ref"], "#/$defs/reasonToken");
    assert_eq!(
        schema["$defs"]["extensionIdentity"]["properties"]["id"]["$ref"],
        "#/$defs/extensionId"
    );

    let required = schema["$defs"]["extensionIdentity"]["required"]
        .as_array()
        .ok_or("extension identity required set must be an array")?;
    for field in
        ["publisher", "package_name", "id", "version", "binary_artifact_role", "authority_identity"]
    {
        assert!(
            required.iter().any(|value| value == field),
            "schema extension identity is missing required field {field:?}"
        );
    }
    for field in ["package_sha256", "server_sha256", "dap_sha256"] {
        assert!(
            schema["$defs"]["extensionIdentity"]["properties"].get(field).is_some(),
            "schema extension identity is missing digest field {field:?}"
        );
    }

    // Every Rust state must remain in the schema compatibility enum, and the
    // reason inventory must match in both directions: Rust reasons are listed
    // in the schema enum and every schema enum entry deserializes to Rust.
    for state in [
        BinaryCompatibilityState::ExactMatch,
        BinaryCompatibilityState::CompatiblePartial,
        BinaryCompatibilityState::Mismatch,
        BinaryCompatibilityState::Unsupported,
        BinaryCompatibilityState::Stale,
        BinaryCompatibilityState::NotProven,
    ] {
        let name = serde_name(state)?;
        assert!(
            schema["properties"]["compatibility"]["enum"]
                .as_array()
                .ok_or("compatibility enum must be an array")?
                .iter()
                .any(|value| value == &name),
            "schema compatibility enum is missing state {name:?}"
        );
    }
    assert_schema_reason_inventory_matches_rust(&schema)?;
    Ok(())
}

/// Assert the wire schema's `reasons` array bounds admit the worst-case response.
///
/// The union return path (#10184) lets one response carry every collected reason
/// at once, and the Rust type system does not bound the array. The schema's
/// `minItems`/`maxItems`/`uniqueItems` are therefore the sole enforcer that a
/// worst-case response stays admissible, so bind them to the real inventory.
fn assert_reason_array_bounds_admit_the_union(
    schema: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let reasons = &schema["properties"]["reasons"];
    let inventory = BinaryCompatibilityReason::ALL.len();

    let max_items =
        reasons["maxItems"].as_u64().ok_or("schema reasons array declares no maxItems")? as usize;
    if inventory > max_items {
        return Err(format!(
            "the union path can emit {inventory} reasons, exceeding schema maxItems {max_items}"
        )
        .into());
    }

    // Every terminal branch of `evaluate_compatibility` returns at least one
    // reason (`ExactMatch` returns exactly one), so the schema must demand
    // exactly that: `minItems: 1` in both directions. Accepting `0` would let
    // the schema stop enforcing the production invariant that every response
    // carries a reason (#15571 FC6).
    let min_items =
        reasons["minItems"].as_u64().ok_or("schema reasons array declares no minItems")? as usize;
    if min_items != 1 {
        return Err(format!(
            "schema minItems must be exactly 1 (one reason per response, ExactMatch minimum); found {min_items}"
        )
        .into());
    }

    if reasons["uniqueItems"] != Value::Bool(true) {
        return Err("schema reasons array no longer declares uniqueItems".into());
    }
    Ok(())
}

#[test]
fn schema_bounds_reject_a_doctored_zero_minimum() -> Result<(), Box<dyn std::error::Error>> {
    // Negative control for the minItems bind (#15571 FC6): if the schema
    // stopped enforcing `minItems: 1`, the bound check must fail rather than
    // let responses without reasons validate.
    let root = repository_root()?;
    let mut schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;
    schema["properties"]["reasons"]["minItems"] = serde_json::json!(0);
    assert!(
        assert_reason_array_bounds_admit_the_union(&schema).is_err(),
        "the minItems bind must reject a schema that stops requiring a reason"
    );
    Ok(())
}

#[test]
fn response_reason_array_bounds_admit_the_worst_case_union()
-> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;
    assert_reason_array_bounds_admit_the_union(&schema)
}

#[test]
fn reason_array_bound_drift_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    // Observes each bound error variant so the oracle above is not vacuous: a
    // schema that could not admit the union must be rejected, not passed over.
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;

    let mut narrowed = schema.clone();
    narrowed["properties"]["reasons"]["maxItems"] = Value::from(1);
    match assert_reason_array_bounds_admit_the_union(&narrowed) {
        Err(error) => assert!(
            error.to_string().contains("exceeding schema maxItems 1"),
            "maxItems drift must name the offending bound, got {error}"
        ),
        Ok(()) => return Err("a maxItems below the inventory must be rejected".into()),
    }

    let mut demanding = schema.clone();
    demanding["properties"]["reasons"]["minItems"] = Value::from(2);
    match assert_reason_array_bounds_admit_the_union(&demanding) {
        Err(error) => assert!(
            error.to_string().contains("must be exactly 1")
                && error.to_string().contains("found 2"),
            "minItems drift must name the offending bound, got {error}"
        ),
        Ok(()) => return Err("a minItems above the ExactMatch arity must be rejected".into()),
    }

    let mut duplicable = schema;
    duplicable["properties"]["reasons"]["uniqueItems"] = Value::Bool(false);
    match assert_reason_array_bounds_admit_the_union(&duplicable) {
        Err(error) => assert!(
            error.to_string().contains("uniqueItems"),
            "uniqueItems drift must be reported, got {error}"
        ),
        Ok(()) => return Err("a schema dropping uniqueItems must be rejected".into()),
    }
    Ok(())
}

#[test]
fn reason_token_grammar_drift_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    // Observes the schema-drift error variant: a reasonToken pattern that no
    // longer admits the current snake_case reason grammar must be named,
    // not silently accepted.
    let drifted = serde_json::json!({
        "$defs": {"reasonToken": {"pattern": "^[A-Z]+$", "maxLength": 64}}
    });
    let error = match assert_reason_admitted_by_schema_token(&drifted, "server_product_mismatch") {
        Err(error) => error.to_string(),
        Ok(()) => return Err("drifted reasonToken grammar must be rejected".into()),
    };
    assert!(
        error.contains("reasonToken grammar drifted"),
        "grammar drift must be reported with the drifted pattern, got: {error}"
    );
    assert!(
        error.contains("^[A-Z]+$"),
        "the drifted pattern itself must appear in the report, got: {error}"
    );
    Ok(())
}

#[test]
fn inadmissible_reason_names_are_rejected_by_the_schema_token_contract()
-> Result<(), Box<dyn std::error::Error>> {
    // Observes the admissibility error variant at the schema's own
    // maxLength boundary: names that are empty, carry non-snake_case
    // characters, or exceed maxLength must be named as inadmissible.
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;
    let max_length = usize::try_from(
        schema["$defs"]["reasonToken"]["maxLength"]
            .as_u64()
            .ok_or("reasonToken maxLength must be an integer")?,
    )
    .map_err(|error| error.to_string())?;

    let overlong = "a".repeat(max_length + 1);
    for inadmissible in ["", "Has-Uppercase", &overlong] {
        let error = match assert_reason_admitted_by_schema_token(&schema, inadmissible) {
            Err(error) => error.to_string(),
            Ok(()) => return Err("inadmissible reason name must be rejected".into()),
        };
        assert!(
            error.contains("not admissible as a schema reasonToken"),
            "inadmissible name {inadmissible:?} must be reported as such, got: {error}"
        );
    }
    Ok(())
}

#[test]
fn projection_marker_drift_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    // Observes the marker-oracle failure variants: a projection that drops
    // either the bounded unknown-reason representation or the redaction
    // marker must be named with the exact drift message.
    let root = repository_root()?;
    let typescript = read(&root.join("vscode-extension/src/binaryIdentityProtocol.generated.ts"))?;

    let dropped_redaction = typescript.replace("redacted: boolean", "redacted: string");
    let error = match assert_projection_bounds_unknown_reasons_and_redaction(&dropped_redaction) {
        Err(error) => error.to_string(),
        Ok(()) => return Err("projection without the redaction marker must be rejected".into()),
    };
    assert_eq!(
        error, "TypeScript projection still treats redaction as unconditionally true",
        "redaction drift must be reported with its exact message"
    );

    let dropped_unknown = typescript.replace("KnownBinaryCompatibilityReason | (string & {})", "");
    let error = match assert_projection_bounds_unknown_reasons_and_redaction(&dropped_unknown) {
        Err(error) => error.to_string(),
        Ok(()) => return Err("projection without the unknown-reason bound must be rejected".into()),
    };
    assert_eq!(
        error, "TypeScript client lost its bounded unknown-reason representation",
        "unknown-reason drift must be reported with its exact message"
    );
    Ok(())
}

#[test]
fn schema_reason_inventory_matches_rust_authority() -> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;
    assert_schema_reason_inventory_matches_rust(&schema)
}

#[test]
fn unknown_inventory_reason_is_rejected_by_schema_token_contract()
-> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;
    let error = match assert_reason_admitted_by_schema_token(&schema, "throwaway_probe") {
        Err(error) => error.to_string(),
        Ok(()) => return Err("unknown inventory reason must be rejected by the schema enum".into()),
    };
    assert!(
        error.contains("not listed in the schema reasonToken enum"),
        "unknown reason rejection must name the enum contract, got: {error}"
    );
    Ok(())
}

#[test]
fn schema_reason_enum_drift_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let schema: Value = serde_json::from_str(&read(
        &root.join("schemas/binary_identity_protocol.v1.schema.json"),
    )?)?;

    let missing = serde_name(BinaryCompatibilityReason::ServerProductMismatch)?;
    let mut shortened = schema.clone();
    let enum_values = shortened["$defs"]["reasonToken"]["enum"]
        .as_array_mut()
        .ok_or("reasonToken enum must be an array")?;
    enum_values.retain(|value| value != &missing);
    let error = match assert_schema_reason_inventory_matches_rust(&shortened) {
        Err(error) => error.to_string(),
        Ok(()) => return Err("schema enum missing a current reason must be rejected".into()),
    };
    assert!(
        error.contains(&missing),
        "missing schema enum entry must name the reason, got: {error}"
    );

    let mut orphan = schema;
    orphan["$defs"]["reasonToken"]["enum"]
        .as_array_mut()
        .ok_or("reasonToken enum must be an array")?
        .push(Value::String("orphaned_reason_literal".into()));
    let error = match assert_schema_reason_inventory_matches_rust(&orphan) {
        Err(error) => error.to_string(),
        Ok(()) => return Err("schema enum listing an orphan reason must be rejected".into()),
    };
    assert!(
        error.contains("orphaned_reason_literal"),
        "orphan schema enum entry must be named, got: {error}"
    );
    Ok(())
}

#[test]
fn every_orphan_typescript_reason_literal_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    let root = repository_root()?;
    let typescript = read(&root.join("vscode-extension/src/binaryIdentityProtocol.generated.ts"))?;
    let placeholder = "reason_withheld_by_this_control";

    for literal in extract_known_typescript_reason_literals(&typescript)? {
        let doctored = typescript
            .replace(&format!("'{literal}'"), &format!("'{placeholder}'"))
            .replace(&format!("\"{literal}\""), &format!("\"{placeholder}\""));
        let error = match assert_typescript_projection_matches_rust_inventory(&doctored) {
            Err(error) => error.to_string(),
            Ok(()) => {
                return Err(format!(
                    "a projection withholding reason {literal:?} must be rejected"
                )
                .into());
            }
        };
        assert!(
            error.contains(&literal),
            "the orphaned literal must be named in the report, got: {error}"
        );
    }
    Ok(())
}
