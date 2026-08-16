//! DAP dependency and package-surface tests (AC18).
//!
//! These checks enforce the product boundary: `perl-dap` ships the native
//! adapter, while `Perl::LanguageServer` may appear only in repository-only
//! conformance or historical material.
//!
//! Run with: `cargo test -p perl-dap --features dap-phase3 --test dap_dependency_tests`

#[cfg(feature = "dap-phase3")]
mod dap_dependencies {
    use anyhow::Result;
    use perl_lsp_rs_core::config::PerlOracleEnv;
    use std::path::PathBuf;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    fn read(path: impl AsRef<std::path::Path>) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    /// Strip doc-comment markers and rejoin hard-wrapped lines so prose
    /// assertions match on wording rather than on where a line happens to wrap.
    ///
    /// Both surfaces this guards are wrapped prose: `README.md` breaks after
    /// "or user" and `lib.rs` breaks after "compatibility", so a raw
    /// `contains` for the full sentence fragment fails even though the text
    /// says exactly what is required.
    fn prose(content: &str) -> String {
        content
            .lines()
            .map(|line| line.trim().trim_start_matches("//!").trim())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    // AC:18
    fn published_crate_rustdoc_is_native_only() -> Result<()> {
        let crate_root = read(repo_root().join("crates/perl-dap/src/lib.rs"))?;
        let crate_prose = prose(&crate_root);
        assert!(crate_prose.contains("Native Debug Adapter Protocol implementation for Perl"));
        assert!(crate_prose.contains("The supported first-mile runtime is the native"));
        assert!(crate_prose.contains("not a runtime backend or published compatibility feature"));

        for forbidden in [
            "pub mod bridge_adapter",
            "pub use bridge_adapter",
            "BridgeAdapter",
            "DapBridgeEnvConfig",
            "spawn_pls_dap",
            "legacy-pls-bridge",
            "Native and Bridge Modes",
        ] {
            assert!(
                !crate_root.contains(forbidden),
                "published crate root contains removed PLS surface {forbidden:?}"
            );
        }
        Ok(())
    }

    #[test]
    // AC:18
    fn cargo_feature_graph_contains_no_pls_runtime_feature() -> Result<()> {
        let dap_manifest = read(repo_root().join("crates/perl-dap/Cargo.toml"))?;
        assert!(dap_manifest.contains("default = [\"dap-phase2\", \"dap-phase3\"]"));
        assert!(dap_manifest.contains("features = [\"dap-phase2\", \"dap-phase3\"]"));
        assert!(!dap_manifest.contains("legacy-pls-bridge"));
        assert!(!dap_manifest.contains("dap-phase1"));
        assert!(!dap_manifest.contains("bridge_adapter_unit_tests"));
        assert!(!dap_manifest.contains("bridge_integration_tests"));

        let lsp_manifest = read(repo_root().join("crates/perl-lsp-rs/Cargo.toml"))?;
        assert!(!lsp_manifest.contains("legacy-pls-bridge"));
        assert!(!lsp_manifest.contains("dap-phase1"));
        assert!(lsp_manifest.contains("dap-phase3 = []"));

        let release_workflow = read(repo_root().join(".github/workflows/release.yml"))?;
        assert!(release_workflow.contains("-p perl-dap --bin perl-dap"));
        assert!(!release_workflow.contains("-p perl-dap --all-features"));
        Ok(())
    }

    #[test]
    // AC:18
    fn production_source_contains_no_pls_proxy_or_mode() -> Result<()> {
        let dap_src = repo_root().join("crates/perl-dap/src");
        assert!(!dap_src.join("bridge_adapter.rs").exists());

        for path in [
            "crates/perl-dap/src/lib.rs",
            "crates/perl-dap/src/server/mode.rs",
            "crates/perl-dap/src/server/lifecycle.rs",
            "crates/perl-lsp-rs/src/lib.rs",
        ] {
            let content = read(repo_root().join(path))?;
            for forbidden in [
                "BridgeAdapter",
                "DapBridgeEnvConfig",
                "DapMode::Bridge",
                "spawn_pls_dap",
                "legacy-pls-bridge",
                "dap-phase1",
            ] {
                assert!(
                    !content.contains(forbidden),
                    "production source {path} contains removed PLS surface {forbidden:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    // AC:18
    fn product_shaped_pls_bridge_tests_are_removed() {
        let root = repo_root();
        for path in [
            "crates/perl-dap/tests/bridge_adapter_unit_tests.rs",
            "crates/perl-dap/tests/bridge_integration_tests.rs",
            "crates/perl-lsp-rs/tests/dap_bridge_tests.rs",
        ] {
            assert!(!root.join(path).exists(), "removed PLS runtime test still exists: {path}");
        }
    }

    #[test]
    // AC:18
    fn crate_readme_describes_native_runtime_and_repo_only_conformance() -> Result<()> {
        let readme = read(repo_root().join("crates/perl-dap/README.md"))?;
        let readme_prose = prose(&readme);
        assert!(readme_prose.contains("Native launch"));
        assert!(
            readme_prose.contains("not a runtime backend, package feature, or user prerequisite")
        );
        assert!(readme_prose.contains("repository-only conformance lanes"));
        assert!(!readme.contains("cpanm Perl::LanguageServer"));
        assert!(!readme.contains("--bridge"));
        Ok(())
    }

    #[test]
    // AC:18
    fn first_mile_dap_docs_do_not_require_pls() -> Result<()> {
        for path in [
            "docs/tutorials/DAP_USER_GUIDE.md",
            "docs/tutorials/DAP_BRIDGE_SETUP_GUIDE.md",
            "book/src/dap/user-guide.md",
            "book/src/dap/bridge-setup.md",
        ] {
            let content = read(repo_root().join(path))?;
            assert!(!content.contains("cpanm Perl::LanguageServer"), "PLS install in {path}");
            assert!(!content.contains("cpan Perl::LanguageServer"), "PLS install in {path}");
            assert!(!content.contains("--bridge"), "removed bridge CLI in {path}");
        }

        let user_guide = read(repo_root().join("docs/tutorials/DAP_USER_GUIDE.md"))?;
        assert!(user_guide.contains("Native `perl-dap`"));
        assert!(user_guide.contains("local Perl interpreter"));
        Ok(())
    }

    #[test]
    // AC:18
    fn perl_version_compatibility() -> Result<()> {
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
}
