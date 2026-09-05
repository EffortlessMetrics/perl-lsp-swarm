mod build_catalog {
    use perl_test_must::{must_err_with, must_with};

    include!("../build_catalog.rs");

    // build_catalog.rs is one shared source included into several compilation
    // units; this target exercises only the resolver and generation subset.
    // Rather than a module-wide `dead_code` allowance, name the exact items
    // owned by the other consumers (build.rs and perl-dap) so that any NEW
    // unused item in the shared source still fails this target's lint.
    #[allow(clippy::items_after_test_module)]
    const _OWNED_BY_OTHER_CONSUMERS: () = {
        // Env-dependent resolver entrypoints, driven by build.rs.
        const _: fn(&Path) -> Result<CatalogSource, String> = resolve_catalog_source;
        const _: fn(&Path) -> Result<(Catalog, CatalogSource), String> = load_catalog_for_build;
        // Grid/compliance counters, consumed by the BDD coverage grids.
        const _: fn(&Catalog) -> usize = Catalog::trackable_feature_count_for_grid;
        const _: fn(&Catalog) -> usize = Catalog::advertised_trackable_count_for_grid;
        // The three below are deprecated compatibility shims kept for the
        // perl-dap consumer; naming them keeps dead_code honest for the rest,
        // and the allowance is scoped to this reference block alone.
        #[allow(deprecated)]
        const _DEPRECATED_COMPAT_SHIMS: () = {
            const _: fn(&Catalog) -> usize = Catalog::trackable_feature_count;
            const _: fn(&Catalog) -> usize = Catalog::advertised_trackable_count;
            const _: fn(&Catalog) -> f32 = Catalog::compliance_percent;
        };
    };

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
}
