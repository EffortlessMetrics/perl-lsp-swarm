//! Command entry point for the canonical authority-bound publication-drift classifier.

fn main() -> color_eyre::eyre::Result<()> {
    xtask::publication_drift::run_from_env()
}
