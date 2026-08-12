//! DAP Dependency Management Tests (AC18)
//!
//! Repository-backed checks for dependency expectations and fallback assets.
//!
//! Run with: `cargo test -p perl-dap --features dap-phase3`

#[cfg(feature = "dap-phase3")]
mod dap_dependencies {
    use anyhow::Result;
    use perl_lsp_rs_core::config::PerlOracleEnv;
    use serde_json::Value;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn read(path: impl AsRef<std::path::Path>) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    /// Public crate docs must describe the native runtime rather than the legacy bridge.
    #[test]
    // AC:18
    fn test_published_crate_rustdoc_is_native_first() -> Result<()> {
        let crate_root = read(repo_root().join("crates/perl-dap/src/lib.rs"))?;
        assert!(crate_root.contains("Native Debug Adapter Protocol implementation for Perl"));
        assert!(crate_root.contains("The supported first-mile runtime is the native"));
        assert!(crate_root.contains("default-off library compatibility"));
        assert!(!crate_root.contains("**Legacy Bridge Mode**"));
        assert!(!crate_root.contains("## Native and Bridge Modes"));
        assert!(!crate_root.contains("Users can start with bridge mode today"));
        assert!(!crate_root.contains("adapter.spawn_pls_dap"));
        Ok(())
    }

    /// The default library and release build must not activate the PLS bridge.
    #[test]
    // AC:18
    fn test_legacy_pls_bridge_is_explicit_and_default_off() -> Result<()> {
        let dap_manifest = read(repo_root().join("crates/perl-dap/Cargo.toml"))?;
        assert!(dap_manifest.contains("default = [\"dap-phase2\", \"dap-phase3\"]"));
        assert!(dap_manifest.contains("legacy-pls-bridge = []"));
        assert!(dap_manifest.contains("dap-phase1 = [\"legacy-pls-bridge\"]"));
        assert!(dap_manifest.contains("required-features = [\"legacy-pls-bridge\"]"));
        assert!(dap_manifest.contains("no-default-features = true"));
        assert!(dap_manifest.contains("features = [\"dap-phase2\", \"dap-phase3\"]"));
        assert!(!dap_manifest.contains("all-features = true"));

        let lsp_manifest = read(repo_root().join("crates/perl-lsp-rs/Cargo.toml"))?;
        assert!(lsp_manifest.contains("dap-phase1 = [\"perl-dap/legacy-pls-bridge\"]"));
        assert!(lsp_manifest.contains("dap-phase3 = []"));

        let release_workflow = read(repo_root().join(".github/workflows/release.yml"))?;
        assert!(release_workflow.contains("-p perl-dap --bin perl-dap"));
        assert!(!release_workflow.contains("-p perl-dap --all-features"));
        Ok(())
    }

    /// Public crate landing-page docs must not require legacy bridge dependencies.
    #[test]
    // AC:18
    fn test_crate_readme_is_native_first() -> Result<()> {
        let readme = read(repo_root().join("crates/perl-dap/README.md"))?;
        assert!(!readme.contains("Perl::LanguageServer"));
        assert!(!readme.contains("BridgeAdapter"));
        assert!(!readme.contains("cpanm Perl::LanguageServer"));
        assert!(readme.contains("Native launch"));
        assert!(readme.contains("Legacy compatibility"));
        Ok(())
    }

    /// Tests feature spec: DAP_IMPLEMENTATION_SPECIFICATION.md#ac18-version-detection
    #[test]
    // AC:18
    fn test_devel_tsperldap_version_detection() -> Result<()> {
        let fixture =
            repo_root().join("crates/perl-dap/tests/fixtures/mocks/perl_shim_responses.json");
        let json: Value = serde_json::from_str(&read(fixture)?)?;
        let description = json
            .get("description")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing fixture description"))?;
        assert!(description.contains("Devel::TSPerlDAP"));
        assert!(json.get("set_breakpoints").is_some());
        assert!(json.get("stack_trace").is_some());
        assert!(json.get("scopes").is_some());
        Ok(())
    }

    /// Tests feature spec: DAP_IMPLEMENTATION_SPECIFICATION.md#ac18-bundled-shim
    #[test]
    // AC:18
    fn test_bundled_shim_fallback() -> Result<()> {
        let architecture_doc = read(repo_root().join("docs/reference/CRATE_ARCHITECTURE_DAP.md"))?;
        assert!(architecture_doc.contains("perl-shim"));
        assert!(architecture_doc.contains("TSPerlDAP.pm"));

        let fixture_index =
            read(repo_root().join("crates/perl-dap/tests/fixtures/FIXTURE_INDEX.md"))?;
        assert!(fixture_index.contains("perl_shim_responses.json"));
        Ok(())
    }

    /// First-mile DAP docs must describe the native `perl-dap` path only.
    #[test]
    // AC:18
    fn test_native_dap_user_docs_do_not_require_bridge_or_pls() -> Result<()> {
        let user_guide = read(repo_root().join("docs/tutorials/DAP_USER_GUIDE.md"))?;
        assert!(!user_guide.contains("Perl::LanguageServer"));
        assert!(!user_guide.contains("BridgeAdapter"));
        assert!(!user_guide.contains("cpanm Perl::LanguageServer"));
        assert!(user_guide.contains("Native `perl-dap`"));
        assert!(user_guide.contains("local Perl interpreter"));

        let bridge_pointer = read(repo_root().join("docs/tutorials/DAP_BRIDGE_SETUP_GUIDE.md"))?;
        assert!(!bridge_pointer.contains("Perl::LanguageServer"));
        assert!(!bridge_pointer.contains("BridgeAdapter"));
        assert!(!bridge_pointer.contains("cpanm Perl::LanguageServer"));

        let book_page = read(repo_root().join("book/src/dap/user-guide.md"))?;
        assert!(!book_page.contains("Perl::LanguageServer"));
        assert!(!book_page.contains("BridgeAdapter"));
        assert!(!book_page.contains("0.9.x"));

        let book_bridge_page = read(repo_root().join("book/src/dap/bridge-setup.md"))?;
        assert!(!book_bridge_page.contains("Perl::LanguageServer"));
        assert!(!book_bridge_page.contains("BridgeAdapter"));
        assert!(!book_bridge_page.contains("cpanm Perl::LanguageServer"));
        Ok(())
    }

    /// Tests feature spec: DAP_IMPLEMENTATION_SPECIFICATION.md#ac18-perl-version
    #[test]
    // AC:18
    fn test_perl_version_compatibility() -> Result<()> {
        let perl_version = PerlOracleEnv::for_dap_test_fixture().and_then(|oracle| {
            let mut cmd = oracle.into_command();
            cmd.arg("-e").arg("print $];");
            cmd.output().ok()
        });

        match perl_version {
            Some(output) if output.status.success() => {
                let version_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let version_num: f64 = version_str.parse().map_err(|e| {
                    anyhow::anyhow!("failed to parse perl version '{version_str}': {e}")
                })?;
                assert!(version_num >= 5.010, "Perl version must be >= 5.010, got {version_num}");
            }
            _ => {
                let guide = read(repo_root().join("docs/tutorials/DAP_USER_GUIDE.md"))?;
                assert!(guide.contains("Perl 5.10 or higher"));
            }
        }

        Ok(())
    }

    /// Legacy bridge dependency details belong in reference documentation only.
    #[test]
    // AC:18
    fn test_legacy_bridge_reference_documents_pls_dependency() -> Result<()> {
        let guide = read(repo_root().join("docs/reference/DAP_LEGACY_BRIDGE_COMPAT.md"))?;
        assert!(guide.contains("cpan Perl::LanguageServer"));
        assert!(guide.contains("cpanm Perl::LanguageServer"));
        assert!(guide.contains("Perl::LanguageServer not found"));
        assert!(guide.contains("--bridge"));
        assert!(guide.contains("BridgeAdapter"));
        assert!(guide.contains("launch.json"));
        Ok(())
    }

    /// Legacy bridge library symbols must advertise their transition status.
    #[test]
    // AC:18
    fn test_legacy_bridge_api_is_explicitly_deprecated() -> Result<()> {
        let crate_root = read(repo_root().join("crates/perl-dap/src/lib.rs"))?;
        assert!(crate_root.contains("#[cfg(feature = \"legacy-pls-bridge\")]"));
        assert!(crate_root.contains(
            "legacy Perl::LanguageServer compatibility; use the native DapServer/DebugAdapter path"
        ));
        assert!(crate_root.contains("pub mod bridge_adapter;"));
        assert!(crate_root.contains("pub use bridge_adapter::{BridgeAdapter, DapBridgeEnvConfig};"));
        assert!(crate_root.contains("#[doc(hidden)]"));

        let mode_module = read(repo_root().join("crates/perl-dap/src/server/mode.rs"))?;
        assert!(mode_module.contains(
            "legacy Perl::LanguageServer compatibility; use DapMode::Native instead"
        ));
        assert!(mode_module.contains("#[doc(hidden)]"));

        let lifecycle = read(repo_root().join("crates/perl-dap/src/server/lifecycle.rs"))?;
        assert!(lifecycle.contains("#![allow(deprecated)]"));
        assert!(lifecycle.contains("legacy Perl::LanguageServer bridge support is not enabled"));
        Ok(())
    }
}