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
//! # Instrument boundary
//!
//! Evidence discovery is a **Rust parse** via `syn::parse_file` plus an item
//! walk (#14346). Each `Item::Macro` for `command_line_oneliner!` is extracted
//! directly from the parsed token tree; `#[test]` functions are found via
//! `Item::Fn`. Comments, literals, attribute spacing, and qualifier placement
//! are all handled by the parser, not a bespoke lexical mask.
//!
//! The boundary is stated rather than hidden because its failure mode is quiet:
//! a construct the extractor misreads becomes a fixture that is counted without
//! running, which is the exact dishonesty this check exists to prevent. Where
//! reachability cannot be established — a fixture inside a module with a
//! suppressing outer attribute — extraction refuses instead of guessing.
//!
//! `macro_rules!` definition bodies are skipped: the names inside are templates,
//! not citable fixtures. Top-level invocations of macros defined by
//! `macro_rules!` in the same file are refused entirely, since their generated
//! names are not visible at parse time. Inner module attributes (`#![cfg(...)]`)
//! suppress their module's contents during the walk rather than causing a
//! refusal, distinguishing them from outer `#[cfg(...)]` on a `mod` item.
//!
//! What this check does and does not establish, stated exactly: it proves the
//! citation is real and reachable — the fixture exists in the corpus as running
//! code, is not commented out, quoted, `#[ignore]`d, or disabled by `cfg`, and
//! declares every switch the row claims. It does **not** execute the fixture; whether it passes is
//! established by running [`CORPUS_COMMAND`], which every earned row names so a
//! reader can run it. The two together are the guarantee; neither alone is.
//!
//! In CI the executing half is `unit_routed_full`, the only pr_fast gate that
//! runs `tests/` directories — and it is scope-aware, so it executes the corpus
//! only when the diff touches `perl-parser-core`. That is weaker than it
//! sounds only in one direction: a citation can go stale solely because the
//! corpus or the code under it changed, and either change is what routes the
//! gate. A cited fixture broken purely by a dependency, with `perl-parser-core`
//! untouched, is the residual gap, and it is the repository-wide integration
//! routing gap recorded on that gate (#4642, corrected in #11694), not one this
//! check introduces.

use crate::utils::project_root;
use color_eyre::eyre::{Context, Result, bail};
use proc_macro2::TokenTree;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use syn::{AttrStyle, Attribute, Item, Meta};

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

    /// The layer's position in the stack, used for section numbering and to
    /// keep the rendered order stable regardless of row declaration order.
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

    /// The layer's heading in the generated page.
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

    /// What earning this layer means, stated so a reader cannot mistake one
    /// layer's evidence for another's.
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
    /// The status token as it appears in the generated page and in refusals.
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
            "e_while_diamond_modifier",
            "named_idioms_share_one_structural_table",
        ],
        boundary_controls: &[
            "negative_controls_keep_context_errors_and_boundaries_visible",
            "structurally_wrong_array_slice_fails_element_assertion",
            "hash_subscript_neighbor_does_not_satisfy_array_element",
            "unrelated_subscript_does_not_satisfy_autosplit_field",
            "block_if_neighbor_does_not_satisfy_postfix_match",
            "grep_list_neighbor_does_not_satisfy_diamond_input",
            "y_transliteration_neighbor_does_not_satisfy_tr_exact_span",
            "parenthesized_split_index_keeps_kind_payload_and_children_not_leaky_span",
        ],
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
            "ne_bare_capture_variable",
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
            "named_idioms_share_one_structural_table",
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
        notes: "A switch decoder now exists as a library API — `perl_parser_core::command_line::decode` (#13726), proven by `crates/perl-parser-core/tests/command_line_argv_decoding.rs` — but this row stays `unsupported`, for two separate reasons. No user-facing surface consumes it yet, so nothing is available to a user; and this checker can only anchor evidence to the parser-body corpus, so an earned row above layer 1 is not currently expressible at all (#14632). The option-contamination control still proves the parser's own boundary: `-ne print;` parses as a unary expression on `ne`, which is what a parser that is handed a body without argv decoding must do.",
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
        notes: "No adapter decodes POSIX shell quoting. An adapter now has somewhere to send its output — `perl_parser_core::command_line::decode` accepts a tokenized argv (#13726) — so this row is unsupported because the adapter itself is missing, not because layer 2 has nothing to feed.",
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
    /// Whether `id` names a fixture the corpus actually declares, in either
    /// citation role.
    fn contains(&self, id: &str) -> bool {
        self.switch_cases.contains_key(id) || self.proof_tests.contains(id)
    }

    /// Every fixture available as evidence, switch cases and proof targets
    /// together.
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
/// `-e` and `-E` are included because the program text can be written
/// attached (`-eprint`), and everything after it is source, not flags.
const VALUE_ATTACHED_FLAGS: &[char] = &['M', 'm', 'I', 'F', '0', 'i', 'D', 'e', 'E'];

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
    for (position, flag) in cluster.char_indices() {
        // A digit after the first position is a value, not a flag: `-l0777`
        // sets the record separator, it does not request `-0` or `-7`. At the
        // first position a digit *is* the flag, as in `-0777`.
        if position > 0 && flag.is_ascii_digit() {
            break;
        }
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

/// Whether a single outer attribute suppresses the item's execution.
///
/// - `#[cfg(...)]` disables the item conditionally.
/// - `#[ignore]` or `#[ignore = "..."]` marks a test as ignored.
/// - `#[cfg_attr(..., ignore)]` ignores the test conditionally.
/// - `#[cfg_attr(..., cfg(...))]` applies conditional compilation via the tail.
///
/// `#[cfg_attr(feature, allow(dead_code))]` does NOT suppress: the tail must
/// name `ignore` or `cfg`, not an arbitrary lint path.
fn attr_suppresses_execution(attr: &Attribute) -> bool {
    let path = attr.path();
    if path.is_ident("cfg") || path.is_ident("ignore") {
        return true;
    }
    if path.is_ident("cfg_attr")
        && let Meta::List(ref list) = attr.meta
    {
        return cfg_attr_tail_suppresses_execution(list.tokens.clone());
    }
    false
}

/// Whether any attribute in `attrs` suppresses execution.
fn attrs_suppress_execution(attrs: &[Attribute]) -> bool {
    attrs.iter().any(attr_suppresses_execution)
}

/// Whether any OUTER attribute (`#[...]`) in `attrs` suppresses execution.
fn outer_attrs_suppress_execution(attrs: &[Attribute]) -> bool {
    attrs.iter().filter(|a| matches!(a.style, AttrStyle::Outer)).any(attr_suppresses_execution)
}

/// Whether the inner attributes (`#![...]`) of an inline module contain a
/// `cfg` predicate that makes the module's contents conditionally compiled.
fn inner_attrs_have_cfg(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| matches!(a.style, AttrStyle::Inner(_)) && a.path().is_ident("cfg"))
}

/// Split a `TokenStream` on top-level commas into non-empty segments.
///
/// In proc_macro2, `Group` tokens are self-contained — their interior token
/// stream is not exposed at the top level — so a comma inside a group never
/// appears here. No depth tracking is required.
fn split_on_commas(tokens: proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut segments: Vec<proc_macro2::TokenStream> = vec![proc_macro2::TokenStream::new()];
    for tt in tokens {
        if let TokenTree::Punct(ref p) = tt
            && p.as_char() == ','
        {
            segments.push(proc_macro2::TokenStream::new());
            continue;
        }
        if let Some(last) = segments.last_mut() {
            last.extend(std::iter::once(tt));
        }
    }
    segments.into_iter().filter(|s| !s.is_empty()).collect()
}

/// Whether a `cfg_attr`'s tail arguments apply a suppressing attribute.
///
/// `#[cfg_attr(cond, ignore)]`, `#[cfg_attr(cond, cfg(...))]`, and nested
/// `#[cfg_attr(cond, cfg_attr(..., ignore))]` suppress;
/// `#[cfg_attr(cond, allow(dead_code))]` does not.
fn cfg_attr_tail_suppresses_execution(tokens: proc_macro2::TokenStream) -> bool {
    let segments = split_on_commas(tokens);
    // segments[0] is the cfg predicate; segments[1..] are the applied attrs.
    segments.iter().skip(1).any(attribute_segment_suppresses_execution)
}

fn attribute_segment_suppresses_execution(seg: &proc_macro2::TokenStream) -> bool {
    match seg.clone().into_iter().next() {
        Some(TokenTree::Ident(ref id)) if *id == "ignore" || *id == "cfg" => true,
        Some(TokenTree::Ident(ref id)) if *id == "cfg_attr" => syn::parse2::<Meta>(seg.clone())
            .ok()
            .is_some_and(|meta| {
                matches!(meta, Meta::List(ref list) if cfg_attr_tail_suppresses_execution(list.tokens.clone()))
            }),
        _ => false,
    }
}

/// Extract name and switch bundle from `command_line_oneliner!` token args.
///
/// The macro signature is `($name:ident, $switches:literal, $case:expr)`.
fn extract_oneliner_case(tokens: &proc_macro2::TokenStream, evidence: &mut CorpusEvidence) {
    let segments = split_on_commas(tokens.clone());
    let (Some(name_seg), Some(switch_seg)) = (segments.first(), segments.get(1)) else {
        return;
    };
    let name = match name_seg.clone().into_iter().next() {
        Some(TokenTree::Ident(id)) => id.to_string(),
        _ => return,
    };
    if !is_identifier(&name) {
        return;
    }
    let switch = match switch_seg.clone().into_iter().next() {
        Some(TokenTree::Literal(lit)) => {
            let Ok(lit_str) =
                syn::parse2::<syn::LitStr>(proc_macro2::TokenStream::from(TokenTree::Literal(lit)))
            else {
                return;
            };
            lit_str.value()
        }
        _ => return,
    };
    if switch.is_empty() {
        return;
    }
    evidence.switch_cases.insert(name, switch);
}

/// Whether a token stream contains a `command_line_oneliner!` invocation.
fn tokens_contain_oneliner_invocation(tokens: &proc_macro2::TokenStream) -> bool {
    let tts: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let mut i = 0;
    while i < tts.len() {
        match &tts[i] {
            TokenTree::Ident(id) if *id == "command_line_oneliner" => {
                if let (Some(TokenTree::Punct(p)), Some(TokenTree::Group(_))) =
                    (tts.get(i + 1), tts.get(i + 2))
                    && p.as_char() == '!'
                {
                    return true;
                }
                i += 1;
            }
            TokenTree::Group(g) => {
                if tokens_contain_oneliner_invocation(&g.stream()) {
                    return true;
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    false
}

/// Walk parsed items and populate `evidence`.
///
/// `suppressed` is `true` when the enclosing scope is conditionally compiled
/// (e.g. a module whose inner `#![cfg(...)]` disables its contents).
fn extract_from_items(items: &[Item], evidence: &mut CorpusEvidence, suppressed: bool) {
    for item in items {
        match item {
            Item::Macro(item_macro) => {
                // macro_rules! definitions are templates, not citable fixtures.
                if item_macro.mac.path.is_ident("macro_rules") {
                    continue;
                }
                let item_suppressed = suppressed || attrs_suppress_execution(&item_macro.attrs);
                if item_macro.mac.path.is_ident("command_line_oneliner") && !item_suppressed {
                    extract_oneliner_case(&item_macro.mac.tokens, evidence);
                }
            }
            Item::Fn(item_fn) => {
                if suppressed || attrs_suppress_execution(&item_fn.attrs) {
                    continue;
                }
                let has_test = item_fn.attrs.iter().any(|a| a.path().is_ident("test"));
                if has_test {
                    let name = item_fn.sig.ident.to_string();
                    if !evidence.switch_cases.contains_key(&name) {
                        evidence.proof_tests.insert(name);
                    }
                }
            }
            Item::Mod(item_mod) => {
                let Some((_, sub_items)) = &item_mod.content else {
                    // Out-of-line module declarations have no inline fixtures.
                    continue;
                };
                if suppressed || outer_attrs_suppress_execution(&item_mod.attrs) {
                    continue;
                }
                // Inner attributes (#![cfg(...)]) suppress the module's
                // contents; the module itself is not refused here (that is
                // reject_suppressed_modules's job for outer attributes).
                let mod_suppressed = inner_attrs_have_cfg(&item_mod.attrs);
                extract_from_items(sub_items, evidence, mod_suppressed);
            }
            _ => {}
        }
    }
}

/// Extract corpus evidence from the conformance corpus source text.
///
/// Returns empty evidence when the source does not parse as valid Rust; the
/// caller's "no corpus cases found" check will catch that.
fn extract_corpus_evidence(source: &str) -> CorpusEvidence {
    let file = match syn::parse_file(source) {
        Ok(f) => f,
        Err(_) => return CorpusEvidence::default(),
    };
    let mut evidence = CorpusEvidence::default();
    extract_from_items(&file.items, &mut evidence, false);
    evidence
}

/// Refuse to extract from a corpus that invokes a macro defined by
/// `macro_rules!` in the same file, or passes `command_line_oneliner!` through
/// another macro invocation (including inside inline modules).
///
/// Generated fixtures are not discoverable at parse time: macro expansion
/// produces names that only exist after the compiler has run, so those names
/// never appear in the invocation list the parser sees. Rather than silently
/// under-counting, this function fails closed and names the offending macro.
///
/// Exceptions: the `macro_rules` path itself (definitions, not invocations)
/// and `command_line_oneliner` (the canonical corpus macro, handled
/// separately). A parse failure bails with a diagnostic rather than silently
/// accepting.
fn refuse_macro_expansion_fixtures(source: &str) -> Result<()> {
    let file = match syn::parse_file(source) {
        Ok(f) => f,
        Err(e) => bail!("failed to parse {CORPUS_PATH} with syn: {e}"),
    };
    let mut defined: BTreeSet<String> = BTreeSet::new();
    collect_macro_rules_names(&file.items, &mut defined);
    refuse_macro_invocations_in_items(&file.items, &defined, false)
}

fn refuse_macro_invocations_in_items(
    items: &[Item],
    defined: &BTreeSet<String>,
    suppressed: bool,
) -> Result<()> {
    for item in items {
        match item {
            Item::Macro(item_macro) => {
                if item_macro.mac.path.is_ident("macro_rules") {
                    continue;
                }
                let item_suppressed = suppressed || attrs_suppress_execution(&item_macro.attrs);
                if item_suppressed || item_macro.mac.path.is_ident("command_line_oneliner") {
                    continue;
                }
                let name = macro_path_string(&item_macro.mac.path);
                if defined.contains(&name) {
                    bail!(
                        "{CORPUS_PATH} invokes `{name}!`, and `{name}` is defined by \
                         `macro_rules!` in the same file. Fixtures generated by macro \
                         expansion are not discoverable by this checker. Inline the \
                         fixtures directly or use `command_line_oneliner!` instead."
                    );
                }
                if tokens_contain_oneliner_invocation(&item_macro.mac.tokens) {
                    bail!(
                        "{CORPUS_PATH} passes `command_line_oneliner!` through `{name}!`. \
                         Fixtures inside unexpanded macro arguments are not discoverable \
                         and may not run. Inline fixtures at item level or use \
                         `command_line_oneliner!` directly."
                    );
                }
            }
            Item::Mod(item_mod) => {
                let Some((_, sub_items)) = &item_mod.content else {
                    continue;
                };
                if suppressed || outer_attrs_suppress_execution(&item_mod.attrs) {
                    continue;
                }
                let mod_suppressed = inner_attrs_have_cfg(&item_mod.attrs);
                refuse_macro_invocations_in_items(sub_items, defined, mod_suppressed)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn collect_macro_rules_names(items: &[Item], names: &mut BTreeSet<String>) {
    for item in items {
        match item {
            Item::Macro(m) if m.mac.path.is_ident("macro_rules") => {
                if let Some(ident) = &m.ident {
                    names.insert(ident.to_string());
                }
            }
            Item::Mod(m) => {
                if let Some((_, sub_items)) = &m.content {
                    collect_macro_rules_names(sub_items, names);
                }
            }
            _ => {}
        }
    }
}

fn macro_path_string(path: &syn::Path) -> String {
    path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("::")
}

/// Refuse to extract from a corpus containing a suppressed module.
///
/// An outer `#[cfg(...)]`, `#[ignore]`, or `#[cfg_attr(..., ignore)]` on an
/// `Item::Mod` makes the module's contents unreachable without the toolchain
/// actually evaluating the predicate. Rather than silently over-count, this
/// function fails closed and names the module.
///
/// Inner `#![cfg(...)]` attributes are handled by the extraction walk (the
/// walk marks the contents as suppressed rather than refusing), because they
/// are semantically equivalent but expressed as a module-internal signal.
///
/// A parse failure returns `Ok(())`: an unparseable corpus has no discernible
/// suppressed modules; the downstream "no corpus cases" check handles it.
fn reject_suppressed_modules(source: &str) -> Result<()> {
    let file = match syn::parse_file(source) {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    reject_suppressed_in_items(&file.items)
}

fn reject_suppressed_in_items(items: &[Item]) -> Result<()> {
    for item in items {
        let Item::Mod(item_mod) = item else { continue };
        if outer_attrs_suppress_execution(&item_mod.attrs) {
            bail!(
                "{CORPUS_PATH} declares module `{}` under a suppressing attribute.                  Fixture reachability inside a disabled module cannot be established by                  attribute inspection, so evidence extraction stops rather than counting                  fixtures that may never compile. Move the fixtures out of the module, or                  extend the extractor with a real item walk.",
                item_mod.ident
            );
        }
        if let Some((_, sub_items)) = &item_mod.content {
            reject_suppressed_in_items(sub_items)?;
        }
    }
    Ok(())
}

/// Whether `value` is a plausible Rust item name.
///
/// A macro argument that is not one is not a fixture name, so it is dropped
/// rather than recorded as evidence under a name nothing declares.
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

    // Every corpus fixture must be accounted for, so growing the corpus without
    // revisiting the claim is a checkable drift rather than a silent gap. Typed
    // proof targets count too: adding one used to slip through, leaving a new
    // fixture rendered in the table but assigned to no claim.
    //
    // A fixture is accounted for by either role. A negative control legitimately
    // appears only in `boundary_controls`, so requiring `evidence` alone would
    // reject the committed corpus rather than catch drift.
    for case in evidence.switch_cases.keys().chain(evidence.proof_tests.iter()) {
        let cited = rows.iter().any(|row| {
            row.evidence.contains(&case.as_str()) || row.boundary_controls.contains(&case.as_str())
        });
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
    refuse_macro_expansion_fixtures(&corpus)?;
    reject_suppressed_modules(&corpus)?;
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

/// Render the capability matrix as Markdown.
///
/// Output is a pure function of `rows` and `evidence` — no timestamps, no
/// environment, no iteration order that varies — because `--check` compares the
/// rendered text against the committed page and any instability would surface as
/// a spurious drift failure. `validate` runs first, so rendering never has to
/// decide what an unearned or unevidenced row should look like.
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
    // `supported` and `partial` are counted separately: collapsing them into one
    // "earned" number reads as more full support than the rows actually claim.
    output.push_str("| # | Layer | Supported | Partial | Total rows |\n");
    output.push_str("| --- | --- | --- | --- | --- |\n");
    for layer in Layer::ALL {
        let layer_rows: Vec<&CapabilityRow> =
            rows.iter().filter(|row| row.layer == *layer).collect();
        let supported = layer_rows.iter().filter(|row| row.status == Support::Supported).count();
        let partial = layer_rows.iter().filter(|row| row.status == Support::Partial).count();
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            layer.ordinal(),
            escape_cell(layer.title()),
            supported,
            partial,
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

/// Fixture identifiers as a table cell, or an em dash when there are none.
fn render_ids(ids: &[&str]) -> String {
    if ids.is_empty() {
        return "—".to_string();
    }
    ids.iter().map(|id| format!("`{id}`")).collect::<Vec<_>>().join("; ")
}

/// The command that runs a row's evidence, or an em dash when the row earns
/// nothing and therefore names none.
fn render_invocable(value: &str) -> String {
    if value.is_empty() { "—".to_string() } else { format!("`{value}`") }
}

/// An em dash for an empty cell, so a blank never reads as an omission.
fn dash_if_empty(value: &str) -> &str {
    if value.is_empty() { "—" } else { value }
}

/// Escape a value for a Markdown table cell.
///
/// A literal pipe would silently split the row into extra columns, and a
/// newline would end it early.
fn escape_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', "<br>")
}

/// Normalise line endings so `--check` compares content rather than the
/// checkout's newline style.
fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_tdd_support::{must_err_with, must_some_with, must_with};

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
        let root = must_with(project_root(), "project root");
        let corpus = must_with(fs::read_to_string(root.join(CORPUS_PATH)), "corpus readable");
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
        let error = must_err_with(
            validate(rows, &evidence),
            format!("expected validation to reject rows mentioning {fragment}"),
        );
        let message = error.to_string();
        assert!(message.contains(fragment), "error {message:?} did not mention {fragment:?}");
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
        assert!(evidence.proof_tests.contains("named_idioms_share_one_structural_table"));
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
        must_with(
            validate(ROWS, &evidence),
            "committed rows must be backed by the committed corpus",
        );
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
            let row = must_some_with(
                ROWS.iter()
                    .find(|row| row.layer == Layer::ShellAdapters && row.subject == *adapter),
                "adapter row present",
            );
            assert!(
                !row.status.is_earned() || argv_earned,
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

    /// Reachability inside a disabled module is not provable by attribute
    /// inspection, so extraction stops rather than over-counting.
    #[test]
    fn suppressed_module_stops_extraction() {
        let disabled_mod = concat!(
            "#[cfg(feature = \"unfinished\")]\n",
            "mod disabled_fixtures {\n",
            "    command_line_oneliner!(ghost_in_mod, \"-e\", \"print 1;\");\n",
            "}\n"
        );
        let error = must_err_with(
            reject_suppressed_modules(disabled_mod),
            "a suppressed module was accepted",
        );
        let message = error.to_string();
        assert!(message.contains("disabled_fixtures"), "{message}");
        assert!(message.contains("reachability"), "{message}");

        // A visibility qualifier between the attribute and the keyword must not
        // hide the suppression.
        for form in [
            "#[cfg(feature = \"x\")]\npub mod d {\n    command_line_oneliner!(g, \"-e\", \"print 1;\");\n}\n",
            "#[cfg(feature = \"x\")]\npub(crate) mod d {}\n",
            "#[cfg(feature = \"x\")]\npub(in crate::a) mod d {}\n",
        ] {
            assert!(
                reject_suppressed_modules(form).is_err(),
                "qualifier hid the suppressed module in {form:?}"
            );
        }

        // An ordinary module declaration is fine; the committed corpus has one.
        must_with(reject_suppressed_modules("mod cpan_test_helpers;\n"), "plain module accepted");
        must_with(reject_suppressed_modules("pub mod helpers;\n"), "plain pub module accepted");
        let root = must_with(project_root(), "project root");
        let corpus = must_with(fs::read_to_string(root.join(CORPUS_PATH)), "corpus readable");
        must_with(reject_suppressed_modules(&corpus), "committed corpus has no suppressed module");
    }

    /// Adding a typed proof target must not slip past the drift rule.
    #[test]
    fn uncited_proof_test_is_rejected() {
        let corpus = format!("{CORPUS_SAMPLE}\n#[test]\nfn brand_new_proof_target() {{}}\n");
        let evidence = extract_corpus_evidence(&corpus);
        assert!(evidence.proof_tests.contains("brand_new_proof_target"));
        let error =
            must_err_with(validate(&scaffold(), &evidence), "an uncited proof target was accepted");
        assert!(error.to_string().contains("brand_new_proof_target"), "{error}");
    }

    /// A negative control is cited as a boundary control, never as support, so
    /// the drift rule has to accept either role.
    #[test]
    fn boundary_control_citation_satisfies_the_drift_rule() {
        let evidence = real_evidence();
        assert!(
            !ROWS.iter().any(|row| {
                row.evidence
                    .contains(&"negative_controls_keep_context_errors_and_boundaries_visible")
            }),
            "the negative control should be cited only as a boundary control"
        );
        must_with(validate(ROWS, &evidence), "committed rows still validate");
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
            "#[cfg_attr(test, cfg(any()))]\n#[test]\nfn cfg_attr_cfg_disabled() {}\n",
            "#[cfg_attr(test, cfg(any()))]\ncommand_line_oneliner!(cfg_attr_cfg_case, \"-e\", \"print 1;\");\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.proof_tests.is_empty() && evidence.switch_cases.is_empty(),
                "suppressed item was captured from {source:?}: switch={:?} proof={:?}",
                evidence.switch_cases,
                evidence.proof_tests
            );
        }
    }

    /// A comment anywhere in the walked neighbourhood must not hide a
    /// suppressing attribute.
    ///
    /// Each backward walk steps over whitespace and reads the token it lands
    /// on. Before non-code was blanked, a comment between an attribute and its
    /// item — or between a visibility qualifier and its keyword — ended the
    /// walk early and the suppressed fixture counted as running evidence.
    #[test]
    fn comments_in_the_walk_neighbourhood_do_not_hide_suppression() {
        for form in [
            "#[cfg(feature = \"x\")]\npub /* later */ mod d {}\n",
            "#[cfg(feature = \"x\")]\npub // later\nmod d {}\n",
            "#[cfg(feature = \"x\")]\n// why this is off\nmod d {}\n",
            "#[cfg(feature = \"x\")]\n/* why */ pub(crate) /* who */ mod d {}\n",
        ] {
            assert!(
                reject_suppressed_modules(form).is_err(),
                "a comment hid the suppressed module in {form:?}"
            );
        }

        for source in [
            "#[cfg(feature = \"x\")]\n// pending\ncommand_line_oneliner!(ghost, \"-e\", \"print 1;\");\n",
            "#[cfg(feature = \"x\")]\n/* pending */\ncommand_line_oneliner!(ghost, \"-e\", \"print 1;\");\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.switch_cases.is_empty(),
                "a comment hid the suppression in {source:?}: {:?}",
                evidence.switch_cases
            );
        }

        for source in [
            "#[ignore]\n// waiting on the oracle\n#[test]\nfn parked() {}\n",
            "#[test]\n/* waiting */\n#[ignore]\nfn parked() {}\n",
            "#[cfg(target_os = \"linux\")]\n// linux only\n#[test]\nfn parked() {}\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.proof_tests.is_empty(),
                "a comment hid the suppression in {source:?}: {:?}",
                evidence.proof_tests
            );
        }

        // Blanking must not cost a live fixture: the same shapes without the
        // suppressing attribute still register.
        let live = extract_corpus_evidence(
            "// ordinary note\ncommand_line_oneliner!(live_case, \"-e\", \"print 1;\");\n\
             #[test]\n// ordinary note\nfn live_proof() {}\n",
        );
        assert!(live.switch_cases.contains_key("live_case"), "{:?}", live.switch_cases);
        assert!(live.proof_tests.contains("live_proof"), "{:?}", live.proof_tests);
    }

    /// A fixture written inside a macro definition is a template, not a test,
    /// and must not become citable evidence.
    ///
    /// `cargo test` generates nothing from an uninvoked `macro_rules!` body, so
    /// a row citing a name that only appears there would publish support for a
    /// fixture that never runs.
    #[test]
    fn macro_definition_bodies_are_not_evidence() {
        let phantom = concat!(
            "macro_rules! helper {\n",
            "    () => {\n",
            "        command_line_oneliner!(phantom_case, \"-e\", \"print 1;\");\n",
            "        #[test]\n",
            "        fn phantom_proof() {}\n",
            "    };\n",
            "}\n"
        );
        let evidence = extract_corpus_evidence(phantom);
        assert!(
            evidence.switch_cases.is_empty(),
            "a templated fixture was captured: {:?}",
            evidence.switch_cases
        );
        assert!(
            evidence.proof_tests.is_empty(),
            "a templated proof target was captured: {:?}",
            evidence.proof_tests
        );

        // Citing the phantom is refused by the existing unknown-fixture rule.
        let mut row = must_some_with(
            ROWS.iter().find(|row| row.layer == Layer::ParserBody && row.subject == "-e").cloned(),
            "the -e row exists",
        );
        row.evidence = &["phantom_case"];
        expect_rejected(&scaffold_with(row), "phantom_case");

        // Nested delimiters inside the body must not end it early, and real
        // invocations after the definition must still register.
        let real = format!(
            "{phantom}\ncommand_line_oneliner!(after_macro, \"-ne\", \"print;\");\n\
             #[test]\nfn after_macro_proof() {{}}\n"
        );
        let evidence = extract_corpus_evidence(&real);
        assert!(evidence.switch_cases.contains_key("after_macro"), "{:?}", evidence.switch_cases);
        assert!(evidence.proof_tests.contains("after_macro_proof"), "{:?}", evidence.proof_tests);
        assert!(!evidence.switch_cases.contains_key("phantom_case"));

        // A comment between the keyword and the body must not leave the
        // template searchable.
        for source in [
            "macro_rules! /* named later */ helper {\n    () => {\n        command_line_oneliner!(phantom_case, \"-e\", \"print 1;\");\n    };\n}\n",
            "macro_rules! // named later\nhelper {\n    () => {\n        command_line_oneliner!(phantom_case, \"-e\", \"print 1;\");\n    };\n}\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.switch_cases.is_empty(),
                "a comment left the template searchable in {source:?}: {:?}",
                evidence.switch_cases
            );
        }

        // The keyword must start a token. An invocation whose name merely ends
        // in `macro_rules` is ordinary code, but passing `command_line_oneliner!`
        // through another macro is refused rather than counted or silently skipped.
        let tail_named = concat!(
            "helper_macro_rules!(\n",
            "    command_line_oneliner!(real_case, \"-ne\", \"print;\");\n",
            ");\n"
        );
        let error = must_err_with(
            refuse_macro_expansion_fixtures(tail_named),
            "a wrapped oneliner invocation was accepted",
        );
        assert!(
            error.to_string().contains("helper_macro_rules"),
            "error did not name the wrapper macro: {error}"
        );
        let evidence = extract_corpus_evidence(tail_named);
        assert!(
            evidence.switch_cases.is_empty(),
            "wrapped fixture was extracted: {:?}",
            evidence.switch_cases
        );

        // The committed corpus defines its fixture macro this way and still
        // yields every real invocation that follows it.
        let root = must_with(project_root(), "project root");
        let corpus = must_with(fs::read_to_string(root.join(CORPUS_PATH)), "corpus readable");
        assert!(corpus.contains("macro_rules! command_line_oneliner"), "corpus shape changed");
        let evidence = extract_corpus_evidence(&corpus);
        assert_eq!(evidence.switch_cases.len(), 20, "{:?}", evidence.switch_cases.keys());
    }

    /// Whitespace inside an attribute, or after `mod`, must not decide whether
    /// a fixture is executable.
    ///
    /// `#[ cfg(any()) ]` and `mod\tname` are valid Rust meaning exactly what
    /// their dense spellings mean. Matching the dense spelling alone let a
    /// disabled fixture certify support — the over-claiming direction this
    /// check exists to prevent. `rustfmt` would normalise these, but soundness
    /// here must not depend on another gate being green.
    #[test]
    fn attribute_spacing_does_not_defeat_suppression() {
        for source in [
            "#[ cfg(any()) ]\ncommand_line_oneliner!(spaced, \"-e\", \"print 1;\");\n",
            "#[cfg ( any() )]\ncommand_line_oneliner!(spaced, \"-e\", \"print 1;\");\n",
            "#[\n    cfg(any())\n]\ncommand_line_oneliner!(spaced, \"-e\", \"print 1;\");\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.switch_cases.is_empty(),
                "spacing defeated suppression in {source:?}: {:?}",
                evidence.switch_cases
            );
        }

        for source in [
            "#[ ignore ]\n#[test]\nfn parked() {}\n",
            "#[test]\n#[ cfg_attr( windows , ignore ) ]\nfn parked() {}\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.proof_tests.is_empty(),
                "spacing defeated suppression in {source:?}: {:?}",
                evidence.proof_tests
            );
        }

        // The `mod` keyword is matched as a token, so separating whitespace and
        // a name on the next line are both seen.
        for form in [
            "#[cfg(feature = \"x\")]\nmod\tdisabled {}\n",
            "#[cfg(feature = \"x\")]\nmod\n    disabled {}\n",
        ] {
            let error = must_err_with(
                reject_suppressed_modules(form),
                format!("whitespace hid the suppressed module in {form:?}"),
            );
            assert!(error.to_string().contains("disabled"), "{error}");
        }

        // A token match must not fire on an identifier that merely contains it.
        for form in ["#[cfg(feature = \"x\")]\nfn modify() {}\n", "fn commodity() {}\n"] {
            must_with(
                reject_suppressed_modules(form),
                format!("`mod` matched inside a word in {form:?}"),
            );
        }
    }

    /// Every literal this module scans for also occurs inside longer names, so
    /// each scan must find a token rather than a substring.
    ///
    /// This shape has been the most repeated defect in the module — a fixture
    /// macro, the `macro_rules!` keyword, and a switch bundle each hit it — so
    /// the control sweeps all of the sites at once rather than the one last
    /// reported.
    #[test]
    fn scans_match_tokens_not_substrings() {
        // A different macro whose name ends with the fixture macro's.
        let helper = "helper_command_line_oneliner!(fake, \"-e\", \"print 1;\");\n";
        let evidence = extract_corpus_evidence(helper);
        assert!(
            evidence.switch_cases.is_empty(),
            "an unrelated macro supplied a fixture: {:?}",
            evidence.switch_cases
        );

        // A `fn ` ending an identifier in attribute *code* — not inside a
        // string, which the mask already removes — must not supply the name.
        let decoy = "#[test]\n#[allow(myfn x)]\nfn real_proof() {}\n";
        let evidence = extract_corpus_evidence(decoy);
        assert_eq!(
            evidence.proof_tests.iter().collect::<Vec<_>>(),
            vec!["real_proof"],
            "declaration name came from something other than the declaration"
        );

        // `mod` separated from its name by a comment is still the keyword.
        assert!(
            reject_suppressed_modules("#[cfg(feature = \"x\")]\nmod/* c */disabled {}\n").is_err(),
            "a comment between `mod` and its name hid the suppressed module"
        );

        // …and the refusal must still name the module, not an empty string.
        let error = must_err_with(
            reject_suppressed_modules("#[cfg(feature = \"x\")]\nmod /* c */ disabled {}\n"),
            "a comment hid the suppressed module",
        );
        assert!(
            error.to_string().contains("`disabled`"),
            "refusal did not name the module: {error}"
        );

        // Identifiers that merely contain a scanned literal stay untouched.
        for form in ["fn modify() {}\n", "fn commodity() {}\n"] {
            must_with(reject_suppressed_modules(form), "`mod` matched inside a word");
        }
    }

    /// Non-code must not be able to imitate the structure evidence extraction
    /// reads: argument delimiters, literal boundaries, or an attribute run.
    ///
    /// All three routes below over-claim — they let a switch, or a fixture,
    /// gain evidence it has not earned — which is the direction this check
    /// exists to prevent.
    #[test]
    fn non_code_cannot_imitate_evidence_structure() {
        // A quoted comment before the real literal must not supply the bundle.
        let quoted_comment = "command_line_oneliner!(case_a, /* \"-Z\" */ \"-e\", \"print 1;\");\n";
        let evidence = extract_corpus_evidence(quoted_comment);
        assert_eq!(
            evidence.switch_cases.get("case_a").map(String::as_str),
            Some("-e"),
            "a comment supplied the switch bundle: {:?}",
            evidence.switch_cases
        );

        // A line comment carrying a comma must not end the name either.
        let comma_comment = "command_line_oneliner!(case_b // , \"-Z\"\n, \"-ne\", \"print;\");\n";
        assert_eq!(
            extract_corpus_evidence(comma_comment).switch_cases.get("case_b").map(String::as_str),
            Some("-ne"),
        );

        // Raw byte and C strings hide their contents like any other literal, so
        // an embedded quote cannot expose fixture-shaped text as code.
        for source in [
            "const S: &[u8] = br#\"q \" command_line_oneliner!(ghost, \"-e\", \"x\");\"#;\n",
            "const S: &[u8] = b\"q command_line_oneliner!(ghost, \\\"-e\\\", \\\"x\\\");\";\n",
            "const S: &core::ffi::CStr = cr#\"q \" command_line_oneliner!(ghost, \"-e\", \"x\");\"#;\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.switch_cases.is_empty(),
                "a literal exposed fixture text in {source:?}: {:?}",
                evidence.switch_cases
            );
        }

        // …while the prefixes as ordinary identifiers stay code.
        let identifiers = "type Br = i32; type B = i32; type Cr = i32;\ncommand_line_oneliner!(live, \"-e\", \"print 1;\");\n";
        assert!(extract_corpus_evidence(identifiers).switch_cases.contains_key("live"));

        // A Unicode-prefixed name whose tail spells a scanned token is a
        // different identifier, at every scan site.
        for source in [
            "écommand_line_oneliner!(fake, \"-e\", \"print 1;\");\n",
            "Ωcommand_line_oneliner!(fake, \"-e\", \"print 1;\");\n",
        ] {
            let evidence = extract_corpus_evidence(source);
            assert!(
                evidence.switch_cases.is_empty(),
                "a Unicode-prefixed macro supplied a fixture in {source:?}: {:?}",
                evidence.switch_cases
            );
        }
        assert!(
            reject_suppressed_modules("#[cfg(feature = \"x\")]\nfn émod() {}\n").is_ok(),
            "`mod` matched inside a Unicode identifier"
        );

        // A suppressing attribute stays visible however many follow it.
        let mut many = String::from("#[cfg(feature = \"x\")]\n");
        for index in 0..20 {
            many.push_str(&format!("#[allow(dead_code)] // {index}\n"));
        }
        many.push_str("command_line_oneliner!(ghost_c, \"-e\", \"print 1;\");\n");
        assert!(
            extract_corpus_evidence(&many).switch_cases.is_empty(),
            "an attribute run hid the suppression: {:?}",
            extract_corpus_evidence(&many).switch_cases
        );
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

    /// A `cfg_attr` wrapped across lines must still be seen. A line-based walk
    /// stopped at the continuation and let the fixture count as running.
    #[test]
    fn multiline_attributes_still_suppress_a_fixture() {
        let wrapped = concat!(
            "#[cfg_attr(\n",
            "    target_os = \"windows\",\n",
            "    ignore = \"flaky on windows\"\n",
            ")]\n",
            "#[test]\n",
            "fn wrapped_but_ignored() {}\n"
        );
        let evidence = extract_corpus_evidence(wrapped);
        assert!(
            evidence.proof_tests.is_empty(),
            "multiline cfg_attr did not suppress: {:?}",
            evidence.proof_tests
        );

        let wrapped_case = concat!(
            "#[cfg(\n",
            "    feature = \"unfinished\"\n",
            ")]\n",
            "command_line_oneliner!(ghost_wrapped, \"-e\", \"print 1;\");\n"
        );
        let evidence = extract_corpus_evidence(wrapped_case);
        assert!(
            evidence.switch_cases.is_empty(),
            "multiline cfg did not suppress: {:?}",
            evidence.switch_cases
        );
    }

    /// The fixture name must come from a real declaration, not from text inside
    /// a doc attribute or comment sitting between `#[test]` and the `fn`.
    #[test]
    fn quoted_function_names_are_not_fixture_names() {
        let doc_decoy = concat!(
            "#[test]\n",
            "#[doc = \"fn decoy_fixture() is only prose\"]\n",
            "fn real_fixture() {}\n"
        );
        let evidence = extract_corpus_evidence(doc_decoy);
        assert!(
            !evidence.proof_tests.contains("decoy_fixture"),
            "a doc attribute supplied the fixture name: {:?}",
            evidence.proof_tests
        );
        assert!(evidence.proof_tests.contains("real_fixture"));

        let comment_decoy = "#[test]\n// fn commented_fixture()\nfn real_fixture() {}\n";
        let evidence = extract_corpus_evidence(comment_decoy);
        assert!(!evidence.proof_tests.contains("commented_fixture"));
        assert!(evidence.proof_tests.contains("real_fixture"));
    }

    /// Attached program text and numeric arguments are values, not flags.
    #[test]
    fn attached_values_do_not_masquerade_as_flags() {
        // `-eprint` is `-e` plus a program, not a request for -p/-r/-i/-n/-t.
        assert_eq!(bundle_flags("-eprint"), vec!['e']);
        assert!(!bundle_declares("-eprint", "-p"));
        assert!(!bundle_declares("-eprint", "-n"));
        assert!(bundle_declares("-eprint", "-e"));

        // `-l0777` sets the record separator; it is not `-0` or `-7`.
        assert_eq!(bundle_flags("-l0777"), vec!['l']);
        assert!(!bundle_declares("-l0777", "-0"));
        assert!(bundle_declares("-l0777", "-l"));

        // A leading digit *is* the flag.
        assert_eq!(bundle_flags("-0777"), vec!['0']);
        assert!(bundle_declares("-0777", "-0"));

        // Ordinary clusters are untouched.
        assert_eq!(bundle_flags("-lane"), vec!['l', 'a', 'n', 'e']);
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
        let error = must_err_with(
            validate(&scaffold_with(row), &evidence),
            "a commented-out fixture earned support",
        );
        assert!(error.to_string().contains("does not exist"));
    }

    /// Code context must not be over-trimmed: the live corpus still resolves,
    /// including its raw-string bodies and character literals.
    #[test]
    fn syn_extraction_finds_all_live_corpus_cases() {
        let evidence = real_evidence();
        assert!(evidence.switch_cases.len() >= 18, "live cases lost: {:?}", evidence.switch_cases);
        assert!(evidence.proof_tests.contains("named_idioms_share_one_structural_table"));
    }

    // ---- syn-specific: delimiter variants, inline mods, cfg_attr, macros --

    /// `command_line_oneliner!` with `[]` or `{}` delimiters is discovered
    /// the same way as the canonical `()` form — the token content is
    /// identical regardless of which delimiter syn gives us.
    #[test]
    fn spaced_bracket_and_brace_delimiters_are_discovered() {
        let source = concat!(
            "command_line_oneliner!(paren_case, \"-e\", \"print 1;\");\n",
            "command_line_oneliner![bracket_case, \"-ne\", \"print;\"];\n",
            "command_line_oneliner!{brace_case, \"-pe\", \"s/a/b/;\"}\n",
            "#[test]\nfn proof_paren() {}\n",
        );
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.contains_key("paren_case"),
            "paren delimiter not found: {:?}",
            evidence.switch_cases
        );
        assert!(
            evidence.switch_cases.contains_key("bracket_case"),
            "bracket delimiter not found: {:?}",
            evidence.switch_cases
        );
        assert!(
            evidence.switch_cases.contains_key("brace_case"),
            "brace delimiter not found: {:?}",
            evidence.switch_cases
        );
        assert!(evidence.proof_tests.contains("proof_paren"));
    }

    /// A fixture inside a plain inline module is discovered. A fixture inside
    /// an inline module whose inner `#![cfg(...)]` disables it is suppressed
    /// by the extraction walk (marked with the suppressed flag) rather than
    /// causing a refusal.
    #[test]
    fn plain_inline_mod_fixture_is_discovered_and_cfg_inner_suppresses() {
        // Plain inline mod: fixture must be found.
        let plain = concat!(
            "mod helpers {\n",
            "    command_line_oneliner!(in_plain, \"-e\", \"print 1;\");\n",
            "    #[test]\n",
            "    fn plain_proof() {}\n",
            "}\n"
        );
        let evidence = extract_corpus_evidence(plain);
        assert!(
            evidence.switch_cases.contains_key("in_plain"),
            "inline mod fixture not found: {:?}",
            evidence.switch_cases
        );
        assert!(evidence.proof_tests.contains("plain_proof"));

        // Inline mod with inner #![cfg(...)]: fixture must be suppressed.
        let inner_cfg = concat!(
            "mod inner_disabled {\n",
            "    #![cfg(feature = \"never\")]\n",
            "    command_line_oneliner!(in_inner_cfg, \"-e\", \"print 1;\");\n",
            "}\n"
        );
        let evidence = extract_corpus_evidence(inner_cfg);
        assert!(
            evidence.switch_cases.is_empty(),
            "inner-cfg fixture was not suppressed: {:?}",
            evidence.switch_cases
        );

        // The same module is not rejected by reject_suppressed_modules (outer
        // attrs only trigger that check).
        must_with(
            reject_suppressed_modules(inner_cfg),
            "inner-cfg mod must not be rejected by the outer-attr check",
        );
    }

    /// A `#[cfg_attr(feature, allow(dead_code))]` on a fixture does NOT
    /// suppress it. Only `cfg_attr` tails that apply `ignore` or `cfg` suppress.
    #[test]
    fn benign_cfg_attr_allow_on_fixture_still_counts() {
        let source = concat!(
            "#[cfg_attr(feature = \"extra\", allow(dead_code))]\n",
            "command_line_oneliner!(benign_attr, \"-e\", \"print 1;\");\n",
            "#[cfg_attr(test, allow(clippy::pedantic))]\n",
            "#[test]\n",
            "fn benign_fn_attr() {}\n",
        );
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.contains_key("benign_attr"),
            "benign cfg_attr suppressed a macro case: {:?}",
            evidence.switch_cases
        );
        assert!(
            evidence.proof_tests.contains("benign_fn_attr"),
            "benign cfg_attr suppressed a proof test: {:?}",
            evidence.proof_tests
        );
    }

    /// A top-level invocation of a macro defined by `macro_rules!` in the
    /// same file is refused with an error naming the macro. Invocations of
    /// `command_line_oneliner!` and the `macro_rules` definition itself are
    /// not refused.
    #[test]
    fn macro_rules_expansion_invocation_is_refused() {
        let expansion = concat!(
            "macro_rules! make {\n",
            "    () => { command_line_oneliner!(fake, \"-e\", \"x;\"); };\n",
            "}\n",
            "make!();\n"
        );
        let error = must_err_with(
            refuse_macro_expansion_fixtures(expansion),
            "a macro expansion invocation was accepted",
        );
        let message = error.to_string();
        assert!(message.contains("`make!`"), "error did not name the macro: {message}");

        let module_expansion = concat!(
            "mod extra {\n",
            "    macro_rules! make {\n",
            "        () => { command_line_oneliner!(new_case, \"-e\", \"x;\"); };\n",
            "    }\n",
            "    make!();\n",
            "}\n"
        );
        let error = must_err_with(
            refuse_macro_expansion_fixtures(module_expansion),
            "module-local macro expansion was accepted",
        );
        assert!(
            error.to_string().contains("`make!`"),
            "module-local error did not name the macro: {error}"
        );

        // command_line_oneliner! invocations are exempt.
        let only_canonical = concat!(
            "macro_rules! command_line_oneliner {\n",
            "    ($n:ident, $s:literal, $e:expr) => { #[test] fn $n() {} };\n",
            "}\n",
            "command_line_oneliner!(safe_case, \"-e\", \"print 1;\");\n"
        );
        must_with(
            refuse_macro_expansion_fixtures(only_canonical),
            "canonical macro invocations must be exempt",
        );

        // A source with no macro_rules at all is fine.
        must_with(
            refuse_macro_expansion_fixtures(
                "command_line_oneliner!(also_safe, \"-e\", \"print 1;\");\n",
            ),
            "source with no macro_rules must pass",
        );
    }

    /// A doc comment, line comment, or block comment that contains text
    /// resembling a suppressing attribute does NOT suppress the fixture below
    /// it. Only real parsed attributes trigger suppression.
    #[test]
    fn fake_suppressing_text_in_comments_does_not_suppress() {
        // Doc comment with fake cfg text.
        let doc = concat!(
            "/// #[cfg(feature = \"never\")]\n",
            "command_line_oneliner!(doc_comment_case, \"-e\", \"print 1;\");\n",
        );
        let evidence = extract_corpus_evidence(doc);
        assert!(
            evidence.switch_cases.contains_key("doc_comment_case"),
            "doc comment text suppressed the fixture: {:?}",
            evidence.switch_cases
        );

        // Line comment with fake cfg text.
        let line = concat!(
            "// #[cfg(feature = \"never\")]\n",
            "command_line_oneliner!(line_comment_case, \"-e\", \"print 1;\");\n",
        );
        let evidence = extract_corpus_evidence(line);
        assert!(
            evidence.switch_cases.contains_key("line_comment_case"),
            "line comment text suppressed the fixture: {:?}",
            evidence.switch_cases
        );

        // Block comment with fake cfg text.
        let block = concat!(
            "/* #[cfg(feature = \"never\")] */\n",
            "command_line_oneliner!(block_comment_case, \"-e\", \"print 1;\");\n",
        );
        let evidence = extract_corpus_evidence(block);
        assert!(
            evidence.switch_cases.contains_key("block_comment_case"),
            "block comment text suppressed the fixture: {:?}",
            evidence.switch_cases
        );

        // Doc attribute with fake ignore text (not an ignore attribute).
        let doc_attr = concat!(
            "#[doc = \"#[ignore] this is just docs\"]\n",
            "#[test]\n",
            "fn doc_attr_proof() {}\n",
        );
        let evidence = extract_corpus_evidence(doc_attr);
        assert!(
            evidence.proof_tests.contains("doc_attr_proof"),
            "doc attribute text suppressed the proof test: {:?}",
            evidence.proof_tests
        );
    }

    /// Token spacing that is invisible to substring scans must still resolve
    /// once discovery is a parse.
    #[test]
    fn spaced_macro_and_test_declaration_tokens_are_discovered() {
        let source = concat!(
            "command_line_oneliner! (spaced_macro, \"-e\", \"print 1;\");\n",
            "#[test]\nfn\nspaced_proof() {}\n",
        );
        let evidence = extract_corpus_evidence(source);
        assert!(
            evidence.switch_cases.contains_key("spaced_macro"),
            "spaced macro invocation not found: {:?}",
            evidence.switch_cases
        );
        assert!(
            evidence.proof_tests.contains("spaced_proof"),
            "newline-separated test fn not found: {:?}",
            evidence.proof_tests
        );
    }

    /// Suppression on an outer inline module hides fixtures in nested modules;
    /// an inner module without its own suppression stays visible when the
    /// outer module is live.
    #[test]
    fn nested_module_suppression_composes_from_outer_cfg_only() {
        let outer_disabled = concat!(
            "#[cfg(feature = \"off\")]\n",
            "mod outer {\n",
            "    mod inner {\n",
            "        command_line_oneliner!(nested_ghost, \"-e\", \"print 1;\");\n",
            "    }\n",
            "}\n",
        );
        assert!(
            reject_suppressed_modules(outer_disabled).is_err(),
            "outer cfg mod should be refused"
        );
        assert!(
            extract_corpus_evidence(outer_disabled).switch_cases.is_empty(),
            "nested fixture leaked through outer cfg: {:?}",
            extract_corpus_evidence(outer_disabled).switch_cases
        );

        let outer_live = concat!(
            "mod outer {\n",
            "    mod inner {\n",
            "        command_line_oneliner!(nested_live, \"-e\", \"print 1;\");\n",
            "        #[test]\n",
            "        fn nested_proof() {}\n",
            "    }\n",
            "}\n",
        );
        must_with(reject_suppressed_modules(outer_live), "live nested modules must not be refused");
        let evidence = extract_corpus_evidence(outer_live);
        assert!(
            evidence.switch_cases.contains_key("nested_live"),
            "nested live fixture missing: {:?}",
            evidence.switch_cases
        );
        assert!(evidence.proof_tests.contains("nested_proof"));
    }
}
