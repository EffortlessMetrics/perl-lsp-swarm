use color_eyre::eyre::{Result, bail};
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("--verify-receipt")) {
        bail!("usage: perl-rust-small-verifier --verify-receipt PATH");
    }
    let Some(path) = args.next() else {
        bail!("--verify-receipt requires PATH");
    };
    if args.next().is_some() {
        bail!("unexpected trailing arguments");
    }
    perl_rust_small_verifier::run(None, Some(path.into()))
}
