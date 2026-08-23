//! Compatibility example delegating to the canonical publication-drift classifier.

fn main() -> color_eyre::eyre::Result<()> {
    xtask::publication_drift::run_from_env()
}
