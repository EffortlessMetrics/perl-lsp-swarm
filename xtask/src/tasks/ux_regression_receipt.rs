//! Compatibility entrypoint for existing `cargo xtask ux-regression-receipt` callers.

pub use perl_lsp_ux_tests::regression_receipt::UxRegressionReceiptConfig;

pub fn run(config: UxRegressionReceiptConfig) -> color_eyre::eyre::Result<()> {
    let output = perl_lsp_ux_tests::regression_receipt::run(config)
        .map_err(|error| color_eyre::eyre::eyre!("{error:#}"))?;
    // Printing lives with the CLI caller: the library returns its output
    // instead (#16906). These two lines are the receipt chatter callers see.
    match &output.written_path {
        Some(path) => println!("Wrote UX regression receipt: {}", path.display()),
        None => println!("{}", output.payload),
    }
    Ok(())
}
