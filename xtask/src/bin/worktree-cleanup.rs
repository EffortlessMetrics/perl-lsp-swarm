//! Command entry point for typed worktree cleanup inspection and application.

fn main() -> color_eyre::eyre::Result<()> {
    xtask::worktree_cleanup::run_from_env()
}
