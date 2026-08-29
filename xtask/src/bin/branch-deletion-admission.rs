//! Command entry point for typed branch-deletion admission (#12885).

fn main() -> color_eyre::eyre::Result<()> {
    xtask::branch_deletion_admission::run_from_env()
}
