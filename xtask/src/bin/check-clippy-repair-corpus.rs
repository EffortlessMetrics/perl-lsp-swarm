//! Command entry point for the Clippy repair-falsifier corpus validator.

fn main() -> color_eyre::eyre::Result<()> {
    xtask::clippy_repair_corpus::run_from_env()
}
