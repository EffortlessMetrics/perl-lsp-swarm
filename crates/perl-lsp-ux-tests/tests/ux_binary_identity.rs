//! Wrong and stale explicit overrides must never become current UX proof.
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::process::Command;

#[test]
fn override_probe_child() -> Result<()> {
    if std::env::var_os("UX_IDENTITY_CHILD").is_some() {
        let error =
            perl_lsp_ux_tests::resolve_binary().context("selected executable must be rejected");
        assert!(error.is_err(), "wrong/stale override became current proof");
        let detail = format!("{:#}", error.err().context("expected refusal")?);
        assert!(detail.contains(&std::env::var("UX_EXPECT_REFUSAL")?), "{detail}");
    }
    Ok(())
}

#[test]
fn wrong_and_stale_overrides_are_rejected() -> Result<()> {
    let fixture = env!("CARGO_BIN_EXE_ux_identity_fixture");
    let base = json!({"schema_version":"perl_lsp.binary_identity.v1","binary":{"role":"server","cargo_package":"perllsp"},"product":{"name":"perl-lsp"},"build":{"source_revision":"a".repeat(40),"source_tree_digest":"b".repeat(64)}});
    for (packet, reason) in [
        (json!({"product":{"name":"other-tool"}}), "wrong product"),
        (base.clone(), "source_revision"),
        (
            json!({"schema_version":"perl_lsp.binary_identity.v1","binary":{"role":"server","cargo_package":"perllsp"},"product":{"name":"perl-lsp"},"build":{}}),
            "source_revision",
        ),
    ] {
        reject_override(fixture, &packet, reason)?;
    }
    for pointer in ["/schema_version", "/binary/role", "/binary/cargo_package"] {
        let mut wrong = base.clone();
        *wrong.pointer_mut(pointer).context("fixture field")? = json!("wrong");
        reject_override(fixture, &wrong, "wrong executable identity")?;
    }
    // Preserve the exact current revision, but report a stale/dirty tree digest.
    let revision = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    let mut stale = base;
    stale["build"]["source_revision"] =
        Value::String(String::from_utf8(revision.stdout)?.trim().to_owned());
    reject_override(fixture, &stale, "source_tree_digest")?;
    Ok(())
}

fn reject_override(fixture: &str, packet: &Value, reason: &str) -> Result<()> {
    let output = Command::new(std::env::current_exe()?)
        .args(["--exact", "override_probe_child", "--nocapture"])
        .env("UX_IDENTITY_CHILD", "1")
        .env("PERL_LSP_BIN", fixture)
        .env("UX_IDENTITY_PACKET", packet.to_string())
        .env("UX_EXPECT_REFUSAL", reason)
        .output()?;
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
