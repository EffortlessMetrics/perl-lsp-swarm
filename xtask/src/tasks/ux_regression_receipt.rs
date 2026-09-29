//! Compatibility entrypoint for existing `cargo xtask ux-regression-receipt` callers.

pub use perl_lsp_ux_tests::regression_receipt::UxRegressionReceiptConfig;

pub fn run(config: UxRegressionReceiptConfig) -> color_eyre::eyre::Result<()> {
    perl_lsp_ux_tests::regression_receipt::run(config)
        .map_err(|error| color_eyre::eyre::eyre!("{error:#}"))
}
