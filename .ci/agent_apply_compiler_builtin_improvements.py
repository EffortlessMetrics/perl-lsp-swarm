#!/usr/bin/env python3
"""Apply the reviewed builtin-catalog improvements on the disposable CI branch."""

from pathlib import Path
from textwrap import dedent


PATH = "xtask/src/bin/compiler-builtin-catalog.rs"


def main() -> None:
    text = Path(PATH).read_text()
    marker = "Review fix: executable EIR requires an implemented lowering seam."
    if marker not in text:
        anchor = text.index(
            "        if (self.hir_pir_lowering != HirPirLowering::CatalogOnly"
        )
        addition = dedent(
            """
            // Review fix: executable EIR requires an implemented lowering seam.
            if self.eir_profile == EirProfile::Executable
                && !matches!(
                    self.hir_pir_lowering,
                    HirPirLowering::ClassifiedCall | HirPirLowering::DedicatedOperation
                )
            {
                bail!(
                    "builtin {} cannot be executable without classified or dedicated HIR/PIR lowering",
                    self.builtin_id
                );
            }
            """
        )
        text = text[:anchor] + addition + text[anchor:]

    test_name = "fn executable_eir_rejects_catalog_only_lowering"
    if test_name not in text:
        anchor = text.index(
            "    #[test]\n    fn rendering_is_independent_of_row_order()"
        )
        test = dedent(
            """
            #[test]
            fn executable_eir_rejects_catalog_only_lowering() -> Result<()> {
                let mut catalog = BuiltinCatalog::from_str(CATALOG)?;
                let builtin = catalog
                    .builtins
                    .first_mut()
                    .ok_or_else(|| anyhow!("committed catalog unexpectedly empty"))?;
                builtin.proof = ProofState::Proven;
                builtin.evidence.push("implementation:fixture".to_string());
                builtin.eir_profile = EirProfile::Executable;
                builtin.hir_pir_lowering = HirPirLowering::CatalogOnly;
                let error = catalog
                    .validate()
                    .expect_err("catalog-only lowering must not authorize executable EIR")
                    .to_string();
                assert!(error.contains("classified or dedicated"));
                Ok(())
            }

            """
        )
        text = text[:anchor] + test + text[anchor:]

    Path(PATH).write_text(text)
    Path("xtask/tests/compiler_builtin_catalog_cli.rs").write_text(
        dedent(
            '''\
            //! End-to-end check for the committed builtin semantic catalog and status.

            use assert_cmd::Command;
            use color_eyre::eyre::{Result, eyre};
            use std::path::Path;

            #[test]
            fn committed_builtin_status_is_current_through_cli() -> Result<()> {
                let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .ok_or_else(|| eyre!("xtask manifest must have a repository parent"))?;
                Command::cargo_bin("compiler-builtin-catalog")?
                    .current_dir(repo_root)
                    .arg("--check")
                    .assert()
                    .success();
                Ok(())
            }
            '''
        )
    )


if __name__ == "__main__":
    main()
