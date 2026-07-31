//! Compatibility shim for the retired tracked goal-selector surface.
//!
//! Live programme selection now belongs to GitHub issues, PRs, dependencies,
//! and the provider-native `deliver-goal` flow. The old command remains
//! temporarily so existing scripts fail forward without restoring repository
//! authority to `.perl-lsp/goals/`.

use color_eyre::eyre::Result;

pub fn run() -> Result<()> {
    println!(
        "active goal manifest check retired: GitHub and deliver-goal own live work selection"
    );
    Ok(())
}
