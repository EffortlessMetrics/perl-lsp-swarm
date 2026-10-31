//! Contract for releasing root attention when a claim reaches a remote-owned wait.
//!
//! #13393 owns continuation/yield semantics. This test prevents provider-specific
//! scheduling mechanics from turning an exact wake event into a same-session polling
//! loop that keeps a large engineering root alive while other claims remain actionable.

use std::fs;
use std::path::{Path, PathBuf};

type DynError = Box<dyn std::error::Error>;

// #18087: the current authorities express the same obligations in different
// clauses. Accept those reviewed formulations without rewriting the authorities
// to satisfy a test. This is a finite prose contract, not a semantic-language
// validator: a new formulation still needs review and a discriminating control.
const ATTENTION_RELEASE: &[(&str, &str)] = &[
    ("A remote wait releases root attention.", "A remote wait retains root attention."),
    (
        "When all remaining work has real external blockers, return those blockers and their wake events instead of maintaining an idle watcher.",
        "When all remaining work has real external blockers, maintain an idle watcher instead of returning those blockers and their wake events.",
    ),
    (
        "When a real interruption or out-of-window dependency owns the next transition, retain the frame as `IN_FLIGHT`, release unnecessary contexts and advance another actionable claim.",
        "When a real interruption or out-of-window dependency owns the next transition, retain the frame as `IN_FLIGHT`, retain unnecessary contexts and do not advance another actionable claim.",
    ),
];
const NO_SCHEDULED_POLLING: &[(&str, &str)] = &[
    (
        "Do not create a timer, cron, scheduled reminder, recurring wake, or polling loop merely to revisit the same remote condition.",
        "Create a timer, cron, scheduled reminder, recurring wake, or polling loop merely to revisit the same remote condition.",
    ),
    (
        "Use supported waits/events or bounded checks; do not create a polling child or scheduled monitor.",
        "Use supported waits/events or bounded checks; create a polling child or scheduled monitor.",
    ),
    (
        "Do not create a timer, cron, polling child or idle watcher for ordinary work.",
        "Create a timer, cron, polling child or idle watcher for ordinary work.",
    ),
];
const OBLIGATIONS: &[(&str, &[(&str, &str)])] =
    &[("attention release", ATTENTION_RELEASE), ("no scheduled polling", NO_SCHEDULED_POLLING)];

/// Lowercase affirmative form of the polling directive. Marker presence alone
/// cannot reject a surface that keeps the negative sentence but adds the exact
/// polling behavior elsewhere, so validation rejects the directive itself.
const ADDITIVE_POLLING_DIRECTIVE: &str = "create a timer to revisit the same remote condition";

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
    for (obligation, variants) in OBLIGATIONS {
        if !variants.iter().any(|(accepted, _)| text.contains(*accepted)) {
            return Err(format!("{path} lost remote-wait invariant {obligation:?}"));
        }
        // An accepted clause must not hide an additive contradictory clause.
        if variants.iter().any(|(_, reversed)| text.contains(*reversed)) {
            return Err(format!("{path} contradicts remote-wait invariant {obligation:?}"));
        }
    }
    if text.to_lowercase().contains(ADDITIVE_POLLING_DIRECTIVE) {
        return Err(format!(
            "{path} adds a same-session polling directive that contradicts the remote-wait invariant"
        ));
    }
    Ok(())
}

#[test]
fn remote_wait_releases_root_attention_across_current_authorities() -> Result<(), DynError> {
    let root = root()?;
    let mut errors = Vec::new();
    for path in SURFACES {
        if let Err(error) = validate_surface(path, &read(&root, path)?) {
            errors.push(error);
        }
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    Ok(())
}

#[test]
fn ratchet_rejects_removal_of_each_obligation_on_every_surface() -> Result<(), DynError> {
    let root = root()?;
    for path in SURFACES {
        let source = normalized(&read(&root, path)?);
        validate_surface(path, &source).map_err(std::io::Error::other)?;
        for (obligation, variants) in OBLIGATIONS {
            let mut regressed = source.clone();
            for (accepted, _) in *variants {
                regressed = regressed.replace(*accepted, "");
            }
            assert_ne!(regressed, source, "{path}: {obligation} removal must apply");
            assert_eq!(
                validate_surface(path, &regressed),
                Err(format!("{path} lost remote-wait invariant {obligation:?}")),
            );
        }
    }
    Ok(())
}

#[test]
fn ratchet_rejects_additive_polling_directive() -> Result<(), DynError> {
    let root = root()?;
    for path in SURFACES {
        let source = read(&root, path)?;
        validate_surface(path, &source).map_err(std::io::Error::other)?;
        let regressed = format!("{source}\nCreate a timer to revisit the same remote condition.\n");
        assert_eq!(
            validate_surface(path, &regressed),
            Err(format!(
                "{path} adds a same-session polling directive that contradicts the remote-wait invariant"
            )),
        );
    }
    Ok(())
}

#[test]
fn ratchet_accepts_current_wording_variants_and_rejects_their_reversals() -> Result<(), DynError> {
    let root = root()?;
    for path in SURFACES {
        let source = normalized(&read(&root, path)?);
        validate_surface(path, &source).map_err(std::io::Error::other)?;
        for (obligation, variants) in OBLIGATIONS {
            let mut without_obligation = source.clone();
            for (accepted, _) in *variants {
                without_obligation = without_obligation.replace(*accepted, "");
            }
            assert_ne!(without_obligation, source, "{path}: {obligation} replacement must apply");
            for (accepted, reversed) in *variants {
                let reworded = format!("{without_obligation}\n{accepted}\n");
                validate_surface(path, &reworded).map_err(std::io::Error::other)?;
                let regressed = format!("{without_obligation}\n{reversed}\n");
                assert_eq!(
                    validate_surface(path, &regressed),
                    Err(format!("{path} lost remote-wait invariant {obligation:?}")),
                );
                let contradictory = format!("{source}\n{reversed}\n");
                assert_eq!(
                    validate_surface(path, &contradictory),
                    Err(format!("{path} contradicts remote-wait invariant {obligation:?}")),
                );
            }
        }
    }
    Ok(())
}

#[test]
fn ratchet_accepts_markdown_rewrapping_on_every_surface() -> Result<(), DynError> {
    let root = root()?;
    for path in SURFACES {
        let source = read(&root, path)?;
        validate_surface(path, &source).map_err(std::io::Error::other)?;
        let rewrapped = source.split_whitespace().collect::<Vec<_>>().join("\n");
        validate_surface(path, &rewrapped).map_err(std::io::Error::other)?;
    }
    Ok(())
}

#[test]
fn ratchet_rejects_empty_surface() {
    assert_eq!(
        validate_surface("empty", ""),
        Err("empty lost remote-wait invariant \"attention release\"".to_owned()),
    );
}
