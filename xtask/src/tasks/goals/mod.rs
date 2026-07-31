//! Compatibility shims for the retired tracked goal selector.
//!
//! The repository no longer stores a current programme, lane, work queue, or
//! selection policy under `.perl-lsp/goals/`. GitHub owns live issues, PRs,
//! dependencies, reviews, checks, and remaining work; provider-native
//! `deliver-goal` and `deliver-pr` skills navigate that graph.

use color_eyre::eyre::Result;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct RetiredGoalCommand<'a> {
    schema_version: u8,
    status: &'a str,
    command: &'a str,
    authority: &'a str,
    replacement: [&'a str; 2],
    message: &'a str,
}

fn render_retired(command: &str, json: bool) -> Result<String> {
    let receipt = RetiredGoalCommand {
        schema_version: 1,
        status: "retired",
        command,
        authority: "github",
        replacement: ["GitHub issue/PR graph", "deliver-goal"],
        message: "tracked goal selection is retired; select work from current GitHub state",
    };

    if json {
        Ok(serde_json::to_string_pretty(&receipt)?)
    } else {
        Ok(format!(
            "{command}: retired — use current GitHub issues/PRs and deliver-goal"
        ))
    }
}

pub fn next(_program: Option<String>, _fixture: Option<PathBuf>, json: bool) -> Result<()> {
    println!("{}", render_retired("goals next", json)?);
    Ok(())
}

pub fn reconcile(
    _program: Option<String>,
    _fixture: Option<PathBuf>,
    json: bool,
) -> Result<usize> {
    println!("{}", render_retired("goals reconcile", json)?);
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retired_json_is_parseable_and_names_github_authority() -> Result<()> {
        let text = render_retired("goals next", true)?;
        let value: serde_json::Value = serde_json::from_str(&text)?;

        assert_eq!(value.get("status").and_then(serde_json::Value::as_str), Some("retired"));
        assert_eq!(value.get("authority").and_then(serde_json::Value::as_str), Some("github"));
        assert_eq!(value.get("command").and_then(serde_json::Value::as_str), Some("goals next"));
        Ok(())
    }
}
