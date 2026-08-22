mod build_script {
    include!("../build.rs");

    #[test]
    fn missing_explicit_override_does_not_fall_back_to_workspace_catalog() {
        let root =
            std::env::temp_dir().join(format!("perl-dap-build-catalog-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create test catalog directory");
        let workspace_catalog = root.join("features.toml");
        let missing_override = root.join("missing-features.toml");
        std::fs::write(&workspace_catalog, "[feature]\n")
            .expect("write fallback workspace catalog");

        let result =
            super::resolve_catalog_source_with_override(&root, Some(missing_override.clone()));

        assert!(result.is_err(), "missing explicit override must be terminal");
        assert!(
            result
                .expect_err("missing explicit override must be terminal")
                .to_string()
                .contains("FEATURES_TOML_OVERRIDE path does not exist")
        );
        assert!(workspace_catalog.exists(), "test setup must include a fallback catalog");
        assert!(!missing_override.exists(), "override must remain missing");
        std::fs::remove_dir_all(root).expect("remove test catalog directory");
    }
}
