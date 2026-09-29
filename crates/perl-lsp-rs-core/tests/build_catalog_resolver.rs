mod build_catalog {
    use perl_test_must::{must_err_with, must_with};

    include!("../build_catalog.rs");

    #[test]
    fn missing_explicit_override_is_terminal_even_with_workspace_catalog() {
        let root = must_with(tempfile::tempdir(), "create test catalog directory");
        let workspace_catalog = root.path().join("features.toml");
        let missing_override = root.path().join("missing-features.toml");
        must_with(
            std::fs::write(&workspace_catalog, "[meta]\nversion = 'test'\nlsp_version = 'test'\n"),
            "write fallback workspace catalog",
        );

        let error = must_err_with(
            resolve_catalog_source_with_override(root.path(), Some(missing_override.clone())),
            "missing explicit override must be terminal",
        );

        assert!(error.contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(workspace_catalog.exists());
        assert!(!missing_override.exists());
    }

    #[test]
    fn missing_override_emits_no_fallback_artifact() {
        let root = must_with(tempfile::tempdir(), "create test catalog directory");
        must_with(
            std::fs::write(
                root.path().join("features.toml"),
                "[meta]\nversion = 'test'\nlsp_version = 'test'\n",
            ),
            "write fallback workspace catalog",
        );
        let out_dir = root.path().join("out");
        must_with(std::fs::create_dir(&out_dir), "create test output directory");

        let error = must_err_with(
            generate_lsp_catalog_module_at(
                root.path(),
                &out_dir,
                Some(root.path().join("missing-features.toml")),
            ),
            "missing explicit override must fail the entrypoint",
        );

        assert!(error.contains("FEATURES_TOML_OVERRIDE path does not exist"));
        assert!(!out_dir.join("feature_contracts.rs").exists());
    }

    #[test]
    fn declared_compliance_percent_is_refused_before_generation() {
        let root = must_with(tempfile::tempdir(), "create test catalog directory");
        let catalog_path = root.path().join("features.toml");
        must_with(
            std::fs::write(
                &catalog_path,
                "[meta]\nversion = 'test'\nlsp_version = 'test'\ncompliance_percent = 98\n\n[[feature]]\nid = 'test'\nmaturity = 'planned'\n",
            ),
            "write catalog with refused aggregate",
        );

        let error =
            must_err_with(read_catalog(&catalog_path), "declaration aggregate must be refused");

        assert!(error.contains("meta.compliance_percent is refused"));
    }

    fn empty_advertised_catalog() -> &'static str {
        "[meta]\nversion = 'test'\nlsp_version = '3.18'\n\n[[feature]]\nid = 'lsp.planned'\nmaturity = 'planned'\nadvertised = false\narea = 'text_document'\n"
    }

    fn advertised_catalog() -> &'static str {
        "[meta]\nversion = 'test'\nlsp_version = '3.18'\n\n[[feature]]\nid = 'lsp.completion'\nmaturity = 'proven'\nadvertised = true\narea = 'text_document'\n"
    }

    fn unpacked_crate(root: &std::path::Path, package: &str, fallback: &str) {
        must_with(std::fs::create_dir_all(root), "create unpacked crate");
        must_with(
            std::fs::write(
                root.join("Cargo.toml"),
                format!("[package]\nname = \"{package}\"\nversion = \"0.0.0\"\n"),
            ),
            "write package manifest",
        );
        must_with(
            std::fs::write(root.join("features_sot.toml"), fallback),
            "write package fallback",
        );
    }

    #[test]
    fn package_fallback_generate_records_digest_and_package() {
        let root = must_with(tempfile::tempdir(), "create unpacked crate");
        unpacked_crate(root.path(), "perl-lsp-rs-core", advertised_catalog());
        let out_dir = root.path().join("out");
        must_with(std::fs::create_dir(&out_dir), "create out dir");

        let source = must_with(
            generate_lsp_catalog_module_package_isolated(root.path(), &out_dir),
            "valid package fallback must generate",
        );
        assert!(matches!(source.kind, CatalogSourceKind::Vendored));
        let generated = must_with(
            std::fs::read_to_string(out_dir.join("feature_contracts.rs")),
            "read generated",
        );
        assert!(generated.contains("catalog-source-kind: package-fallback"));
        assert!(generated.contains("catalog-source-digest: sha256:"));
        assert!(generated.contains("catalog-package: perl-lsp-rs-core"));
        assert!(generated.contains("catalog-projection: FullCatalog"));
    }

    #[test]
    fn empty_package_fallback_does_not_emit_module() {
        let root = must_with(tempfile::tempdir(), "create unpacked crate");
        unpacked_crate(root.path(), "perl-lsp-rs-core", empty_advertised_catalog());
        let out_dir = root.path().join("out");
        must_with(std::fs::create_dir(&out_dir), "create out dir");

        let error = must_err_with(
            generate_lsp_catalog_module_package_isolated(root.path(), &out_dir),
            "empty fallback must fail closed",
        );
        assert!(error.contains("EMPTY_FALLBACK"));
        assert!(!out_dir.join("feature_contracts.rs").exists());
    }

    #[test]
    fn malformed_package_fallback_does_not_emit_module() {
        let root = must_with(tempfile::tempdir(), "create unpacked crate");
        unpacked_crate(root.path(), "perl-lsp-rs-core", "not toml [[[");
        let out_dir = root.path().join("out");
        must_with(std::fs::create_dir(&out_dir), "create out dir");

        let error = must_err_with(
            generate_lsp_catalog_module_package_isolated(root.path(), &out_dir),
            "malformed fallback must fail closed",
        );
        assert!(error.contains("MALFORMED_FALLBACK"));
        assert!(!out_dir.join("feature_contracts.rs").exists());
    }
}
