use perl_lsp_rs_core::product_identity::{
    BinaryIdentityPacketV1, IdentityOutputFormat, requested_identity_output,
};
use std::io::Write as _;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if let Some(format) = requested_identity_output(&args) {
        let packet = BinaryIdentityPacketV1::embedded_server(env!("CARGO_PKG_VERSION"));
        let rendered = match format {
            IdentityOutputFormat::Human => packet.to_human(),
            IdentityOutputFormat::Json => match packet.to_json() {
                Ok(value) => value,
                Err(error) => {
                    let _ = writeln!(std::io::stderr(), "failed to serialize identity: {error}");
                    return std::process::ExitCode::FAILURE;
                }
            },
        };
        if writeln!(std::io::stdout(), "{rendered}").is_err() {
            return std::process::ExitCode::FAILURE;
        }
        return std::process::ExitCode::SUCCESS;
    }

    std::process::ExitCode::from(perllsp::run_cli(args) as u8)
}
