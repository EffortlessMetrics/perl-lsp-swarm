use anyhow::Result;
use perl_lsp_ux_tests::server_request_fixture;
use std::io::BufReader;

fn main() -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = stdout.lock();
    server_request_fixture::run(&mut reader, &mut writer)
}
