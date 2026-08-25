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
}
