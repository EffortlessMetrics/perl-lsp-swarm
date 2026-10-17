//! Controlled executable used to discriminate UX binary selection failures.
use std::io::Write;

fn main() -> std::io::Result<()> {
    if let Ok(packet) = std::env::var("UX_IDENTITY_PACKET") {
        writeln!(std::io::stdout().lock(), "{packet}")?;
    }
    Ok(())
}
