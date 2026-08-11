//! Compatibility entry point for the canonical publication-drift classifier.

#[path = "../src/bin/publication-drift.rs"]
mod classifier;

fn main() -> color_eyre::eyre::Result<()> {
    classifier::run()
}
