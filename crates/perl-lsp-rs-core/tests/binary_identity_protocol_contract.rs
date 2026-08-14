use perl_lsp_rs_core::protocol::binary_identity::{
    BINARY_COMPATIBILITY_METHOD, BINARY_IDENTITY_FEATURE_VERSION, BINARY_IDENTITY_METHOD,
    BinaryCompatibilityReason, BinaryCompatibilityState, CANONICAL_DAP_POSTURE,
    CANONICAL_EXTENSION_ID, CANONICAL_EXTENSION_PACKAGE, CANONICAL_EXTENSION_PUBLISHER,
};
use serde::Serialize;
use serde_json::Value;
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
    Ok(())
}

/// Every current Rust reason, including the field-specific partial reasons.
fn all_reasons() -> Vec<BinaryCompatibilityReason> {
    use BinaryCompatibilityReason::*;
    vec![
        ServerProductMismatch,
        ProductRepositoryMismatch,
        PacketSchemaUnsupported,
        ProductIdentityVersionUnsupported,
        DapPostureMismatch,
        ExtensionPublisherMismatch,
        ExtensionPackageMismatch,
        ExtensionIdentityMismatch,
        ExtensionAuthorityNotProven,
        ExtensionPackageDigestNotProven,
        VersionMismatch,
        TargetMismatch,
        TargetNotProven,
        SourceRevisionMismatch,
        SourceRevisionNotProven,
        SourceTreeDigestMismatch,
        SourceTreeDigestNotProven,
        ProfileMismatch,
        ProfileNotProven,
        CandidateMismatch,
        CandidateNotProven,
        ArtifactRoleMismatch,
        ArtifactRoleNotProven,
        ArtifactDigestMismatch,
        ArtifactDigestNotProven,
        DapRoleMismatch,
        DapIdentityAbsent,
        BuildIdentityPartial,
        BuildIdentityNotProven,
        PayloadNotRedacted,
        ServerInstanceStale,
        EnvironmentSnapshotStale,
        FeatureVersionUnsupported,
        ExactIdentityMatch,
    ]
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

    for reason in all_reasons() {
        let name = serde_name(reason)?;
        assert!(
            typescript_declares(&typescript, &name),
            "TypeScript projection is missing compatibility reason {name:?}"
        );
    }
    assert!(
        typescript.contains("KnownBinaryCompatibilityReason | (string & {})"),
        "TypeScript client lost its bounded unknown-reason representation"
    );
    assert!(
        typescript.contains("redacted: boolean"),
        "TypeScript projection still treats redaction as unconditionally true"
    );
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

    // Every Rust state and reason must remain distinctly representable in the
    // schema: the state enum must contain each serde name and every reason
    // (including the field-specific partial and mismatch reasons) must satisfy
    // the bounded reasonToken grammar the schema declares.
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
    for reason in all_reasons() {
        let name = serde_name(reason)?;
        assert_reason_admitted_by_schema_token(&schema, &name)?;
    }
    Ok(())
}
