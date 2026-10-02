//! Rust Small has no local result/mirror job: admission owns draft suppression.
use serde_yaml_ng::Value;
use std::{fs, path::Path};
type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub fn validate(text: &str) -> TestResult<()> {
    let yaml: Value = serde_yaml_ng::from_str(text)?;
    let jobs = yaml.get("jobs").and_then(Value::as_mapping).ok_or("jobs missing")?;
    if jobs.len() != 1 {
        return Err("Rust Small must have one governed call and no local draft mirror".into());
    }
    let call = jobs.get(Value::String("rust-small-proof".into())).ok_or("governed call missing")?;
    if call.get("name").and_then(Value::as_str) != Some("Perl LSP Rust Small governed proof") {
        return Err("native result context prefix drifted".into());
    }
    let guard = call.get("if").and_then(Value::as_str).ok_or("draft admission missing")?;
    if guard != "github.event.pull_request.draft != true || github.event_name != 'pull_request'" {
        return Err("drafts must skip the complete governed call".into());
    }
    if call.get("uses").and_then(Value::as_str)
        != Some(
            "EffortlessMetrics/em-ci-workflows/.github/workflows/rust.yml@a3125de962a74e7ec2e127d41d7532f66c69334d",
        )
    {
        return Err("isolated owned policy revision drifted".into());
    }
    let inputs = call.get("with").ok_or("proof inputs missing")?;
    for (key, expected) in [
        ("profile", "standard"),
        ("script", ".ci/rust-standard-proof.sh"),
        ("result_script", ".ci/rust-standard-result.sh"),
    ] {
        if inputs.get(key).and_then(Value::as_str) != Some(expected) {
            return Err(format!("governed {key} drifted").into());
        }
    }
    for forbidden in ["steps", "runs-on", "secrets", "continue-on-error", "permissions"] {
        if call.get(forbidden).is_some() {
            return Err(format!("consumer call must not add {forbidden}").into());
        }
    }
    let scopes =
        yaml.get("permissions").and_then(Value::as_mapping).ok_or("read scopes missing")?;
    if scopes.len() != 4 {
        return Err("read scope set drifted".into());
    }
    for scope in ["actions", "checks", "contents", "pull-requests"] {
        if scopes.get(Value::String(scope.into())).and_then(Value::as_str) != Some("read") {
            return Err(format!("scope {scope} must remain read only").into());
        }
    }
    Ok(())
}

pub fn check_contract(path: &Path) -> TestResult<()> {
    let text = fs::read_to_string(path)?;
    validate(&text)?;
    for (before, after) in [
        ("name: Perl LSP Rust Small governed proof", "name: Unbound proof"),
        ("draft != true", "draft == true"),
        ("result_script: .ci/rust-standard-result.sh", "result_script: .ci/noop.sh"),
        ("  checks: read", "  checks: write"),
    ] {
        let mutant = text.replacen(before, after, 1);
        if mutant == text || validate(&mutant).is_ok() {
            return Err(format!("owned-policy mutant was not rejected: {before}").into());
        }
    }
    if validate(&(text + "\n  obsolete-result:\n    runs-on: ubuntu-latest\n    steps: []\n"))
        .is_ok()
    {
        return Err("local result/mirror restoration was not rejected".into());
    }
    Ok(())
}
