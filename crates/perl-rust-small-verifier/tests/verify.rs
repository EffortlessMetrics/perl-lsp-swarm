//! CLI controls use synthetic receipts, never claims of executed product proof.
use serde_json::{Value, json};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

struct Repository(PathBuf);
impl Drop for Repository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn output(root: &Path, program: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(program).args(args).current_dir(root).output()?;
    if !result.status.success() {
        return Err(format!("{program} failed: {}", String::from_utf8_lossy(&result.stderr)).into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

fn fixture(root: &Path) -> Result<Value> {
    let mut steps = vec![
        ("fetch locked inputs", vec!["cargo", "fetch", "--locked"]),
        ("workspace compile check", vec!["cargo", "check", "--workspace", "--locked"]),
    ];
    for (name, package, test) in [
        ("parser semantic smoke", "perl-parser", "semantic_smoke_tests"),
        ("parser accuracy e2e", "perl-parser", "parser_accuracy_e2e"),
        (
            "parser accuracy NodeKind vocabulary",
            "perl-parser",
            "parser_accuracy_nodekind_vocabulary",
        ),
        ("lsp smoke", "perl-lsp-rs", "lsp_smoke"),
        ("postfix hash-slice recovery", "perl-parser-core", "postfix_hash_slice_tests"),
    ] {
        let mut args = vec!["cargo", "test", "--locked", "-p", package, "--test", test, "--"];
        if name == "lsp smoke" {
            args.push("--test-threads=1");
        }
        args.push("--nocapture");
        steps.push((name, args));
    }
    let base = vec![
        "cargo",
        "test",
        "-p",
        "perl-lsp-rs",
        "--lib",
        "--features",
        "workspace",
        "--profile",
        "agent",
        "--locked",
        "references_tier_scorecard_tests",
        "--",
    ];
    let mut census = base.clone();
    census.push("--list");
    let mut replay = base;
    replay.push("--test-threads=1");
    steps.extend([
        ("references scorecard census", census),
        ("references scorecard replay", replay),
        ("diff hygiene", vec!["git", "diff", "--check"]),
    ]);
    Ok(json!({
        "schema_version": "rust_small_proof.v1",
        "subject": {
            "git_sha": output(root, "git", &["rev-parse", "HEAD"])?,
            "worktree_dirty": false,
            "rustc_version": output(root, "rustc", &["--version"])?,
            "cargo_version": output(root, "cargo", &["--version"])?,
            "scorecard_profile": "agent", "scorecard_features": "workspace", "locked": true
        },
        "steps": steps.into_iter().map(|(name, argv)| json!({"name":name,"argv":argv,"outcome":"ok","exit_code":0})).collect::<Vec<_>>(),
        "scorecard_census": 42, "result": "success"
    }))
}

fn verify(root: &Path, receipt: &Value) -> Result<Output> {
    fs::write(root.join("receipt.json"), serde_json::to_vec(receipt)?)?;
    Ok(Command::new(env!("CARGO_BIN_EXE_perl-rust-small-verifier"))
        .args(["--verify-receipt", "receipt.json"])
        .current_dir(root)
        .output()?)
}

#[test]
fn cli_preserves_canonical_admission_and_failure_semantics() -> Result {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let path =
        std::env::temp_dir().join(format!("perl-receipt-cli-{}-{nonce}", std::process::id()));
    fs::create_dir(&path)?;
    let root = Repository(path);
    output(&root.0, "git", &["init", "--quiet"])?;
    fs::write(root.0.join("source.txt"), "synthetic test checkout\n")?;
    output(&root.0, "git", &["add", "source.txt"])?;
    output(
        &root.0,
        "git",
        &[
            "-c",
            "user.name=Receipt Test",
            "-c",
            "user.email=receipt@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    )?;
    let clean = fixture(&root.0)?;
    let accepted = verify(&root.0, &clean)?;
    assert!(accepted.status.success(), "{}", String::from_utf8_lossy(&accepted.stderr));

    let mut bad = clean.clone();
    bad["subject"]["git_sha"] = json!("0000000000000000000000000000000000000000");
    let rejected = verify(&root.0, &bad)?;
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("receipt subject does not match"));

    let mut bad = clean.clone();
    bad["steps"][0]["exit_code"] = json!(1);
    assert!(!verify(&root.0, &bad)?.status.success());

    let mut failed = clean.clone();
    failed["steps"][9]["outcome"] = json!("product_failure");
    failed["steps"][9]["exit_code"] = json!(1);
    failed["result"] = json!("product_failure");
    let accepted = verify(&root.0, &failed)?;
    assert!(
        accepted.status.success(),
        "coherent failure must remain valid: {}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(String::from_utf8_lossy(&accepted.stdout).contains("ProductFailure"));

    fs::write(root.0.join("source.txt"), "dirty\n")?;
    assert!(!verify(&root.0, &clean)?.status.success());
    fs::write(root.0.join("source.txt"), "synthetic test checkout\n")?;

    for args in [
        vec![],
        vec!["--verify-receipt"],
        vec!["--receipt", "receipt.json"],
        vec!["--verify-receipt", "receipt.json", "extra"],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_perl-rust-small-verifier"))
            .args(args)
            .current_dir(&root.0)
            .output()?;
        assert!(!result.status.success());
    }
    Ok(())
}
