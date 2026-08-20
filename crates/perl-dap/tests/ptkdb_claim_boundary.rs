//! Prevents fake-peer host proof from being promoted into an unearned real
//! ptkdb compatibility claim.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(repo_root().join(path))?)
}

#[test]
fn ptkdb_docs_separate_bootstrap_host_protocol_and_live_partner_proof()
-> Result<(), Box<dyn std::error::Error>> {
    let quickstart = read("docs/how-to/EXTERNAL_DEBUGGER_PEER_QUICKSTART.md")?;
    assert!(quickstart.contains("Experimental / developer preview"));
    assert!(quickstart.contains("Stock `Devel::ptkdb` live peer"));
    assert!(quickstart.contains("Not yet proven"));
    assert!(quickstart.contains("This is one-way setup"));
    assert!(quickstart.contains("A peer session starts with no assumed capabilities"));

    let target = read("docs/reference/PTKDB_PEER_INTEGRATION_TARGET.md")?;
    assert!(target.contains("mirror_minimum"));
    assert!(target.contains("Fake-peer conformance establishes host correctness only"));
    assert!(target.contains("does **not** require ptkdb to accept editor control"));
    assert!(target.contains("ptkdb live peer experimental"));

    let decisions = read("docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md")?;
    assert!(decisions.contains("stock ptkdb live compatibility  not proven"));
    assert!(decisions.contains("A session starts with `none`"));
    assert!(
        decisions.contains("The Microsoft DAP implementor listing names `perl-dap`, not ptkdb")
    );

    Ok(())
}

#[test]
fn ptkdb_docs_do_not_reintroduce_optimistic_v1_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    for path in [
        "docs/how-to/EXTERNAL_DEBUGGER_PEER_QUICKSTART.md",
        "docs/reference/PTKDB_PEER_INTEGRATION_TARGET.md",
        "docs/reference/EXTERNAL_DEBUGGER_PEER_DECISIONS.md",
    ] {
        let content = read(path)?;
        for forbidden in [
            "✅ against a peer that speaks the protocol",
            "realistic v1 capability report for ptkdb",
            "stock ptkdb is supported",
            "fully supported ptkdb integration",
        ] {
            assert!(
                !content.contains(forbidden),
                "{path} contains unearned ptkdb claim {forbidden:?}"
            );
        }
    }

    Ok(())
}

#[test]
fn vscode_keeps_native_as_the_default_debugger_backend() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = read("vscode-extension/package.json")?;
    assert!(manifest.contains("debuggerBackend"));
    assert!(manifest.contains("\"default\": \"native\""));
    assert!(manifest.contains("Experimental external live peer"));
    assert!(manifest.contains("stock Devel::ptkdb compatibility is not proven"));
    assert!(manifest.contains("Perl: ptkdb live peer (experimental mirror)"));
    Ok(())
}
