#![deny(clippy::map_err_ignore)] // Cohort C0 activation (#12598): census-clean on all targets; new findings move the crate to C1.
fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(perl_ripr_facts::run_cli(std::env::args()) as u8)
}
