mod build_catalog {
    include!("../build_catalog.rs");

    #[test]
    fn missing_explicit_override_is_terminal_even_with_workspace_catalog() {
        let root = tempfile::tempdir().expect("create test catalog directory");
        let workspace_catalog = root.path().join("features.toml");
        let missing_override = root.path().join("missing-features.toml");
        std::fs::write(&workspace_catalog, "[meta]\nversion = 'test'\nlsp_version = 'test'\n")
            .expect("write fallback workspace catalog");

        let error =
            resolve_catalog_source_with_override(root.path(), Some(missing_override.clone()))
                .expect_err("missing explicit override must be terminal");

        assert!(error.contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(workspace_catalog.exists());
        assert!(!missing_override.exists());
    }

    #[test]
    fn missing_override_emits_no_fallback_artifact() {
        let root = tempfile::tempdir().expect("create test catalog directory");
        std::fs::write(
            root.path().join("features.toml"),
            "[meta]\nversion = 'test'\nlsp_version = 'test'\n",
        )
        .expect("write fallback workspace catalog");
        let out_dir = root.path().join("out");
        std::fs::create_dir(&out_dir).expect("create test output directory");

        let error = generate_lsp_catalog_module_at(
            root.path(),
            &out_dir,
            Some(root.path().join("missing-features.toml")),
        )
        .expect_err("missing explicit override must fail the entrypoint");

        assert!(error.contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(!out_dir.join("feature_contracts.rs").exists());
    }

    #[test]
    fn declared_compliance_percent_is_refused_before_generation() {
        let root = tempfile::tempdir().expect("create test catalog directory");
        let catalog_path = root.path().join("features.toml");
        std::fs::write(
            &catalog_path,
            "[meta]\nversion = 'test'\nlsp_version = 'test'\ncompliance_percent = 98\n\n[[feature]]\nid = 'test'\nmaturity = 'planned'\n",
        )
        .expect("write catalog with refused aggregate");

        let error = read_catalog(&catalog_path).expect_err("declaration aggregate must be refused");

        assert!(error.contains("meta.compliance_percent is refused"));
    }
}
