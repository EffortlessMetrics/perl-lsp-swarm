//! Compatibility shims for the retired repository hook surface.
//!
//! Project-level Claude and Codex hooks were removed because runtime lifecycle,
//! command policy, formatting, proof, and merge currentness belong to provider
//! settings, repository skills, GitHub protection, and coherent candidate gates.
//! The public xtask commands remain temporarily so older `just` recipes and
//! external callers fail forward without reintroducing hook authority.

use color_eyre::eyre::Result;

fn retired(command: &str) {
    println!(
        "{command}: repository agent hooks are retired; no project hook validation is required"
    );
}

pub fn run_hook_check() -> Result<()> {
    retired("hook-check");
    Ok(())
}

pub fn run_hook_registry_check() -> Result<()> {
    retired("hook-registry-check");
    Ok(())
}

pub fn run_hook_tests() -> Result<()> {
    retired("hook-tests");
    Ok(())
}
