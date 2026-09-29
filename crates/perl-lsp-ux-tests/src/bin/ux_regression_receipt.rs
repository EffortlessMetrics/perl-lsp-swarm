//! Small receipt emitter for the UX gate after the harness has already built.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Result, bail};
use perl_lsp_ux_tests::regression_receipt::{UxRegressionReceiptConfig, run};

fn main() -> Result<()> {
    let mut input = None;
    let mut receipt = None;
    let mut sha = None;
    let mut exit_status_file = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let Some(value) = args.next() else {
            bail!("missing value for {flag}");
        };
        match flag.as_str() {
            "--input" => input = Some(PathBuf::from(value)),
            "--receipt" => receipt = Some(PathBuf::from(value)),
            "--sha" => sha = Some(value),
            "--exit-status-file" => exit_status_file = Some(PathBuf::from(value)),
            _ => bail!("unknown option {flag}"),
        }
    }
    let Some(input) = input else { bail!("--input is required") };
    let Some(sha) = sha else { bail!("--sha is required") };
    let Some(exit_status_file) = exit_status_file else { bail!("--exit-status-file is required") };
    let output = run(UxRegressionReceiptConfig {
        input,
        receipt,
        sha: Some(sha),
        exit_status_file: Some(exit_status_file),
    })?;
    // This binary is the CLI emitter, so reporting lives here. The package
    // denies print_stdout on every target (#16906), hence explicit writes.
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    match &output.written_path {
        Some(path) => writeln!(stdout, "Wrote UX regression receipt: {}", path.display())?,
        None => writeln!(stdout, "{}", output.payload)?,
    }
    Ok(())
}
