mod build_catalog {
    // Mirrors the scoped allowance in build.rs: build_catalog.rs is one shared
    // source included into several compilation units, and this target exercises
    // only the resolver and generation subset (grid/deprecated helpers belong to
    // the build.rs and perl-dap consumers).
    #![allow(dead_code)]

    include!("../build_catalog.rs");

    use perl_tdd_support::{must, must_err};

    #[test]
    fn missing_explicit_override_is_terminal_even_with_workspace_catalog() {
        let root = must(tempfile::tempdir());
        let workspace_catalog = root.path().join("features.toml");
        let missing_override = root.path().join("missing-features.toml");
        must(std::fs::write(
            &workspace_catalog,
            "[meta]\nversion = 'test'\nlsp_version = 'test'\n",
        ));

        let error = must_err(resolve_catalog_source_with_override(
            root.path(),
            Some(missing_override.clone()),
        ));

        assert!(error.contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(workspace_catalog.exists());
        assert!(!missing_override.exists());
    }

    #[test]
    fn missing_override_emits_no_fallback_artifact() {
        let root = must(tempfile::tempdir());
        must(std::fs::write(
            root.path().join("features.toml"),
            "[meta]\nversion = 'test'\nlsp_version = 'test'\n",
        ));
        let out_dir = root.path().join("out");
        must(std::fs::create_dir(&out_dir));

        let error = must_err(generate_lsp_catalog_module_at(
            root.path(),
            &out_dir,
            Some(root.path().join("missing-features.toml")),
        ));

        assert!(error.contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(!out_dir.join("feature_contracts.rs").exists());
    }

    #[test]
    fn declared_compliance_percent_is_refused_before_generation() {
        let root = must(tempfile::tempdir());
        let catalog_path = root.path().join("features.toml");
        must(std::fs::write(
            &catalog_path,
            "[meta]\nversion = 'test'\nlsp_version = 'test'\ncompliance_percent = 98\n\n[[feature]]\nid = 'test'\nmaturity = 'planned'\n",
        ));

        let error = must_err(read_catalog(&catalog_path));

        assert!(error.contains("meta.compliance_percent is refused"));
    }
}
