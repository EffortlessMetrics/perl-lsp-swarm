//! Recurrence guard: the shared UX harness must not regrow synchronization sleeps.
//!
//! A wall-clock sleep in the *shared* harness is a weak correctness oracle that
//! every scenario inherits: it lengthens fast runs, makes results runner-speed
//! sensitive, and makes a lost wakeup indistinguishable from slow product
//! behavior. After #13319 the harness waits on observations instead
//! (`observation::Inbox`), so a new bare sleep here is a regression.
//!
//! This guard deliberately does **not** ban sleeping outright. A sleep is
//! legitimate when it is the *stimulus* a test applies, or when the product
//! contract itself requires re-issuing a request. Those must simply say so, on
//! the line above the sleep:
//!
//! ```text
//! // ux-timing: deliberate-stimulus — non-zero elapsed time for the timing recorder
//! // ux-timing: product-retry — workspace/symbol has no push notification to wait on
//! ```
//!
//! An unmarked sleep, or one carrying an unknown class, fails this test.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Files this guard governs.
///
/// The shared harness is covered in full: a synchronization sleep here is
/// inherited by every scenario. Scenario files are covered as they are
/// migrated; `ux_scenario_01_simple_file.rs` is covered because #13319 names it.
const GOVERNED: &[&str] = &[
    "src/client.rs",
    "src/diagnostics.rs",
    "src/env.rs",
    "src/lib.rs",
    "src/observation.rs",
    "src/project_fixture.rs",
    "src/recorder.rs",
    "src/scorecard.rs",
    "src/taxonomy.rs",
    "src/workspace.rs",
    "tests/ux_scenario_01_simple_file.rs",
];

/// Files that must contain no sleep at all, marked or not.
///
/// These are the transport and wait substrate itself. There is no legitimate
/// reason for the layer that *implements* event-driven waiting to sleep.
const SLEEP_FREE: &[&str] = &["src/client.rs", "src/diagnostics.rs", "src/observation.rs"];

/// Timing classes a sleep may declare.
const CLASSES: &[&str] = &["deliberate-stimulus", "product-retry"];

/// How far above a sleep the guard looks for its disposition marker.
const MARKER_LOOKBACK: usize = 3;

const MARKER: &str = "ux-timing:";

/// One unowned sleep.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Violation {
    file: String,
    line: usize,
    reason: String,
    source: String,
}

/// Does this line call — or import — a thread sleep?
///
/// Whitespace around `::` is normalised so `thread :: sleep(` cannot slip past,
/// and a bare `thread::sleep` without a paren is matched too, which catches
/// `use std::thread::sleep as pause;` — the aliasing route that would otherwise
/// let a synchronization sleep back in under a different name.
///
/// Known limit: a call split across lines (`thread::sleep` then `(` on the next
/// line) is not detected. This is a ratchet against the shape this crate
/// actually removed, not a general polling detector — a busy-wait loop with no
/// sleep at all is likewise out of its reach.
fn is_sleep(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or("");
    let normalized: String = code.replace(" ::", "::").replace(":: ", "::").replace("\t", " ");
    normalized.contains("thread::sleep")
}

/// Return the declared class on a marker line, if it declares one.
fn declared_class(line: &str) -> Option<&'static str> {
    let rest = line.split_once(MARKER)?.1.trim();
    CLASSES.iter().copied().find(|class| {
        rest.strip_prefix(class)
            // A class must end or be followed by whitespace, so `product-retry-ish`
            // cannot masquerade as `product-retry`.
            .is_some_and(|tail| tail.is_empty() || tail.starts_with(char::is_whitespace))
    })
}

/// Find every sleep in `contents` that does not carry a valid disposition.
fn unowned_sleeps(file: &str, contents: &str, sleep_free: bool) -> Vec<Violation> {
    let lines: Vec<&str> = contents.lines().collect();
    let mut violations = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if !is_sleep(line) {
            continue;
        }
        let source = line.trim().to_string();
        if sleep_free {
            violations.push(Violation {
                file: file.to_string(),
                line: index + 1,
                reason: "the wait substrate itself must never sleep".to_string(),
                source,
            });
            continue;
        }

        let lookback = index.saturating_sub(MARKER_LOOKBACK);
        let marker = lines[lookback..index].iter().find(|candidate| candidate.contains(MARKER));
        let reason = match marker {
            None => format!(
                "no `{MARKER}` disposition within {MARKER_LOOKBACK} lines above; \
                 wait on an observation instead, or declare one of {CLASSES:?}"
            ),
            Some(marker) if declared_class(marker).is_none() => {
                format!("`{MARKER}` marker declares no known class; expected one of {CLASSES:?}")
            }
            Some(_) => continue,
        };
        violations.push(Violation { file: file.to_string(), line: index + 1, reason, source });
    }

    violations
}

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[test]
fn the_shared_harness_has_no_unowned_synchronization_sleep() -> anyhow::Result<()> {
    let root = crate_root();
    let sleep_free: BTreeSet<&str> = SLEEP_FREE.iter().copied().collect();
    let mut violations = Vec::new();
    let mut unreadable = Vec::new();

    for relative in GOVERNED {
        match fs::read_to_string(root.join(relative)) {
            Ok(contents) => {
                violations.extend(unowned_sleeps(
                    relative,
                    &contents,
                    sleep_free.contains(relative),
                ));
            }
            // Coverage must never lapse silently: a governed file that moved
            // away has to be re-pointed deliberately, not dropped.
            Err(error) => unreadable.push(format!("{relative}: {error}")),
        }
    }

    anyhow::ensure!(
        unreadable.is_empty(),
        "governed files could not be read; update GOVERNED in this guard \
         deliberately rather than letting coverage lapse:\n  {}",
        unreadable.join("\n  ")
    );

    violations.sort();
    anyhow::ensure!(
        violations.is_empty(),
        "unowned synchronization sleeps in the shared UX harness:\n{}",
        violations
            .iter()
            .map(|v| format!("  {}:{}  {}\n      → {}", v.file, v.line, v.source, v.reason))
            .collect::<Vec<_>>()
            .join("\n")
    );
    Ok(())
}

/// The guard is only worth having if it actually rejects the shapes it claims
/// to. These are its negative controls.
#[cfg(test)]
mod guard_controls {
    use super::{Violation, declared_class, unowned_sleeps};

    fn reasons(violations: &[Violation]) -> Vec<&str> {
        violations.iter().map(|v| v.reason.as_str()).collect()
    }

    #[test]
    fn a_bare_sleep_is_rejected() -> anyhow::Result<()> {
        let source = "fn wait() {\n    std::thread::sleep(POLL);\n}\n";
        let found = unowned_sleeps("src/x.rs", source, false);
        anyhow::ensure!(found.len() == 1, "a bare sleep must be reported: {found:?}");
        anyhow::ensure!(
            found.first().ok_or_else(|| anyhow::anyhow!("missing violation"))?.line == 2,
            "expected the bare sleep on line 2, got {found:?}"
        );
        Ok(())
    }

    #[test]
    fn a_declared_stimulus_is_accepted() -> anyhow::Result<()> {
        let source = "// ux-timing: deliberate-stimulus — non-zero elapsed time\n\
                      std::thread::sleep(FIVE_MS);\n";
        anyhow::ensure!(
            unowned_sleeps("src/x.rs", source, false).is_empty(),
            "declared stimulus must be accepted: {source:?}"
        );
        Ok(())
    }

    #[test]
    fn a_declared_product_retry_is_accepted() -> anyhow::Result<()> {
        let source = "// ux-timing: product-retry — no push notification exists\n\
                      std::thread::sleep(pause);\n";
        anyhow::ensure!(
            unowned_sleeps("src/x.rs", source, false).is_empty(),
            "declared product retry must be accepted: {source:?}"
        );
        Ok(())
    }

    #[test]
    fn an_unknown_class_is_rejected() -> anyhow::Result<()> {
        let source = "// ux-timing: because-it-flakes\nstd::thread::sleep(POLL);\n";
        let found = unowned_sleeps("src/x.rs", source, false);
        anyhow::ensure!(found.len() == 1, "an invented class must not pass: {found:?}");
        anyhow::ensure!(
            reasons(&found)
                .first()
                .ok_or_else(|| anyhow::anyhow!("missing violation reason"))?
                .contains("no known class"),
            "unknown class must have a no-known-class reason: {found:?}"
        );
        Ok(())
    }

    /// A marker must not be reusable from far above by an unrelated sleep.
    #[test]
    fn a_distant_marker_does_not_cover_a_later_sleep() -> anyhow::Result<()> {
        let source = "// ux-timing: product-retry — legitimate\n\
                      std::thread::sleep(pause);\n\
                      let a = 1;\n\
                      let b = 2;\n\
                      let c = 3;\n\
                      let d = 4;\n\
                      std::thread::sleep(POLL);\n";
        let found = unowned_sleeps("src/x.rs", source, false);
        anyhow::ensure!(found.len() == 1, "only the uncovered sleep is a violation: {found:?}");
        anyhow::ensure!(
            found.first().ok_or_else(|| anyhow::anyhow!("missing violation"))?.line == 7,
            "expected uncovered sleep on line 7, got {found:?}"
        );
        Ok(())
    }

    #[test]
    fn a_class_prefix_cannot_masquerade_as_the_class() -> anyhow::Result<()> {
        anyhow::ensure!(
            declared_class("// ux-timing: product-retry — ok").is_some(),
            "exact product-retry class must be recognized"
        );
        anyhow::ensure!(
            declared_class("// ux-timing: product-retrying-forever").is_none(),
            "a longer word starting with a valid class must not be accepted"
        );
        for suffix in ["-ish", "_unknown", ".unknown"] {
            let marker = format!("// ux-timing: product-retry{suffix}");
            anyhow::ensure!(declared_class(&marker).is_none(), "unknown class accepted: {marker}");
            let source = format!("{marker}\nstd::thread::sleep(POLL);\n");
            let found = unowned_sleeps("src/x.rs", &source, false);
            anyhow::ensure!(found.len() == 1, "unknown class must not waive a sleep: {found:?}");
        }
        Ok(())
    }

    /// A marked sleep is still forbidden inside the wait substrate itself.
    #[test]
    fn the_substrate_rejects_even_a_declared_sleep() -> anyhow::Result<()> {
        let source = "// ux-timing: product-retry — nice try\nstd::thread::sleep(POLL);\n";
        let found = unowned_sleeps("src/observation.rs", source, true);
        anyhow::ensure!(found.len() == 1, "the substrate must be sleep-free: {found:?}");
        anyhow::ensure!(
            reasons(&found)
                .first()
                .ok_or_else(|| anyhow::anyhow!("missing violation reason"))?
                .contains("never sleep"),
            "substrate violation must explain never-sleep rule: {found:?}"
        );
        Ok(())
    }

    /// A sleep mentioned only in prose or a doc comment is not a call.
    #[test]
    fn an_aliased_sleep_import_is_rejected() -> anyhow::Result<()> {
        let source = "use std::thread::sleep as pause;\nlet x = 1;\n";
        let found = unowned_sleeps("src/x.rs", source, false);
        anyhow::ensure!(found.len() == 1, "aliasing must not smuggle a sleep in: {found:?}");
        Ok(())
    }

    #[test]
    fn spacing_around_the_path_separator_cannot_hide_a_sleep() -> anyhow::Result<()> {
        let source = "std::thread :: sleep(POLL);\n";
        let found = unowned_sleeps("src/x.rs", source, false);
        anyhow::ensure!(found.len() == 1, "spaced paths must still be caught: {found:?}");
        Ok(())
    }

    #[test]
    fn a_sleep_named_in_a_comment_is_not_a_violation() -> anyhow::Result<()> {
        let source = "// we no longer thread::sleep( here\nlet x = 1;\n";
        anyhow::ensure!(
            unowned_sleeps("src/x.rs", source, false).is_empty(),
            "comment-only sleep mention must be accepted: {source:?}"
        );
        Ok(())
    }
}
