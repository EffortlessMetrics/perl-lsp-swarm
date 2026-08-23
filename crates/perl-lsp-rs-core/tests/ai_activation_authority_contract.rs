use serde_json::Value;
use std::{error::Error, fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn generic_lsp_cannot_regain_remote_ai_arm_or_selection_authority() -> Result<(), Box<dyn Error>> {
    let root = root();
    let config = fs::read_to_string(root.join("crates/perl-lsp-rs-core/src/config/mod.rs"))?;
    let catalog = fs::read_to_string(
        root.join("crates/perl-lsp-rs-core/src/configuration_authority/catalog.rs"),
    )?;
    let runtime = fs::read_to_string(root.join("crates/perl-lsp-rs/src/runtime/mod.rs"))?;
    let schema_text = fs::read_to_string(root.join("schemas/perllsp-settings.schema.json"))?;
    let schema: Value = serde_json::from_str(&schema_text)?;

    for forbidden in [
        "self.ai_completion.user_enabled = enabled",
        "self.ai_completion.provider = provider.to_string()",
        "self.ai_completion.model = model.to_string()",
    ] {
        assert!(!config.contains(forbidden), "generic parser authority returned: {forbidden}");
    }

    assert!(catalog.contains("const AI_TRUSTED_ARM_SELECT"));
    for id in ["ai.user_enabled", "ai.provider", "ai.model"] {
        let start = catalog.find(&format!("\"{id}\"")).ok_or("catalog row")?;
        let tail = &catalog[start..];
        let end = tail.find("    ),").ok_or("catalog row end")?;
        assert!(
            tail[..end].contains("AI_TRUSTED_ARM_SELECT"),
            "{id} must use trusted arm/select sources"
        );
    }

    let ai = &schema["properties"]["perl"]["properties"]["aiCompletion"]["properties"];
    for forbidden in
        ["enabled", "provider", "model", "endpoint", "apiKeyEnv", "apiKeyHeader", "apiKeyPrefix"]
    {
        assert!(ai.get(forbidden).is_none(), "generic schema restored {forbidden}");
    }

    assert!(runtime.contains("enum AiActivationAuthority"));
    assert!(runtime.contains("struct AuthorityBoundAiBackend"));
    assert!(runtime.contains("current != self.expected_authority"));
    assert!(runtime.contains("!effective_enabled"));
    assert!(runtime.contains("!authority.is_trusted()"));

    let ai_doc = fs::read_to_string(root.join("docs/reference/AI_COMPLETION.md"))?;
    assert!(ai_doc.contains("unavailable until a trusted user/operator adapter"));
    assert!(!ai_doc.contains("Set `aiCompletion.enabled` to `true`"));

    let extension_readme = fs::read_to_string(root.join("vscode-extension/README.md"))?;
    assert!(!extension_readme.contains("To enable it, set `perl-lsp.aiCompletion.enabled`"));
    Ok(())
}
