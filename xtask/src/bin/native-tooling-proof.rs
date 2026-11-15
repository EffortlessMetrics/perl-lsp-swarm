//! Bounded proof target for the native critic and tooling task modules.
//!
//! Run their existing tests with:
//! `./scripts/cargo-admitted test -p xtask --bin native-tooling-proof`.

// Match the diagnostic-output allowance of the main xtask binary.
#![allow(clippy::print_stderr, clippy::print_stdout)]

#[cfg(test)]
#[path = "../tasks/git_context.rs"]
pub(crate) mod git_context;

#[cfg(test)]
#[path = "../tasks/native_critic.rs"]
mod native_critic;

#[cfg(test)]
#[path = "../tasks/native_tooling.rs"]
mod native_tooling;

// Preserve native_tooling's crate-relative Git helper path.
#[cfg(test)]
mod tasks {
    pub(crate) use crate::git_context;
}

fn main() -> color_eyre::Result<()> {
    color_eyre::eyre::bail!(
        "native-tooling-proof is a test-only target; run \
         ./scripts/cargo-admitted test -p xtask --bin native-tooling-proof"
    )
}
