#![expect(
    clippy::print_stderr,
    reason = "Integration-test diagnostic and skip output; tracing is not the harness logger."
)]
mod build_catalog {
    #![allow(dead_code)]
    include!("../build_catalog.rs");

    use perl_tdd_support::{must, must_err};

    #[test]
    fn missing_explicit_override_does_not_fall_back_to_workspace_catalog() {
        let root =
            std::env::temp_dir().join(format!("perl-dap-build-catalog-{}", std::process::id()));
        must(std::fs::create_dir_all(&root));
        let workspace_catalog = root.join("features.toml");
        let missing_override = root.join("missing-features.toml");
        must(std::fs::write(&workspace_catalog, "[feature]\n"));

        let result = resolve_catalog_source_with_override(&root, Some(missing_override.clone()));

        assert!(result.is_err(), "missing explicit override must be terminal");
        assert!(
            must_err(result).to_string().contains("FEATURES_TOML_OVERRIDE path does not exist")
        );
        assert!(workspace_catalog.exists(), "test setup must include a fallback catalog");
        assert!(!missing_override.exists(), "override must remain missing");
        must(std::fs::remove_dir_all(root));
    }

    #[test]
    fn generate_catalog_module_propagates_missing_explicit_override() {
        let root = must(tempfile::tempdir());
        let workspace_catalog = root.path().join("features.toml");
        let missing_override = root.path().join("missing-features.toml");
        let out_dir = root.path().join("out");
        must(std::fs::create_dir(&out_dir));
        must(std::fs::write(&workspace_catalog, "[feature]\n"));

        let result =
            generate_catalog_module_at(root.path(), &out_dir, Some(missing_override.clone()));

        let error = must_err(result);
        assert!(error.to_string().contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(
            !out_dir.join("dap_feature_catalog.rs").exists(),
            "source resolution failure must not emit fallback catalog"
        );
    }

    fn empty_advertised_dap_catalog() -> &'static str {
        "[meta]\nversion = 'test'\nlsp_version = '3.18'\n\n[[feature]]\nid = 'lsp.planned'\narea = 'text_document'\nadvertised = false\n"
    }

    fn advertised_dap_catalog() -> &'static str {
        "[meta]\nversion = 'test'\nlsp_version = '3.18'\n\n[[feature]]\nid = 'dap.core'\narea = 'debug'\nadvertised = true\n"
    }

    fn unpacked_dap(root: &std::path::Path, fallback: &str) {
        must(std::fs::create_dir_all(root));
        must(std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"perl-dap\"\nversion = \"0.0.0\"\n",
        ));
        must(std::fs::write(root.join("features_sot.toml"), fallback));
    }

    #[test]
    fn package_isolated_perl_dap_records_identity() {
        let root = must(tempfile::tempdir());
        unpacked_dap(root.path(), advertised_dap_catalog());
        let out_dir = root.path().join("out");
        must(std::fs::create_dir(&out_dir));

        must(generate_catalog_module_package_isolated(root.path(), &out_dir));
        let generated = must(std::fs::read_to_string(out_dir.join("dap_feature_catalog.rs")));
        assert!(generated.contains("catalog-source-kind: package-fallback"));
        assert!(generated.contains("catalog-source-digest: sha256:"));
        assert!(generated.contains("catalog-package: perl-dap"));
        assert!(generated.contains("\"dap.core\""));
    }

    #[test]
    fn malformed_package_fallback_does_not_emit_default_dap_features() {
        let root = must(tempfile::tempdir());
        unpacked_dap(root.path(), "not toml [[[");
        let out_dir = root.path().join("out");
        must(std::fs::create_dir(&out_dir));

        let error = must_err(generate_catalog_module_package_isolated(root.path(), &out_dir));
        assert!(error.to_string().contains("MALFORMED_FALLBACK"));
        assert!(
            !out_dir.join("dap_feature_catalog.rs").exists(),
            "malformed fallback must not emit DEFAULT_DAP_FEATURES"
        );
    }

    #[test]
    fn empty_package_fallback_does_not_satisfy_dap_package_proof() {
        let root = must(tempfile::tempdir());
        unpacked_dap(root.path(), empty_advertised_dap_catalog());
        let out_dir = root.path().join("out");
        must(std::fs::create_dir(&out_dir));

        let error = must_err(generate_catalog_module_package_isolated(root.path(), &out_dir));
        assert!(error.to_string().contains("EMPTY_FALLBACK"));
        assert!(!out_dir.join("dap_feature_catalog.rs").exists());
    }

    #[test]
    fn package_isolated_does_not_rediscover_workspace_or_use_override() {
        let root = must(tempfile::tempdir());
        let crate_dir = root.path().join("crates/perl-dap");
        must(std::fs::create_dir_all(&crate_dir));
        must(std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"perl-dap\"\nversion = \"0.0.0\"\n",
        ));
        must(std::fs::write(root.path().join("features.toml"), advertised_dap_catalog()));
        let missing = crate_dir.join("missing-override.toml");

        let error = must_err(resolve_catalog_source_package_isolated(&crate_dir, Some(missing)));
        assert!(error.to_string().contains("OVERRIDE_NOT_ALLOWED"));

        let error = must_err(resolve_catalog_source_package_isolated(&crate_dir, None));
        assert!(error.to_string().contains("MISSING_FALLBACK"));
    }
}
