//! Contract for releasing root attention when a claim reaches a remote-owned wait.
//!
//! #13393 owns continuation/yield semantics. This test prevents provider-specific
//! scheduling mechanics from turning an exact wake event into a same-session polling
//! loop that keeps a large engineering root alive while other claims remain actionable.

use std::fs;
use std::path::{Path, PathBuf};

type DynError = Box<dyn std::error::Error>;

const ATTENTION_RELEASE: &str = "A remote wait releases root attention.";
const NO_SCHEDULED_POLLING: &str =
    "Do not create a timer, cron, scheduled reminder, recurring wake, or polling loop merely to revisit the same remote condition.";

const SURFACES: &[&str] = &[
    "AGENTS.md",
    "CLAUDE.md",
    ".agents/skills/deliver-goal/SKILL.md",
    ".claude/skills/deliver-goal/SKILL.md",
    "docs/agents/DEVELOPMENT_METHOD.md",
];

fn root() -> Result<PathBuf, DynError> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("CARGO_MANIFEST_DIR has no parent")?
        .to_path_buf())
}

fn read(root: &Path, relative: &str) -> Result<String, DynError> {
    let path = root.join(relative);
    fs::read_to_string(&path).map_err(|error| {
        std::io::Error::new(error.kind(), format!("failed to read {}: {error}", path.display()))
            .into()
    })
}

fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_surface(path: &str, text: &str) -> Result<(), String> {
    let text = normalized(text);
    for marker in [ATTENTION_RELEASE, NO_SCHEDULED_POLLING] {
        if !text.contains(marker) {
            return Err(format!("{path} lost remote-wait invariant {marker:?}"));
        }
    }
    Ok(())
}

#[test]
fn remote_wait_releases_root_attention_across_current_authorities() -> Result<(), DynError> {
    let root = root()?;
    for path in SURFACES {
        let text = read(&root, path)?;
        validate_surface(path, &text).map_err(std::io::Error::other)?;
    }
    Ok(())
}

#[test]
fn ratchet_rejects_same_session_timer_polling() -> Result<(), DynError> {
    let root = root()?;
    let source = read(&root, ".claude/skills/deliver-goal/SKILL.md")?;
    let regressed = source.replacen(
        NO_SCHEDULED_POLLING,
        "Create a timer to revisit the same remote condition.",
        1,
    );

    assert_ne!(regressed, source, "timer-polling mutation fixture must apply");
    assert!(
        validate_surface("mutated deliver-goal", &regressed).is_err(),
        "a provider skill that restores same-session scheduled polling must fail the contract"
    );
    Ok(())
}

#[test]
fn ratchet_rejects_attention_retention_during_remote_wait() -> Result<(), DynError> {
    let root = root()?;
    let source = read(&root, ".agents/skills/deliver-goal/SKILL.md")?;
    let regressed = source.replacen(
        ATTENTION_RELEASE,
        "A remote wait retains root attention.",
        1,
    );

    assert_ne!(regressed, source, "attention-retention mutation fixture must apply");
    assert!(
        validate_surface("mutated deliver-goal", &regressed).is_err(),
        "a provider skill that keeps root attention attached to remote wait must fail the contract"
    );
    Ok(())
}
