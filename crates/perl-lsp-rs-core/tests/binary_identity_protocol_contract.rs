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
            typescript.contains(&format!("\"{literal}\"")),
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
            typescript.contains(&format!("\"{name}\"")),
            "TypeScript projection is missing compatibility state {name:?}"
        );
    }

    for reason in [
        BinaryCompatibilityReason::ServerProductMismatch,
        BinaryCompatibilityReason::ProductRepositoryMismatch,
        BinaryCompatibilityReason::PacketSchemaUnsupported,
        BinaryCompatibilityReason::ProductIdentityVersionUnsupported,
        BinaryCompatibilityReason::DapPostureMismatch,
        BinaryCompatibilityReason::ExtensionPublisherMismatch,
        BinaryCompatibilityReason::ExtensionPackageMismatch,
        BinaryCompatibilityReason::ExtensionIdentityMismatch,
        BinaryCompatibilityReason::ExtensionAuthorityNotProven,
        BinaryCompatibilityReason::ExtensionPackageDigestNotProven,
        BinaryCompatibilityReason::VersionMismatch,
        BinaryCompatibilityReason::TargetMismatch,
        BinaryCompatibilityReason::TargetNotProven,
        BinaryCompatibilityReason::SourceRevisionMismatch,
        BinaryCompatibilityReason::SourceRevisionNotProven,
        BinaryCompatibilityReason::SourceTreeDigestMismatch,
        BinaryCompatibilityReason::SourceTreeDigestNotProven,
        BinaryCompatibilityReason::ProfileMismatch,
        BinaryCompatibilityReason::ProfileNotProven,
        BinaryCompatibilityReason::CandidateMismatch,
        BinaryCompatibilityReason::CandidateNotProven,
        BinaryCompatibilityReason::ArtifactRoleMismatch,
        BinaryCompatibilityReason::ArtifactRoleNotProven,
        BinaryCompatibilityReason::ArtifactDigestMismatch,
        BinaryCompatibilityReason::ArtifactDigestNotProven,
        BinaryCompatibilityReason::DapRoleMismatch,
        BinaryCompatibilityReason::DapIdentityAbsent,
        BinaryCompatibilityReason::BuildIdentityPartial,
        BinaryCompatibilityReason::BuildIdentityNotProven,
        BinaryCompatibilityReason::PayloadNotRedacted,
        BinaryCompatibilityReason::ServerInstanceStale,
        BinaryCompatibilityReason::EnvironmentSnapshotStale,
        BinaryCompatibilityReason::FeatureVersionUnsupported,
        BinaryCompatibilityReason::ExactIdentityMatch,
    ] {
        let name = serde_name(reason)?;
        assert!(
            typescript.contains(&format!("\"{name}\"")),
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
    Ok(())
}
