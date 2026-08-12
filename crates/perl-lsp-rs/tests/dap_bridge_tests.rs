//! Legacy PLS bridge conformance tests.
//!
//! These tests run only when the explicit `dap-phase1` compatibility alias is
//! selected. They preserve historical integration coverage without making the
//! bridge part of the default LSP or DAP product surface.
//!
//! Run with: `cargo test -p perl-lsp-rs --features dap-phase1 --test dap_bridge_tests`

#![allow(deprecated)]

#[cfg(feature = "dap-phase1")]
mod legacy_pls_bridge_conformance {
    use anyhow::Result;

    /// The VS Code extension continues to register the native Perl debugger surface.
    #[test]
    // AC:1
    fn test_vscode_debugger_contribution() -> Result<()> {
        use serde_json::Value;

        let path = "../../vscode-extension/package.json";
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read package.json at {path}: {e}"))?;
        let json: Value = serde_json::from_str(&content)?;

        let debuggers = json
            .pointer("/contributes/debuggers")
            .ok_or_else(|| anyhow::anyhow!("Missing contributes.debuggers"))?
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("contributes.debuggers is not an array"))?;
        let perl_debugger = debuggers
            .iter()
            .find(|d| d["type"] == "perl")
            .ok_or_else(|| anyhow::anyhow!("Missing type: 'perl' debugger"))?;

        let launch = perl_debugger
            .pointer("/configurationAttributes/launch/properties")
            .ok_or_else(|| anyhow::anyhow!("Missing launch properties"))?;
        assert!(launch.get("program").is_some(), "Missing program attribute");
        assert!(launch.get("args").is_some(), "Missing args attribute");
        assert!(launch.get("perlPath").is_some(), "Missing perlPath attribute");
        assert!(launch.get("includePaths").is_some(), "Missing includePaths attribute");
        Ok(())
    }

    /// Launch and attach snippets remain available independently of the legacy backend.
    #[test]
    // AC:2
    fn test_launch_configuration_snippets() -> Result<()> {
        use serde_json::Value;

        let path = "../../vscode-extension/snippets/launch.json";
        let content = std::fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!("Failed to read launch.json snippet at {path}: {e}")
        })?;
        let json: Value = serde_json::from_str(&content)?;

        let launch_snippet = json
            .get("Perl: Launch Script")
            .ok_or_else(|| anyhow::anyhow!("Missing 'Perl: Launch Script' snippet"))?;
        assert!(launch_snippet["prefix"] == "perl-launch", "Wrong prefix for launch");

        let attach_snippet = json
            .get("Perl: Attach to Process")
            .ok_or_else(|| anyhow::anyhow!("Missing 'Perl: Attach to Process' snippet"))?;
        assert!(attach_snippet["prefix"] == "perl-attach", "Wrong prefix for attach");

        let pkg_path = "../../vscode-extension/package.json";
        let pkg_content = std::fs::read_to_string(pkg_path)?;
        let pkg_json: Value = serde_json::from_str(&pkg_content)?;
        let snippets = pkg_json
            .pointer("/contributes/snippets")
            .ok_or_else(|| anyhow::anyhow!("Missing snippets contribution"))?
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("snippets is not an array"))?;
        let has_json = snippets.iter().any(|s| {
            s["language"] == "json" && s["path"].as_str().unwrap_or("").contains("launch.json")
        });
        assert!(has_json, "Missing launch.json snippet registration for json language");
        Ok(())
    }

    /// Legacy installation and migration details remain quarantined in reference docs.
    #[test]
    // AC:3
    fn test_legacy_bridge_reference_is_complete() -> Result<()> {
        let path = "../../docs/reference/DAP_LEGACY_BRIDGE_COMPAT.md";
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read legacy bridge reference at {path}: {e}"))?;

        assert!(content.contains("cpan Perl::LanguageServer"));
        assert!(content.contains("cpanm Perl::LanguageServer"));
        assert!(content.contains("BridgeAdapter"));
        assert!(content.contains("--bridge"));
        assert!(content.contains("launch.json"));
        assert!(content.contains("Troubleshooting"));
        Ok(())
    }

    /// The explicit compatibility feature still exposes BridgeAdapter for conformance work.
    #[test]
    // AC:4
    fn test_legacy_bridge_adapter_can_be_instantiated() {
        use perl_lsp::BridgeAdapter;

        let _adapter = BridgeAdapter::new();
    }

    /// Native platform normalization remains available in compatibility builds.
    #[test]
    // AC:4/AC:7
    fn test_cross_platform_path_mapping() {
        use perl_dap::platform::normalize_path;
        use std::path::PathBuf;

        let unix_path = PathBuf::from("/usr/bin/perl");
        let norm_unix = normalize_path(&unix_path);
        assert!(!norm_unix.to_string_lossy().is_empty());

        #[cfg(target_os = "linux")]
        {
            let wsl_path = PathBuf::from("/mnt/c/Users/test.pl");
            let norm_wsl = normalize_path(&wsl_path);
            assert!(norm_wsl.to_string_lossy().starts_with("C:"));
        }
    }
}
