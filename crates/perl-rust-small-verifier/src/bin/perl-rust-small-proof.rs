//! Governed producer entrypoint; semantic proof remains owned by xtask.
use color_eyre::eyre::{Result, bail};

fn main() -> Result<()> {
    if std::env::args_os().nth(1).is_some() {
        bail!("usage: perl-rust-small-proof (no arguments)");
    }
    perl_rust_small_verifier::run(None, None)
}
