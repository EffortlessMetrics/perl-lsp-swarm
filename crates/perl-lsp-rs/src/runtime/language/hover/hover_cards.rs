use super::presentation::{HoverBlock, HoverCard};
use super::{Value, json};

fn method_modifier_description(modifier_kind: &str) -> &'static str {
    match modifier_kind {
        "before" => "runs **before** the method — use for preconditions and logging",
        "after" => "runs **after** the method — use for postconditions and cleanup",
        "around" => {
            "wraps the method — receives `$orig` as first arg, must call `$orig->($self, @_)`"
        }
        "override" => "overrides the parent method — use to replace inherited behavior",
        "augment" => "extends the parent method — call `inner()` to invoke the next layer",
        _ => "modifies the method",
    }
}

pub(super) fn method_modifier_hover(modifier_kind: &str, method_name: &str, doc: &str) -> Value {
    let kind_label = method_modifier_description(modifier_kind);
    let mut blocks = vec![
        HoverBlock::MethodModifierHeading { modifier_kind: modifier_kind.to_string() },
        HoverBlock::MethodModifierSummary { method_name: method_name.to_string(), kind_label },
    ];
    if !doc.is_empty() {
        blocks.push(HoverBlock::UserProse { text: doc.to_string() });
    }
    HoverCard::new(blocks).to_lsp_hover(true)
}

/// Return the timing semantics description for a Perl phase block.
///
/// Returns `None` for unrecognised phase names so callers can fall through.
fn phase_block_description(phase: &str) -> Option<(&'static str, &'static str)> {
    // Returns (short_timing, full_description)
    match phase {
        "BEGIN" => Some((
            "Compile-time execution",
            "Runs **as soon as the block is fully parsed**, at **compile time**, \
before the rest of the file compiles. \
Multiple `BEGIN` blocks run in declaration order (FIFO). \
Use for loading modules, setting up the environment, and other tasks that \
must complete before compilation continues.",
        )),
        "END" => Some((
            "Program-exit cleanup",
            "Runs at **program exit** (after the main program finishes or on `exit()`), \
in **reverse declaration order** (LIFO). \
Not called on abnormal exit via `exec` or unhandled signals. \
Use for cleanup tasks such as removing temporary files.",
        )),
        "INIT" => Some((
            "Post-compile, pre-runtime startup",
            "Runs at the **start of runtime**, after all compilation completes \
and before the main program body executes, in **declaration order** (FIFO). \
Caveat: `INIT` may be unreliable when invoked via `require` or string `eval` \
after the main compile phase.",
        )),
        "CHECK" => Some((
            "End-of-compilation hook",
            "Runs at the **end of compilation** (after all compilation, before runtime), \
in **reverse declaration order** (LIFO). \
Caveat: `CHECK` may be unreliable when invoked via `require` or string `eval` \
after the main compile phase.",
        )),
        "UNITCHECK" => Some((
            "End-of-compilation-unit hook",
            "Runs at the end of the **compilation unit** it is defined in \
(more localised and reliable than `CHECK`), in **reverse declaration order** (LIFO). \
Unlike `CHECK`, `UNITCHECK` fires for each `require`d or `eval`'d compilation unit \
independently.",
        )),
        _ => None,
    }
}

/// Build a hover card for a Perl phase block (`BEGIN`, `END`, `INIT`, `CHECK`, `UNITCHECK`).
///
/// Returns `None` when `phase` is not one of the recognised phase keywords.
pub(super) fn phase_block_hover(phase: &str) -> Option<Value> {
    let (timing, description) = phase_block_description(phase)?;
    Some(
        HoverCard::new(vec![
            HoverBlock::PhaseBlockHeading { phase: phase.to_string() },
            HoverBlock::TimingLabel { text: timing },
            HoverBlock::AuthoredProse { text: description },
            HoverBlock::PresentationLink {
                label: "perlmod — Special Subroutines".to_string(),
                url: "https://perldoc.perl.org/perlmod#BEGIN%2C-UNITCHECK%2C-CHECK%2C-INIT-and-END"
                    .to_string(),
            },
        ])
        .to_lsp_hover(true),
    )
}

/// Indexed workspace-qualified callable (`Pkg::sub`). Must not include install advice.
pub(super) fn qualified_callable_hover(
    kind_label: &str,
    signature: &str,
    package: &str,
    documentation: Option<&str>,
) -> Value {
    let mut blocks = vec![
        HoverBlock::BoldTitle { title: kind_label.to_string() },
        HoverBlock::CodeLine { code: signature.to_string() },
        HoverBlock::PackageDefinedLine { package: package.to_string() },
    ];
    if let Some(text) = documentation.filter(|text| !text.is_empty()) {
        blocks.push(HoverBlock::UserProse { text: text.to_string() });
    }
    HoverCard::new(blocks).to_lsp_hover(true)
}

/// Fallback when a qualified callable is not an indexed workspace sub.
///
/// Intentionally not the missing-module card: an unresolved `Pkg::sub()` is
/// not a CPAN module to `cpanm`.
pub(super) fn unresolved_qualified_token_hover(qualified: &str) -> Value {
    json!({
        "contents": {
            "kind": "markdown",
            "value": format!("**Perl**: `{qualified}`"),
        },
    })
}
