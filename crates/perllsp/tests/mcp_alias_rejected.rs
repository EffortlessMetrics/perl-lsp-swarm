use std::process::Command;

#[test]
fn retired_mcp_alias_exits_without_starting_lsp() {
    let output = Command::new(env!("CARGO_BIN_EXE_perllsp"))
        .arg("--mcp")
        .output()
        .expect("perllsp should execute");

    assert!(!output.status.success(), "--mcp unexpectedly succeeded");
    assert!(output.stdout.is_empty(), "protocol stdout was not empty: {:?}", output.stdout);

    let stderr = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(stderr.contains("`--mcp` is not an LSP transport alias."), "{stderr}");
    assert!(stderr.contains("Use `perllsp --stdio` for LSP."), "{stderr}");
    assert!(
        stderr.contains("Use `perllsp mcp --stdio` only when the native MCP adapter is available."),
        "{stderr}"
    );
    assert!(!stderr.contains("Content-Length"), "LSP framing leaked into rejection: {stderr}");
}
