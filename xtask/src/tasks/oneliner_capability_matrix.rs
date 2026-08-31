//! Generate and check the command-line (one-liner) analysis capability matrix.
//!
//! "One-liner support" can mean at least eight different things, from accepting
//! the source body Perl hands the parser through to running a trusted
//! differential oracle against real `perl`. A single yes/no badge would hide
//! those distinctions, so this task publishes one row per capability layer and
//! per command-line form.
//!
//! The declared rows in [`ROWS`] are the *claim*. They are not self-certifying:
//! every row that claims `supported` or `partial` must name fixture evidence
//! that is live in the command-line conformance corpus, and the corpus is
//! parsed from source rather than restated here. A row that claims a behavior
//! with no such evidence fails the check instead of rendering an optimistic
//! table.
//!
//! What this check does and does not establish, stated exactly: it proves the
//! citation is real and reachable — the fixture exists in the corpus as running
//! code, is not commented out, quoted, `#[ignore]`d, or disabled by `cfg`, and
//! declares every switch the row claims. It does **not** execute the fixture; whether it passes is
//! established by running [`CORPUS_COMMAND`], which every earned row names so a
//! reader can run it. The two together are the guarantee; neither alone is.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

/// Generated matrix location.
pub const MATRIX_PATH: &str = "docs/project/status/oneliner_capability_matrix.md";

/// The conformance corpus that supplies parser-body evidence.
pub const CORPUS_PATH: &str = "crates/perl-parser-core/tests/command_line_oneliners.rs";

/// The command a reader can run to execute every cited parser-body fixture.
const CORPUS_COMMAND: &str = "cargo test -p perl-parser-core --test command_line_oneliners";

/// One analysis layer. Support for a layer is never implied by another layer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Layer {
    ParserBody,
    StructuredArgv,
    SourceComposition,
    ImplicitRuntime,
    CompileTimeContext,
    EditorOperations,
    ShellAdapters,
    DifferentialOracle,
}

impl Layer {
    const ALL: &'static [Layer] = &[
        Layer::ParserBody,
        Layer::StructuredArgv,
        Layer::SourceComposition,
        Layer::ImplicitRuntime,
        Layer::CompileTimeContext,
        Layer::EditorOperations,
        Layer::ShellAdapters,
        Layer::DifferentialOracle,
    ];

    fn ordinal(self) -> usize {
        match self {
            Layer::ParserBody => 1,
            Layer::StructuredArgv => 2,
            Layer::SourceComposition => 3,
            Layer::ImplicitRuntime => 4,
            Layer::CompileTimeContext => 5,
            Layer::EditorOperations => 6,
            Layer::ShellAdapters => 7,
            Layer::DifferentialOracle => 8,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Layer::ParserBody => "Parser-body acceptance",
            Layer::StructuredArgv => "Structured-argv decoding",
            Layer::SourceComposition => "Source composition and provenance",
            Layer::ImplicitRuntime => "Implicit runtime/loop context",
            Layer::CompileTimeContext => "Compile-time feature/module/include context",
            Layer::EditorOperations => "Diagnostics and editor operations",
            Layer::ShellAdapters => "Shell-specific extraction adapters",
            Layer::DifferentialOracle => "Trusted differential-oracle coverage",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Layer::ParserBody => {
                "The parser accepts the source body Perl would hand it for this form. This is a syntax claim about the body text only: it does not decode the switch, synthesize the implicit loop, or supply interpreter setup."
            }
            Layer::StructuredArgv => {
                "A command line is decoded into typed arguments: switch clusters, switch-attached values, repeated source fragments, the `--` terminator, and residual operands."
            }
            Layer::SourceComposition => {
                "Decoded fragments are composed into one analyzable source unit whose offsets map back to original command coordinates."
            }
            Layer::ImplicitRuntime => {
                "Switch-implied runtime structure (the `-n`/`-p` read loop, `-a` autosplit, `-l` record handling) is modeled as semantic context rather than left to the reader."
            }
            Layer::CompileTimeContext => {
                "Switch-implied compile-time context — `-M`/`-m` imports, `-I` include roots, and `-E` feature enablement — informs name resolution and diagnostics."
            }
            Layer::EditorOperations => {
                "Diagnostics and each LSP/editor operation answer against command-line source at original command coordinates."
            }
            Layer::ShellAdapters => {
                "A host shell's quoting and tokenization rules are decoded before argv decoding. Each shell is a separate adapter; none is implied by core structured-argv support."
            }
            Layer::DifferentialOracle => {
                "A trusted oracle compares this toolchain's understanding against real `perl` behavior for the same command line."
            }
        }
    }
}

/// Support classification. `Partial` must always name the missing layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Support {
    Supported,
    Partial,
    Unsupported,
    NotApplicable,
}

impl Support {
    fn as_str(self) -> &'static str {
        match self {
            Support::Supported => "supported",
            Support::Partial => "partial",
            Support::Unsupported => "unsupported",
            Support::NotApplicable => "not_applicable",
        }
    }

    /// Whether this status asserts that the capability is available to a user.
    fn is_earned(self) -> bool {
        matches!(self, Support::Supported | Support::Partial)
    }
}

/// One declared capability row.
#[derive(Clone, Copy)]
struct CapabilityRow {
    layer: Layer,
    /// The switch or command-line form this row classifies.
    subject: &'static str,
    status: Support,
    /// Corpus fixture identifiers proving the capability is earned.
    evidence: &'static [&'static str],
    /// Corpus fixture identifiers proving where the capability stops. A
    /// boundary control never earns support; it makes an absence checkable.
    boundary_controls: &'static [&'static str],
    /// For `partial` only: the exact layer that is missing.
    missing_layer: &'static str,
    /// The command or API a user can actually invoke for an earned capability.
    invocable: &'static str,
    notes: &'static str,
}

/// The command-line forms that must each carry an explicit row.
const REQUIRED_SUBJECTS: &[&str] = &[
    "-e",
    "-E",
    "repeated source fragments",
    "-n",
    "-p",
    "-a",
    "-F",
    "-l",
    "-0 / -g",
    "-I",
    "-M",
    "-m",
    "--",
    "explicit script file",
    "stdin program",
];

/// Shell adapters that must each be classified independently.
const REQUIRED_SHELL_ADAPTERS: &[&str] = &["POSIX shell", "PowerShell", "cmd.exe"];

/// Subjects allowed to be `not_applicable`, because another surface owns them.
///
/// Every other status is anchored to the corpus: an earned row must cite
/// evidence, and an unearned one may cite boundary controls. `not_applicable`
/// is anchored to nothing, so without this list it would be the cheapest way to
/// retire an inconvenient gap. Adding a subject here is a deliberate edit that
/// has to name its real owner in the row's note.
const NOT_APPLICABLE_SUBJECTS: &[&str] = &["explicit script file"];

const ROWS: &[CapabilityRow] = &[
    // ---- Layer 1: parser-body acceptance -------------------------------
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-e",
        status: Support::Supported,
        evidence: &[
            "e_print_literal",
            "e_grep_diamond_input",
            "e_map_diamond_input",
            "e_explicit_diamond_loop",
            "e_sort_diamond_with_for_modifier",
            "e_parenthesized_split_slice",
            "e_printf_special_variables",
            "positive_idioms_have_typed_ast_hir_and_source_range_proof",
        ],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: CORPUS_COMMAND,
        notes: "Single-fragment program bodies parse cleanly with typed AST/HIR and source ranges. The `-e` switch itself is not decoded; that is layer 2.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-n",
        status: Support::Supported,
        evidence: &[
            "ne_implicit_topic_match",
            "ne_skip_blank_lines",
            "ne_end_phase_counter",
            "ne_argv_and_input_line_number",
            "ne_begin_phase_input_record_separator",
            "ne_capture_group",
        ],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: CORPUS_COMMAND,
        notes: "Bodies that rely on `$_`, `next`, `$ARGV`, `$.`, and phase blocks parse. The implicit read loop is not synthesized; that is layer 4.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-p",
        status: Support::Supported,
        evidence: &[
            "pe_implicit_topic_substitution",
            "pe_implicit_topic_transliteration",
            "pe_trim_whitespace",
            "positive_idioms_have_typed_ast_hir_and_source_range_proof",
        ],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: CORPUS_COMMAND,
        notes: "Substitution and transliteration bodies lower to typed HIR. The implicit print-back loop is not synthesized; that is layer 4.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-a",
        status: Support::Supported,
        evidence: &["lane_first_autosplit_field", "lane_join_autosplit_fields"],
        boundary_controls: &[],
        missing_layer: "",
        invocable: CORPUS_COMMAND,
        notes: "Both cited bodies read `@F`, the variable autosplit populates, so the evidence is specific to this switch rather than incidental. Autosplit inserts no source text; whether `@F` is actually populated is layer 4.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-l",
        status: Support::Partial,
        evidence: &["lane_first_autosplit_field", "lane_join_autosplit_fields"],
        boundary_controls: &[],
        missing_layer: "switch-specific evidence: no cited body contains anything `-l` changes, so acceptance here is incidental to the `-lane` bundle rather than proof about record separators",
        invocable: CORPUS_COMMAND,
        notes: "The cited bodies arrive through the `-lane` bundle and parse, but unlike `-a` and its `@F`, nothing in them is specific to record-separator handling. Promoting this row needs a body whose parse depends on `-l`.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-E",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No corpus case supplies an `-E` body, so feature-enabled constructs such as `say` carry no command-line parser-body evidence here.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-F",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No corpus case declares `-F`. The autosplit pattern lives in argv, not in the body, so evidence must come from layer 2.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-0 / -g",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No corpus case declares `-0` or `-g`. A body that sets `$/` directly is ordinary Perl and does not evidence the switch.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-I",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "Include roots are argv values, not body syntax. No corpus case declares `-I`.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-M",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "Import requests are argv values, not body syntax. No corpus case declares `-M`.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "-m",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No-import module requests are argv values, not body syntax. No corpus case declares `-m`.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "repeated source fragments",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: "",
        notes: "Repeated `-e` fragments join with newlines. The corpus asserts single-line inputs and keeps a multiline negative control, so multi-fragment composition is an excluded boundary rather than an untested gap.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "--",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: "",
        notes: "The argv terminator never reaches the body. The option-contamination control shows raw switch text parsing as an ordinary unary expression rather than being recognized.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "explicit script file",
        status: Support::NotApplicable,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "A named script is ordinary file parsing owned by the general parser corpus and generated parser status, not by the command-line lane.",
    },
    CapabilityRow {
        layer: Layer::ParserBody,
        subject: "stdin program",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "Reading a program from stdin (`perl -`) has no corpus case and no ingestion path in this lane.",
    },
    // ---- Layer 2: structured-argv decoding -----------------------------
    CapabilityRow {
        layer: Layer::StructuredArgv,
        subject: "whole layer",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: "",
        notes: "No switch decoder exists in the workspace. The option-contamination control proves the boundary directly: `-ne print;` parses as a unary expression on `ne`, which is what a parser without argv decoding must do.",
    },
    // ---- Layer 3: source composition and provenance --------------------
    CapabilityRow {
        layer: Layer::SourceComposition,
        subject: "whole layer",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: "",
        notes: "Nothing composes fragments or maps offsets back to command coordinates. Corpus ranges are offsets into a single body string, which is not command provenance.",
    },
    // ---- Layer 4: implicit runtime/loop context ------------------------
    CapabilityRow {
        layer: Layer::ImplicitRuntime,
        subject: "whole layer",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &["negative_controls_keep_context_errors_and_boundaries_visible"],
        missing_layer: "",
        invocable: "",
        notes: "The implicit `-n`/`-p` loop is never synthesized. The explicit-loop control asserts a written loop stays a real `Foreach` with a typed `LoopShell`, so an implicit wrapper cannot be mistaken for one.",
    },
    // ---- Layer 5: compile-time feature/module/include context ----------
    CapabilityRow {
        layer: Layer::CompileTimeContext,
        subject: "whole layer",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "`-M`, `-m`, `-I`, and `-E` supply no name-resolution or feature context. Body `BEGIN` blocks parse, but a parsed phase block is not switch-derived compile-time context.",
    },
    // ---- Layer 6: diagnostics and editor operations --------------------
    CapabilityRow {
        layer: Layer::EditorOperations,
        subject: "whole layer",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No LSP or editor operation accepts a command line as a document. Diagnostics, hover, completion, and navigation over command-line source are all unearned.",
    },
    // ---- Layer 7: shell-specific extraction adapters -------------------
    CapabilityRow {
        layer: Layer::ShellAdapters,
        subject: "POSIX shell",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No adapter decodes POSIX shell quoting. Layer 2 remaining unsupported means there is nothing for an adapter to feed.",
    },
    CapabilityRow {
        layer: Layer::ShellAdapters,
        subject: "PowerShell",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "PowerShell quoting differs from POSIX and needs its own adapter and evidence. It is never implied by POSIX or by structured-argv support.",
    },
    CapabilityRow {
        layer: Layer::ShellAdapters,
        subject: "cmd.exe",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "cmd.exe quoting differs from both POSIX and PowerShell and needs its own adapter and evidence.",
    },
    // ---- Layer 8: trusted differential-oracle coverage -----------------
    CapabilityRow {
        layer: Layer::DifferentialOracle,
        subject: "whole layer",
        status: Support::Unsupported,
        evidence: &[],
        boundary_controls: &[],
        missing_layer: "",
        invocable: "",
        notes: "No command line is executed against real `perl` and compared. The corpus is a parser-side contract and makes no behavioral agreement claim.",
    },
];

/// Fixture evidence mechanically extracted from the conformance corpus.
#[derive(Debug, Default, PartialEq, Eq)]
struct CorpusEvidence {
    /// Macro-declared cases, mapped to the switch bundle they represent.
    switch_cases: BTreeMap<String, String>,
    /// Free `#[test]` functions in the corpus.
    proof_tests: BTreeSet<String>,
}

impl CorpusEvidence {
    fn contains(&self, id: &str) -> bool {
        self.switch_cases.contains_key(id) || self.proof_tests.contains(id)
    }

    fn total(&self) -> usize {
        self.switch_cases.len() + self.proof_tests.len()
    }

    /// Whether `id` is a macro case whose bundle declares `switch`.
    fn case_declares_switch(&self, id: &str, switch: &str) -> bool {
        self.switch_cases.get(id).is_some_and(|bundle| bundle_declares(bundle, switch))
    }
}

/// Whether a switch bundle such as `-lane` declares a switch such as `-l`.
fn bundle_declares(bundle: &str, switch: &str) -> bool {
    let Some(wanted) = switch.strip_prefix('-') else {
        return false;
    };
    let mut chars = wanted.chars();
    let (Some(flag), None) = (chars.next(), chars.next()) else {
        // Only single-flag switches are addressable in a cluster.
        return false;
    };
    bundle_flags(bundle).contains(&flag)
}

/// Perl switches whose value is attached directly to the flag, so everything
/// after them in a cluster is that value rather than further flags.
const VALUE_ATTACHED_FLAGS: &[char] = &['M', 'm', 'I', 'F', '0', 'i', 'D'];

/// The flags a switch bundle actually declares.
///
/// Substring membership would be wrong: `-Mfeature` would "declare" `-e` and
/// `-a` because those letters occur in the module name. A value-attached flag
/// therefore ends the cluster and swallows the rest.
fn bundle_flags(bundle: &str) -> Vec<char> {
    let Some(cluster) = bundle.strip_prefix('-') else {
        return Vec::new();
    };
    let mut flags = Vec::new();
    for flag in cluster.chars() {
        flags.push(flag);
        if VALUE_ATTACHED_FLAGS.contains(&flag) {
            break;
        }
    }
    flags
}

/// The switch tokens a row subject names, derived from the subject itself.
///
/// The subject is the single source of truth for which switch a row is about.
/// A separate declared field would be a bypass: leaving it blank would silently
/// skip the evidence binding while the row still reads as a switch claim.
///
/// `"-e"` yields `["-e"]` and `"-0 / -g"` yields `["-0", "-g"]`. Subjects that
/// name no switch — `"--"`, `"whole layer"`, `"PowerShell"`, `"stdin program"` —
/// yield nothing.
fn subject_switches(subject: &str) -> Vec<&str> {
    subject
        .split('/')
        .map(str::trim)
        .filter(|token| {
            // A switch token is a dash plus exactly one flag character. `--` is
            // the argv terminator, not a flag.
            token.starts_with('-')
                && token.chars().count() == 2
                && token.chars().nth(1).is_some_and(|flag| flag != '-')
        })
        .collect()
}

/// A byte mask over `source` marking bytes that are real code — outside line
/// and block comments, string and raw-string literals, and character literals.
///
/// Evidence must come from fixtures that actually run. A commented-out or
/// quoted `command_line_oneliner!` reads exactly like a live one to a plain
/// substring scan, so without this mask a deleted fixture could still be cited
/// to earn support.
fn code_mask(source: &str) -> Vec<bool> {
    let bytes = source.as_bytes();
    let mut mask = vec![false; bytes.len()];
    let mut i = 0usize;

    while i < bytes.len() {
        let byte = bytes[i];

        if byte == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        if byte == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let mut depth = 1usize;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }

        // Raw string: `r`, then any number of `#`, then `"`. The `r` must start
        // a token, or it is just the tail of an identifier.
        if byte == b'r' && !bytes.get(i.wrapping_sub(1)).is_some_and(|b| is_identifier_byte(*b)) {
            let mut cursor = i + 1;
            let mut hashes = 0usize;
            while bytes.get(cursor) == Some(&b'#') {
                hashes += 1;
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'"') {
                i = skip_raw_string(bytes, cursor + 1, hashes);
                continue;
            }
        }

        if byte == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // A `'` that does not open a literal is a lifetime: ordinary code.
        if byte == b'\''
            && let Some(end) = char_literal_end(source, i)
        {
            i = end;
            continue;
        }

        mask[i] = true;
        i += 1;
    }

    mask
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Byte index just past a raw string whose body starts at `start`.
fn skip_raw_string(bytes: &[u8], start: usize, hashes: usize) -> usize {
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let closing = i + 1 + hashes;
            if bytes.get(i + 1..closing).is_some_and(|tail| tail.iter().all(|b| *b == b'#')) {
                return closing;
            }
        }
        i += 1;
    }
    bytes.len()
}

/// Byte index just past a character literal starting at `quote`, or `None` when
/// the quote opens a lifetime instead.
fn char_literal_end(source: &str, quote: usize) -> Option<usize> {
    let rest = source.get(quote + 1..)?;
    if rest.starts_with('\\') {
        // Escaped: the literal ends at the next quote.
        let close = rest.find('\'')?;
        return Some(quote + 1 + close + 1);
    }
    let first = rest.chars().next()?;
    let after = quote + 1 + first.len_utf8();
    if source.as_bytes().get(after) == Some(&b'\'') { Some(after + 1) } else { None }
}

/// The contiguous block of attribute lines directly above the item containing
/// `index`, used to see what is attached to a fixture before it is counted.
fn preceding_attributes(source: &str, index: usize) -> String {
    const MAX_LINES: usize = 12;
    let Some(head) = source.get(..index) else {
        return String::new();
    };
    let mut lines: Vec<&str> = head.lines().collect();
    // Drop the partial line the item sits on. When the item begins exactly at a
    // line boundary there is no partial line to drop, and dropping one anyway
    // would discard the very attribute being looked for.
    if !head.ends_with('\n') {
        lines.pop();
    }
    let mut block: Vec<&str> = Vec::new();
    for line in lines.iter().rev().take(MAX_LINES) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("#[") || trimmed.starts_with("#!") {
            block.push(trimmed);
            continue;
        }
        break;
    }
    block.join("\n")
}

/// Whether attribute text disables the item or stops it from running.
///
/// A `cfg`-disabled fixture is not compiled and a `cfg_attr(..., ignore)` one is
/// skipped, so neither is executed by the corpus command. Counting either as
/// evidence would let a row claim support from a fixture that never runs.
fn suppresses_execution(attributes: &str) -> bool {
    attributes.contains("#[cfg(")
        || attributes.contains("#[cfg_attr(")
        || attributes.contains("#[ignore")
}

/// Extract corpus evidence from the conformance corpus source text.
fn extract_corpus_evidence(source: &str) -> CorpusEvidence {
    let mut evidence = CorpusEvidence::default();
    let code = code_mask(source);

    for (index, _) in source.match_indices("command_line_oneliner!(") {
        if !code.get(index).copied().unwrap_or(false) {
            continue;
        }
        if suppresses_execution(&preceding_attributes(source, index)) {
            continue;
        }
        let Some(rest) = source.get(index + "command_line_oneliner!(".len()..) else {
            continue;
        };
        let Some(comma) = rest.find(',') else {
            continue;
        };
        let Some(name) = rest.get(..comma).map(str::trim) else {
            continue;
        };
        if name.is_empty() || !is_identifier(name) {
            continue;
        }
        let Some(after_name) = rest.get(comma + 1..) else {
            continue;
        };
        let Some(open) = after_name.find('"') else {
            continue;
        };
        let Some(tail) = after_name.get(open + 1..) else {
            continue;
        };
        let Some(close) = tail.find('"') else {
            continue;
        };
        let Some(bundle) = tail.get(..close) else {
            continue;
        };
        evidence.switch_cases.insert(name.to_string(), bundle.to_string());
    }

    // Free `#[test]` functions: the corpus's typed-proof and negative-control
    // targets. Macro-generated cases are already captured above.
    for (index, _) in source.match_indices("#[test]") {
        if !code.get(index).copied().unwrap_or(false) {
            continue;
        }
        let Some(rest) = source.get(index..) else {
            continue;
        };
        let Some(fn_offset) = rest.find("fn ") else {
            continue;
        };
        // Only accept the declaration that actually follows the attribute.
        // Anything between them must be whitespace or further attributes, so a
        // `#[test]` with no declaration of its own cannot borrow the name of a
        // later function.
        let Some(gap) = rest.get("#[test]".len()..fn_offset) else {
            continue;
        };
        if gap.contains(['{', '}', ';']) {
            continue;
        }
        // A test that is ignored, cfg-disabled, or conditionally ignored does
        // not run, so it cannot evidence anything. Attributes may sit on either
        // side of `#[test]`, so both neighbourhoods are inspected.
        if suppresses_execution(gap) || suppresses_execution(&preceding_attributes(source, index)) {
            continue;
        }
        let Some(after_fn) = rest.get(fn_offset + "fn ".len()..) else {
            continue;
        };
        let name: String =
            after_fn.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if name.is_empty() {
            continue;
        }
        if !evidence.switch_cases.contains_key(&name) {
            evidence.proof_tests.insert(name);
        }
    }

    evidence
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !value.starts_with(|c: char| c.is_ascii_digit())
}

/// Fail closed when a declared row is not backed by the evidence it claims.
fn validate(rows: &[CapabilityRow], evidence: &CorpusEvidence) -> Result<()> {
    let mut seen: BTreeSet<(usize, &str)> = BTreeSet::new();

    for row in rows {
        let subject = row.subject;
        let layer = row.layer.title();

        if !seen.insert((row.layer.ordinal(), subject)) {
            bail!("duplicate capability row for {layer} / {subject}");
        }

        // A cited identifier must exist in the corpus, whether it is offered as
        // support evidence or as a boundary control.
        for id in row.evidence.iter().chain(row.boundary_controls.iter()) {
            if !evidence.contains(id) {
                bail!(
                    "{layer} / {subject} cites fixture `{id}`, which does not exist in {CORPUS_PATH}"
                );
            }
        }

        if row.status.is_earned() {
            if row.evidence.is_empty() {
                bail!(
                    "{layer} / {subject} claims `{}` with no fixture evidence",
                    row.status.as_str()
                );
            }
            if row.invocable.is_empty() {
                bail!(
                    "{layer} / {subject} claims `{}` without naming an invocable command or API",
                    row.status.as_str()
                );
            }
        } else {
            if !row.evidence.is_empty() {
                bail!(
                    "{layer} / {subject} is `{}` but cites support evidence; an unearned row may cite boundary controls only",
                    row.status.as_str()
                );
            }
            if !row.invocable.is_empty() {
                bail!(
                    "{layer} / {subject} is `{}` but names an invocable command",
                    row.status.as_str()
                );
            }
        }

        if row.status == Support::Partial {
            if row.missing_layer.is_empty() {
                bail!("{layer} / {subject} is `partial` without naming the missing layer");
            }
        } else if !row.missing_layer.is_empty() {
            bail!("{layer} / {subject} names a missing layer but is not `partial`");
        }

        if row.notes.is_empty() {
            bail!("{layer} / {subject} has no boundary note");
        }

        if row.status == Support::NotApplicable && !NOT_APPLICABLE_SUBJECTS.contains(&subject) {
            bail!(
                "{layer} / {subject} is `not_applicable` but is not a declared out-of-lane \
                 subject; an unowned gap is `unsupported`"
            );
        }

        // Parser-body evidence is the only evidence this corpus can supply.
        // Refusing it elsewhere is what stops parser-only proof from being
        // promoted into an end-to-end support claim.
        if row.layer != Layer::ParserBody && !row.evidence.is_empty() {
            bail!(
                "{layer} / {subject} cites parser-body corpus evidence for a non-parser layer; \
                 parser-only proof cannot earn a higher layer"
            );
        }

        // The corpus can only evidence switch-shaped subjects, so an earned
        // parser-body row must name a switch and cite a case declaring it.
        // Deriving the switch from the subject leaves no field to blank out.
        if row.layer == Layer::ParserBody && row.status.is_earned() {
            let switches = subject_switches(subject);
            if switches.is_empty() {
                bail!(
                    "{layer} / {subject} claims `{}` but its subject names no switch the corpus can evidence",
                    row.status.as_str()
                );
            }
            // Every switch the subject names needs its own declaring fixture.
            // A combined subject such as `-0 / -g` claims both, so evidence for
            // one must not publish support for the other.
            for switch in &switches {
                let bound = row.evidence.iter().any(|id| evidence.case_declares_switch(id, switch));
                if !bound {
                    bail!(
                        "{layer} / {subject} claims `{}` but no cited corpus case declares `{}`",
                        row.status.as_str(),
                        switch
                    );
                }
            }
        }
    }

    // Pinned to the layer: a required form moved elsewhere would otherwise
    // satisfy completeness while silently losing its parser-body classification.
    for subject in REQUIRED_SUBJECTS {
        if !rows.iter().any(|row| row.layer == Layer::ParserBody && row.subject == *subject) {
            bail!("no parser-body capability row classifies the command-line form `{subject}`");
        }
    }

    for adapter in REQUIRED_SHELL_ADAPTERS {
        if !rows.iter().any(|row| row.layer == Layer::ShellAdapters && row.subject == *adapter) {
            bail!("shell adapter `{adapter}` has no independent row");
        }
    }

    for layer in Layer::ALL {
        if !rows.iter().any(|row| row.layer == *layer) {
            bail!("capability layer `{}` has no row", layer.title());
        }
    }

    // Every corpus case must be accounted for, so growing the corpus without
    // revisiting the claim is a checkable drift rather than a silent gap.
    for case in evidence.switch_cases.keys() {
        let cited = rows.iter().any(|row| row.evidence.contains(&case.as_str()));
        if !cited {
            bail!(
                "corpus case `{case}` is not cited by any capability row; \
                 refresh {MATRIX_PATH} so the claim tracks the corpus"
            );
        }
    }

    Ok(())
}

/// Generate or check the matrix.
pub fn run(check: bool) -> Result<()> {
    let root = project_root()?;
    let corpus = fs::read_to_string(root.join(CORPUS_PATH))
        .with_context(|| format!("failed to read {CORPUS_PATH}"))?;
    let evidence = extract_corpus_evidence(&corpus);

    if evidence.switch_cases.is_empty() {
        bail!("no command-line corpus cases found in {CORPUS_PATH}");
    }

    validate(ROWS, &evidence)?;

    let path = root.join(MATRIX_PATH);
    let generated = render_matrix(ROWS, &evidence);

    if check {
        let existing =
            fs::read_to_string(&path).with_context(|| format!("failed to read {MATRIX_PATH}"))?;
        if normalize_newlines(&existing) != generated {
            bail!("{MATRIX_PATH} is stale; run `cargo xtask oneliner-capability-matrix`");
        }
        println!(
            "One-liner capability matrix is up to date: {} rows over {} fixtures",
            ROWS.len(),
            evidence.total()
        );
        return Ok(());
    }

    fs::write(&path, generated).with_context(|| format!("failed to write {MATRIX_PATH}"))?;
    println!("Wrote {MATRIX_PATH} with {} rows", ROWS.len());
    Ok(())
}

fn render_matrix(rows: &[CapabilityRow], evidence: &CorpusEvidence) -> String {
    let mut output = String::new();
    output.push_str("# Perl Command-Line Analysis Capability Matrix\n\n");
    output.push_str("Status: generated\n");
    output.push_str("Owner: perl-lsp maintainers\n");
    output.push_str("Generator: `cargo xtask oneliner-capability-matrix`\n");
    output.push_str("Check: `cargo xtask oneliner-capability-matrix --check`\n");
    output.push_str(&format!("Evidence source: [`{CORPUS_PATH}`](../../../{CORPUS_PATH})\n\n"));

    output.push_str("\"One-liner support\" is not one capability. This matrix separates it into eight layers so that accepting a program body is never reported as understanding a command line. Each row carries its own evidence: a row claiming `supported` or `partial` must cite fixtures that are live in the conformance corpus, and the generator fails instead of rendering a claim that has none.\n\n");
    output.push_str(&format!("Scope of the check, exactly: it proves each citation is real and reachable — present in the corpus as running code, not commented out, quoted, `#[ignore]`d, or disabled by `cfg`, and declaring every switch the row claims. It does not run the fixtures. Execution is `{CORPUS_COMMAND}`, which every earned row names.\n\n"));

    output.push_str("Support vocabulary:\n\n");
    output.push_str("- `supported`: earned for this layer, with cited fixture evidence and an invocable command.\n");
    output.push_str("- `partial`: earned in part; the row names the exact missing layer rather than a generic caveat.\n");
    output.push_str("- `unsupported`: not earned. A boundary control may still prove where the behavior stops.\n");
    output.push_str("- `not_applicable`: outside this lane, owned elsewhere.\n\n");

    output.push_str("Evidence is parser-side only. A fixture from the parser corpus can earn layer 1 and nothing above it, so parser-only proof cannot be promoted into an end-to-end support claim.\n\n");

    output.push_str("## Layer summary\n\n");
    output.push_str("| # | Layer | Earned rows | Total rows |\n");
    output.push_str("| --- | --- | --- | --- |\n");
    for layer in Layer::ALL {
        let layer_rows: Vec<&CapabilityRow> =
            rows.iter().filter(|row| row.layer == *layer).collect();
        let earned = layer_rows.iter().filter(|row| row.status.is_earned()).count();
        output.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            layer.ordinal(),
            escape_cell(layer.title()),
            earned,
            layer_rows.len()
        ));
    }
    output.push('\n');

    for layer in Layer::ALL {
        let layer_rows: Vec<&CapabilityRow> =
            rows.iter().filter(|row| row.layer == *layer).collect();
        if layer_rows.is_empty() {
            continue;
        }
        output.push_str(&format!("## {}. {}\n\n", layer.ordinal(), layer.title()));
        output.push_str(layer.description());
        output.push_str("\n\n");
        output.push_str("| Subject | Status | Missing layer | Evidence | Boundary control | Invocable | Notes |\n");
        output.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
        for row in layer_rows {
            output.push_str("| ");
            output.push_str(&escape_cell(row.subject));
            output.push_str(" | `");
            output.push_str(row.status.as_str());
            output.push_str("` | ");
            output.push_str(&escape_cell(dash_if_empty(row.missing_layer)));
            output.push_str(" | ");
            output.push_str(&escape_cell(&render_ids(row.evidence)));
            output.push_str(" | ");
            output.push_str(&escape_cell(&render_ids(row.boundary_controls)));
            output.push_str(" | ");
            output.push_str(&escape_cell(&render_invocable(row.invocable)));
            output.push_str(" | ");
            output.push_str(&escape_cell(row.notes));
            output.push_str(" |\n");
        }
        output.push('\n');
    }

    output.push_str("## Corpus fixtures\n\n");
    output.push_str(&format!(
        "{} fixtures are available as evidence: {} switch-bundle cases and {} typed-proof targets.\n\n",
        evidence.total(),
        evidence.switch_cases.len(),
        evidence.proof_tests.len()
    ));
    output.push_str("| Fixture | Declared switches |\n");
    output.push_str("| --- | --- |\n");
    for (name, bundle) in &evidence.switch_cases {
        output.push_str(&format!("| `{}` | `{}` |\n", escape_cell(name), escape_cell(bundle)));
    }
    for name in &evidence.proof_tests {
        output.push_str(&format!("| `{}` | typed proof |\n", escape_cell(name)));
    }

    output
}

fn render_ids(ids: &[&str]) -> String {
    if ids.is_empty() {
        return "—".to_string();
    }
    ids.iter().map(|id| format!("`{id}`")).collect::<Vec<_>>().join("; ")
}

fn render_invocable(value: &str) -> String {
    if value.is_empty() { "—".to_string() } else { format!("`{value}`") }
}

fn dash_if_empty(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}

fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORPUS_SAMPLE: &str = r####"
command_line_oneliner!(e_print_literal, "-e", r#"print "hello\n";"#);
command_line_oneliner!(ne_implicit_topic_match, "-ne", r#"print if /needle/;"#);
command_line_oneliner!(
    lane_first_autosplit_field,
    "-lane",
    r#"print $F[0];"#
);

#[test]
fn positive_idioms_have_typed_ast_hir_and_source_range_proof() -> TestResult {
    Ok(())
}

#[test]
fn negative_controls_keep_context_errors_and_boundaries_visible() -> TestResult {
    Ok(())
}
"####;

    fn sample_evidence() -> CorpusEvidence {
        extract_corpus_evidence(CORPUS_SAMPLE)
    }

    fn real_evidence() -> CorpusEvidence {
        let root = project_root().expect("project root");
        let corpus = fs::read_to_string(root.join(CORPUS_PATH)).expect("corpus readable");
        extract_corpus_evidence(&corpus)
    }

    fn base_row() -> CapabilityRow {
        CapabilityRow {
            layer: Layer::ParserBody,
            subject: "-e",
            status: Support::Supported,
            evidence: &["e_print_literal"],
            boundary_controls: &[],
            missing_layer: "",
            invocable: CORPUS_COMMAND,
            notes: "note",
        }
    }

    /// Rows that satisfy the structural completeness rules, so that a single
    /// mutated row is the only reason a negative-control case can fail.
    fn scaffold() -> Vec<CapabilityRow> {
        let mut rows = Vec::new();
        for subject in REQUIRED_SUBJECTS {
            rows.push(CapabilityRow {
                layer: Layer::ParserBody,
                subject,
                status: Support::Unsupported,
                evidence: &[],
                boundary_controls: &[],
                missing_layer: "",
                invocable: "",
                notes: "scaffold",
            });
        }
        for adapter in REQUIRED_SHELL_ADAPTERS {
            rows.push(CapabilityRow {
                layer: Layer::ShellAdapters,
                subject: adapter,
                status: Support::Unsupported,
                evidence: &[],
                boundary_controls: &[],
                missing_layer: "",
                invocable: "",
                notes: "scaffold",
            });
        }
        for layer in Layer::ALL {
            if *layer == Layer::ParserBody || *layer == Layer::ShellAdapters {
                continue;
            }
            rows.push(CapabilityRow {
                layer: *layer,
                subject: "whole layer",
                status: Support::Unsupported,
                evidence: &[],
                boundary_controls: &[],
                missing_layer: "",
                invocable: "",
                notes: "scaffold",
            });
        }
        // The scaffold must cite every corpus case or the drift rule fires for
        // an unrelated reason. `-e` carries that anchor because every sample
        // bundle (`-e`, `-ne`, `-lane`) declares the `e` flag.
        rows.retain(|row| !(row.layer == Layer::ParserBody && row.subject == "-e"));
        rows.push(CapabilityRow {
            layer: Layer::ParserBody,
            subject: "-e",
            status: Support::Supported,
            evidence: &["e_print_literal", "ne_implicit_topic_match", "lane_first_autosplit_field"],
            boundary_controls: &[],
            missing_layer: "",
            invocable: CORPUS_COMMAND,
            notes: "anchor",
        });
        rows
    }

    fn scaffold_with(row: CapabilityRow) -> Vec<CapabilityRow> {
        let mut rows = scaffold();
        rows.retain(|existing| !(existing.layer == row.layer && existing.subject == row.subject));
        rows.push(row);
        rows
    }

    fn expect_rejected(rows: &[CapabilityRow], fragment: &str) {
        let evidence = sample_evidence();
        match validate(rows, &evidence) {
            Ok(()) => panic!("expected validation to reject rows mentioning {fragment}"),
            Err(error) => {
                let message = error.to_string();
                assert!(
                    message.contains(fragment),
                    "error {message:?} did not mention {fragment:?}"
                );
            }
        }
    }

    // ---- corpus extraction ------------------------------------------------

    #[test]
    fn extraction_reads_switch_bundles_and_proof_targets() {
        let evidence = sample_evidence();
        assert_eq!(evidence.switch_cases.get("e_print_literal").map(String::as_str), Some("-e"));
        assert_eq!(
            evidence.switch_cases.get("ne_implicit_topic_match").map(String::as_str),
            Some("-ne")
        );
        assert_eq!(
            evidence.switch_cases.get("lane_first_autosplit_field").map(String::as_str),
            Some("-lane")
        );
        assert!(
            evidence
                .proof_tests
                .contains("negative_controls_keep_context_errors_and_boundaries_visible")
        );
        assert_eq!(evidence.total(), 5);
    }

    #[test]
    fn extraction_finds_the_committed_corpus() {
        let evidence = real_evidence();
        assert!(
            evidence.switch_cases.len() >= 18,
            "expected the committed corpus to declare at least 18 switch cases, found {}",
            evidence.switch_cases.len()
        );
        assert!(
            evidence
                .proof_tests
                .contains("positive_idioms_have_typed_ast_hir_and_source_range_proof")
        );
    }

    #[test]
    fn detached_test_attribute_does_not_borrow_a_later_function_name() {
        // The attribute here has no declaration of its own: a body closes
        // before the next `fn`. Extraction must skip it rather than reporting
        // `helper` as a proof target.
        let detached = "#[test]\nconst X: usize = { 1 };\nfn helper() {}\n";
        let evidence = extract_corpus_evidence(detached);
        assert!(
            evidence.proof_tests.is_empty(),
            "detached attribute borrowed a later name: {:?}",
            evidence.proof_tests
        );

        // The ordinary attached form still resolves, including through an
        // intervening attribute that does not suppress the run.
        let attached = "#[test]\n#[allow(clippy::needless_range_loop)]\nfn real_case() {}\n";
        let evidence = extract_corpus_evidence(attached);
        assert!(evidence.proof_tests.contains("real_case"));
    }

    #[test]
    fn bundle_membership_is_per_flag() {
        assert!(bundle_declares("-lane", "-l"));
        assert!(bundle_declares("-lane", "-a"));
        assert!(bundle_declares("-lane", "-n"));
        assert!(bundle_declares("-lane", "-e"));
        assert!(!bundle_declares("-lane", "-p"));
        assert!(!bundle_declares("-e", "-n"));
        assert!(!bundle_declares("-ne", "-M"));
        assert!(!bundle_declares("-e", ""));
    }

    // ---- the committed claim ---------------------------------------------

    #[test]
    fn committed_rows_validate_against_the_committed_corpus() {
        let evidence = real_evidence();
        validate(ROWS, &evidence).expect("committed rows must be backed by the committed corpus");
    }

    #[test]
    fn committed_matrix_earns_parser_body_only() {
        for row in ROWS {
            if row.layer != Layer::ParserBody {
                assert!(
                    !row.status.is_earned(),
                    "{} / {} claims {} without an earned layer above parser-body",
                    row.layer.title(),
                    row.subject,
                    row.status.as_str()
                );
            }
        }
    }

    #[test]
    fn shell_adapters_are_independent_of_structured_argv() {
        let argv_earned = ROWS
            .iter()
            .filter(|row| row.layer == Layer::StructuredArgv)
            .any(|row| row.status.is_earned());
        for adapter in REQUIRED_SHELL_ADAPTERS {
            let row = ROWS
                .iter()
                .find(|row| row.layer == Layer::ShellAdapters && row.subject == *adapter)
                .expect("adapter row present");
            assert!(
                !(row.status.is_earned() && !argv_earned),
                "{adapter} cannot be earned while structured-argv decoding is not"
            );
        }
    }

    // ---- negative controls -----------------------------------------------

    #[test]
    fn unknown_fixture_is_rejected() {
        let mut row = base_row();
        row.evidence = &["fixture_that_does_not_exist"];
        expect_rejected(&scaffold_with(row), "does not exist");
    }

    #[test]
    fn earned_status_without_evidence_is_rejected() {
        let mut row = base_row();
        row.evidence = &[];
        expect_rejected(&scaffold_with(row), "no fixture evidence");
    }

    #[test]
    fn unearned_status_carrying_support_evidence_is_rejected() {
        let mut row = base_row();
        row.status = Support::Unsupported;
        row.invocable = "";
        expect_rejected(&scaffold_with(row), "boundary controls only");
    }

    #[test]
    fn earned_status_without_invocable_command_is_rejected() {
        let mut row = base_row();
        row.invocable = "";
        expect_rejected(&scaffold_with(row), "invocable command");
    }

    #[test]
    fn partial_without_named_missing_layer_is_rejected() {
        let mut row = base_row();
        row.status = Support::Partial;
        row.missing_layer = "";
        expect_rejected(&scaffold_with(row), "without naming the missing layer");
    }

    #[test]
    fn non_partial_naming_a_missing_layer_is_rejected() {
        let mut row = base_row();
        row.missing_layer = "structured-argv decoding";
        expect_rejected(&scaffold_with(row), "not `partial`");
    }

    #[test]
    fn parser_evidence_cannot_earn_a_higher_layer() {
        let mut row = base_row();
        row.layer = Layer::EditorOperations;
        row.subject = "whole layer";
        expect_rejected(&scaffold_with(row), "cannot earn a higher layer");
    }

    #[test]
    fn switch_claim_unbacked_by_a_declaring_case_is_rejected() {
        let mut row = base_row();
        row.subject = "-M";
        // `e_print_literal` is a real fixture, but it declares `-e`, not `-M`.
        expect_rejected(&scaffold_with(row), "no cited corpus case declares");
    }

    /// The binding rule used to be gated on a separate declared `switch` field,
    /// so blanking that field let a switch-shaped row claim support on
    /// unrelated evidence. The subject is now the only source of truth, and a
    /// subject naming no switch cannot be earned at all.
    #[test]
    fn earned_parser_row_whose_subject_names_no_switch_is_rejected() {
        for subject in ["--", "whole layer", "repeated source fragments", "stdin program", ""] {
            let mut row = base_row();
            row.subject = subject;
            expect_rejected(&scaffold_with(row), "names no switch");
        }
    }

    /// A cfg-disabled fixture is never compiled, so the corpus command never
    /// runs it and it cannot be evidence.
    #[test]
    fn cfg_disabled_fixtures_are_not_evidence() {
        let disabled_case = concat!(
            "#[cfg(feature = \"unfinished\")]\n",
            "command_line_oneliner!(ghost_cfg, \"-e\", \"print 1;\");\n"
        );
        let evidence = extract_corpus_evidence(disabled_case);
        assert!(
            evidence.switch_cases.is_empty(),
            "cfg-disabled macro case was captured: {:?}",
            evidence.switch_cases
        );

        // `cfg_attr(..., ignore)` on either side of `#[test]` also suppresses.
        for source in [
            "#[test]\n#[cfg_attr(windows, ignore)]\nfn conditionally_ignored() {}\n",
            "#[cfg_attr(windows, ignore)]\n#[test]\nfn conditionally_ignored() {}\n",
            "#[cfg(target_os = \"linux\")]\n#[test]\nfn platform_only() {}\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.proof_tests.is_empty(),
                "suppressed test was captured from {source:?}: {:?}",
                evidence.proof_tests
            );
        }
    }

    /// Pinning required forms to their layer stops a form from being moved out
    /// of parser-body while completeness still reads as satisfied.
    #[test]
    fn required_form_in_the_wrong_layer_is_rejected() {
        let mut rows = scaffold();
        for row in rows.iter_mut() {
            if row.layer == Layer::ParserBody && row.subject == "-M" {
                row.layer = Layer::DifferentialOracle;
            }
        }
        expect_rejected(&rows, "no parser-body capability row classifies");
    }

    /// A combined subject claims every switch it names, so evidence for one
    /// must not publish support for the other.
    #[test]
    fn combined_switch_row_needs_evidence_for_each_switch() {
        let mut row = base_row();
        row.subject = "-0 / -g";
        // A fixture declaring `-e` covers neither; even one that covered `-0`
        // alone must not carry `-g`.
        row.evidence = &["e_print_literal"];
        expect_rejected(&scaffold_with(row), "no cited corpus case declares");

        // Both halves present is what the rule actually requires.
        assert_eq!(subject_switches("-0 / -g"), vec!["-0", "-g"]);
    }

    /// An ignored test does not run, so citing it must not earn support.
    #[test]
    fn ignored_test_fn_is_not_evidence() {
        let source = "#[test]\n#[ignore = \"flaky\"]\nfn broken_but_ignored() {}\n";
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.proof_tests.is_empty(),
            "an ignored test was captured as evidence: {:?}",
            evidence.proof_tests
        );
    }

    /// Substring membership would let a switch value masquerade as flags.
    #[test]
    fn value_attached_switches_do_not_declare_letters_of_their_value() {
        assert!(bundle_declares("-Mfeature", "-M"));
        assert!(!bundle_declares("-Mfeature", "-e"), "module name letters became flags");
        assert!(!bundle_declares("-Mfeature", "-a"));
        assert!(bundle_declares("-Ilib", "-I"));
        assert!(!bundle_declares("-Ilib", "-l"), "include path letters became flags");
        assert!(!bundle_declares("-0777", "-7"));
        // Pure clusters are unaffected.
        assert_eq!(bundle_flags("-lane"), vec!['l', 'a', 'n', 'e']);
        assert_eq!(bundle_flags("-Mfeature"), vec!['M']);
    }

    #[test]
    fn undeclared_not_applicable_subject_is_rejected() {
        let mut row = base_row();
        row.subject = "-M";
        row.status = Support::NotApplicable;
        row.evidence = &[];
        row.invocable = "";
        expect_rejected(&scaffold_with(row), "not a declared out-of-lane subject");
    }

    #[test]
    fn subject_is_the_only_switch_authority() {
        assert_eq!(subject_switches("-e"), vec!["-e"]);
        assert_eq!(subject_switches("-0 / -g"), vec!["-0", "-g"]);
        assert!(subject_switches("--").is_empty());
        assert!(subject_switches("whole layer").is_empty());
        assert!(subject_switches("PowerShell").is_empty());
        assert!(subject_switches("stdin program").is_empty());
        assert!(subject_switches("").is_empty());
    }

    #[test]
    fn missing_required_form_is_rejected() {
        let rows: Vec<CapabilityRow> =
            scaffold().into_iter().filter(|row| row.subject != "-M").collect();
        expect_rejected(&rows, "`-M`");
    }

    #[test]
    fn missing_shell_adapter_is_rejected() {
        let rows: Vec<CapabilityRow> = scaffold()
            .into_iter()
            .filter(|row| !(row.layer == Layer::ShellAdapters && row.subject == "PowerShell"))
            .collect();
        expect_rejected(&rows, "PowerShell");
    }

    #[test]
    fn duplicate_row_is_rejected() {
        let mut rows = scaffold();
        rows.push(base_row());
        rows.push(base_row());
        expect_rejected(&rows, "duplicate capability row");
    }

    #[test]
    fn uncited_corpus_case_is_rejected() {
        // Drop the anchor's evidence so no row accounts for the corpus cases.
        let mut row = base_row();
        row.status = Support::Unsupported;
        row.evidence = &[];
        row.invocable = "";
        expect_rejected(&scaffold_with(row), "not cited by any capability row");
    }

    #[test]
    fn row_without_a_boundary_note_is_rejected() {
        let mut row = base_row();
        row.notes = "";
        expect_rejected(&scaffold_with(row), "no boundary note");
    }

    // ---- rendering --------------------------------------------------------

    #[test]
    fn render_is_deterministic() {
        let evidence = real_evidence();
        assert_eq!(render_matrix(ROWS, &evidence), render_matrix(ROWS, &evidence));
    }

    #[test]
    fn render_covers_every_layer_and_row() {
        let evidence = real_evidence();
        let rendered = render_matrix(ROWS, &evidence);
        for layer in Layer::ALL {
            assert!(rendered.contains(layer.title()), "missing layer {}", layer.title());
        }
        for subject in REQUIRED_SUBJECTS {
            assert!(rendered.contains(subject), "missing required form {subject}");
        }
    }

    #[test]
    fn table_cells_escape_pipe_characters() {
        assert_eq!(escape_cell("a|b"), "a\\|b");
    }

    // ---- PROBE: commented-out fixtures should not become evidence ---------

    #[test]
    fn commented_out_macro_call_is_not_evidence() {
        let source = "// command_line_oneliner!(ghost_case, \"-e\", \"print 1;\");\n";
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.is_empty(),
            "commented-out macro call was captured: {:?}",
            evidence.switch_cases
        );
    }

    #[test]
    fn block_commented_macro_call_is_not_evidence() {
        let source = "/* command_line_oneliner!(ghost_case, \"-e\", \"x\"); */\n";
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.is_empty(),
            "block-commented macro call was captured: {:?}",
            evidence.switch_cases
        );
    }

    #[test]
    fn commented_out_test_fn_is_not_evidence() {
        let source = "// #[test]\n// fn ghost_proof_test() {}\n";
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.proof_tests.is_empty(),
            "commented-out #[test] fn was captured: {:?}",
            evidence.proof_tests
        );
    }

    #[test]
    fn string_literal_macro_call_is_not_evidence() {
        let source = "let s = \"command_line_oneliner!(ghost2, \\\"-e\\\", \\\"x\\\");\";\n";
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.is_empty(),
            "string-literal macro call was captured: {:?}",
            evidence.switch_cases
        );
    }

    #[test]
    fn raw_string_macro_call_is_not_evidence() {
        let source = "let s = r##\"command_line_oneliner!(ghost3, \"-e\", \"x\");\"##;\n";
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.is_empty(),
            "raw-string macro call was captured: {:?}",
            evidence.switch_cases
        );
    }

    /// A commented-out fixture must not merely be uncounted: citing it has to
    /// fail, because a deleted fixture proves nothing.
    #[test]
    fn citing_a_commented_out_fixture_is_rejected() {
        let corpus = format!(
            "{CORPUS_SAMPLE}\n// command_line_oneliner!(ghost_case, \"-e\", \"print 1;\");\n"
        );
        let evidence = extract_corpus_evidence(&corpus);
        assert!(!evidence.contains("ghost_case"));

        let mut row = base_row();
        row.evidence = &["ghost_case"];
        match validate(&scaffold_with(row), &evidence) {
            Ok(()) => panic!("a commented-out fixture earned support"),
            Err(error) => assert!(error.to_string().contains("does not exist")),
        }
    }

    /// Code context must not be over-trimmed: the live corpus still resolves,
    /// including its raw-string bodies and character literals.
    #[test]
    fn code_mask_keeps_the_live_corpus_visible() {
        let evidence = real_evidence();
        assert!(evidence.switch_cases.len() >= 18, "live cases lost: {:?}", evidence.switch_cases);
        assert!(
            evidence
                .proof_tests
                .contains("positive_idioms_have_typed_ast_hir_and_source_range_proof")
        );
    }
}
