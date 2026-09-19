//! Command entry point for the read-only Open VSX public-state probe (#9923).

fn main() -> color_eyre::eyre::Result<()> {
    xtask::open_vsx_public_state::run_from_env()
}
