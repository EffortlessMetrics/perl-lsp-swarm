//! Controlled executable used to discriminate UX binary selection failures.
fn main() {
    if let Ok(packet) = std::env::var("UX_IDENTITY_PACKET") {
        println!("{packet}");
    }
}
