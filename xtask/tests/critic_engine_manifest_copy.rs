//! Product-surface regression for the native-only critic runtime (#17985).
//!
//! The generic native-product prose guard checks whether external-tool names are
//! qualified. Qualification alone is not enough: user-facing copy must not tell
//! users that `.perl-lsp.toml` selects an external runtime engine when the
//! effective product state is native or disabled. This test binds the contributed
//! VS Code setting to that narrower semantic contract.

use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

type TestResult<T = ()> = Result<T, Box<dyn Error>>;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".."))
}

fn critic_engine_setting(manifest: &Value) -> TestResult<&Value> {
    let configurations = manifest
        .pointer("/contributes/configuration")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("manifest contributes.configuration must be an array"))?;

    configurations
        .iter()
        .find_map(|configuration| {
            configuration.pointer("/properties/perl-lsp.critic.engine")
        })
        .ok_or_else(|| io::Error::other("manifest must define perl-lsp.critic.engine").into())
}

#[test]
fn critic_engine_copy_does_not_offer_external_runtime_selection() -> TestResult {
    let manifest_path = repo_root().join("vscode-extension/package.json");
    let manifest: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    let setting = critic_engine_setting(&manifest)?;

    let variants = setting
        .get("enum")
        .and_then(Value::as_array)
        .ok_or_else(|| io::Error::other("critic engine enum must be an array"))?;
    assert_eq!(variants.len(), 1, "critic engine must expose exactly one runtime choice");
    assert_eq!(variants[0].as_str(), Some("native"));

    for field in ["description", "markdownDescription"] {
        let copy = setting
            .get(field)
            .and_then(Value::as_str)
            .ok_or_else(|| io::Error::other(format!("critic engine {field} must be a string")))?;
        let normalized = copy.replace('`', "").to_ascii_lowercase();

        assert!(normalized.contains("native"), "{field} must name the native runtime: {copy}");
        assert!(normalized.contains("external"), "{field} must bound external aliases: {copy}");
        assert!(normalized.contains("perlcritic"), "{field} must identify the retired external engine: {copy}");
        assert!(
            normalized.contains("do not select")
                || normalized.contains("does not select")
                || normalized.contains("cannot select"),
            "{field} must state that legacy aliases cannot select a product runtime engine: {copy}"
        );

        for forbidden in [
            "select the optional external",
            "select the external",
            "select an external",
            "switch to the external",
            "switch to an external",
        ] {
            assert!(
                !normalized.contains(forbidden),
                "{field} must not advertise external runtime selection via `{forbidden}`: {copy}"
            );
        }
    }

    Ok(())
}
