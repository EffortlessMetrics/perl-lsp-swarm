use anyhow::Result;
use perl_lsp_ux_tests::reverse_request_fixture::{ProtocolFailure, run};
use std::io::BufReader;

fn main() -> Result<()> {
    let protocol_failure = match std::env::var("UX_FIXTURE_PROTOCOL_FAILURE").ok().as_deref() {
        Some("malformed-frame") => Some(ProtocolFailure::MalformedFrame),
        Some("invalid-json") => Some(ProtocolFailure::InvalidJson),
        Some("partial-header") => Some(ProtocolFailure::PartialHeader),
        Some(other) => anyhow::bail!("unknown protocol failure mode: {other}"),
        None => None,
    };
    let did_close = std::env::var("UX_FIXTURE_DID_CLOSE").ok();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    run(&mut reader, &mut writer, protocol_failure, did_close.as_deref())
        .map_err(anyhow::Error::from)
}
