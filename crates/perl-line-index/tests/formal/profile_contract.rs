//! Source/profile drift guards, not native proof or Rust syntax validation.
//!
//! These checks intentionally recognize this small, reviewed source layout. A
//! rewrite requires review rather than guessing semantic equivalence. Only a real
//! Kani run can establish reachable assertions and satisfied cover obligations.

const POSITIVE: &str = include_str!("line_index_kani.rs");
const NEGATIVE: &str = include_str!("false_scalar_boundary.rs");
const README: &str = include_str!("../../README.md");

struct Harness {
    name: &'static str,
    checker: &'static str,
    covers: &'static [&'static str],
}

const HARNESSES: &[Harness] = &[
    Harness {
        name: "byte_geometry_and_reverse_acceptance",
        checker: "contract::check_bytes(",
        covers: &[
            "FV_POSITION_COVER_EMPTY",
            "FV_POSITION_COVER_MAX_BYTES",
            "FV_POSITION_COVER_MAX_LINES",
            "FV_POSITION_COVER_CRLF",
            "FV_POSITION_COVER_CLAMP_MAX",
            "FV_POSITION_COVER_INVALID_LINE_MAX",
            "FV_POSITION_COVER_INVALID_COLUMN_MAX",
        ],
    },
    Harness {
        name: "strict_utf16_soundness",
        checker: "contract::check_utf16_sound(",
        covers: &[
            "FV_POSITION_COVER_UTF16_ACCEPTANCE",
            "FV_POSITION_COVER_SURROGATE_REJECTION",
            "FV_POSITION_COVER_CRLF_REJECTION",
            "FV_POSITION_COVER_UTF16_COLUMN_MAX",
        ],
    },
    Harness {
        name: "strict_utf16_completeness",
        checker: "contract::check_utf16_complete(",
        covers: &[
            "FV_POSITION_COVER_EMPTY_EOF",
            "FV_POSITION_COVER_NON_BMP_EOF",
            "FV_POSITION_COVER_COMBINING_EOF",
            "FV_POSITION_COVER_BMP_EOF",
            "FV_POSITION_COVER_TWO_BYTE_EOF",
            "FV_POSITION_COVER_TAB_EOF",
            "FV_POSITION_COVER_TRAILING_LINE",
        ],
    },
];

fn common_scope(source: &str) -> bool {
    [
        "#[cfg(not(target_pointer_width = \"64\"))]",
        "#[path = \"../../src/lib.rs\"]\nmod production;",
        "mod contract;",
        "kani::assume(case < contract::CASE_COUNT);",
    ]
    .iter()
    .all(|required| source.matches(*required).count() == 1)
        && source.matches("kani::assume(").count() == 1
        && !source.contains("#[kani::stub")
}

fn native_profile_matches(positive: &str, negative: &str) -> bool {
    if !common_scope(positive) || !common_scope(negative) {
        return false;
    }
    let bodies: Vec<_> = positive.split("#[kani::proof]\n").skip(1).collect();
    if bodies.len() != HARNESSES.len()
        || positive.matches("#[kani::proof]").count() != HARNESSES.len()
        || negative.matches("#[kani::proof]").count() != 1
    {
        return false;
    }
    for (body, harness) in bodies.iter().zip(HARNESSES) {
        let header = format!("#[kani::unwind(32)]\nfn {}() {{", harness.name);
        if !body.starts_with(header.as_str())
            || body.matches(harness.checker).count() != 1
            || body.matches("kani::cover!(").count() != harness.covers.len()
        {
            return false;
        }
        for id in harness.covers {
            let quoted = format!("\"{id}\"");
            if body.matches(quoted.as_str()).count() != 1 {
                return false;
            }
        }
    }
    negative.contains("#[kani::proof]\n#[kani::unwind(32)]\nfn false_byte_scalar_boundary() {")
        && negative.matches("\"FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY\"").count() == 1
        && !negative.contains("kani::cover!(")
        && !positive.contains("FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY")
}

// This is the exact argv spelling accepted by the pinned Kani 0.68.0 source.
// It is a documentation contract, not a process runner or a tool-identity proof.
const COMMANDS: &str = "
kani crates/perl-line-index/tests/formal/line_index_kani.rs
-Z unstable-options -Z concrete-playback
--export-json target/formal/position-positive.json --concrete-playback=print
kani crates/perl-line-index/tests/formal/false_scalar_boundary.rs
-Z unstable-options -Z concrete-playback
--export-json target/formal/position-false-theorem.json --concrete-playback=print
";

fn documented_commands_match(readme: &str) -> bool {
    let mut native_blocks = 0;
    for section in readme.split("```bash\n").skip(1) {
        let Some((body, _)) = section.split_once("```") else {
            return false;
        };
        if !body.trim_start().starts_with("kani ") {
            continue;
        }
        native_blocks += 1;
        let tokens: Vec<_> = body.split_whitespace().filter(|token| *token != "\\").collect();
        if tokens != COMMANDS.split_whitespace().collect::<Vec<_>>() {
            return false;
        }
    }
    native_blocks == 1 && readme.contains("**Kani 0.68.0**")
}

#[test]
fn native_profile_sources_match_reviewed_harness_and_cover_inventory() {
    assert!(native_profile_matches(POSITIVE, NEGATIVE));
    assert_eq!(HARNESSES.iter().map(|harness| harness.covers.len()).sum::<usize>(), 18);
}

#[test]
fn native_profile_refuses_each_missing_duplicate_or_relabelled_cover() {
    for harness in HARNESSES {
        for id in harness.covers {
            let quoted = format!("\"{id}\"");
            for replacement in [String::new(), format!("{quoted}, {quoted}"), "\"FOREIGN\"".into()]
            {
                let changed = POSITIVE.replacen(quoted.as_str(), &replacement, 1);
                assert_ne!(changed, POSITIVE);
                assert!(!native_profile_matches(&changed, NEGATIVE), "accepted altered {id}");
            }
        }
    }
}

#[test]
fn native_profile_refuses_harness_scope_and_control_drift() {
    for (from, to) in [
        ("#[kani::proof]", "#[not_a_proof]"),
        ("#[kani::unwind(32)]", "#[kani::unwind(31)]"),
        ("target_pointer_width = \"64\"", "target_pointer_width = \"32\""),
        ("../../src/lib.rs", "copied_model.rs"),
        ("kani::assume(case < contract::CASE_COUNT);", "kani::assume(case < 1);"),
        ("contract::check_utf16_complete(", "unchecked_completeness("),
        ("fn strict_utf16_soundness()", "fn another_harness()"),
    ] {
        let changed = POSITIVE.replacen(from, to, 1);
        assert_ne!(changed, POSITIVE);
        assert!(!native_profile_matches(&changed, NEGATIVE), "accepted altered {from}");
    }
    assert!(!native_profile_matches(&format!("{POSITIVE}\n{NEGATIVE}"), NEGATIVE));
    let changed = NEGATIVE.replace("FV_POSITION_FALSE_BYTE_SCALAR_BOUNDARY", "WRONG_FAILURE");
    assert!(!native_profile_matches(POSITIVE, &changed));
    let narrowed = format!("{POSITIVE}\nkani::assume(false);\n");
    assert!(!native_profile_matches(&narrowed, NEGATIVE));
}

#[test]
fn documented_native_commands_match_pinned_kani_flags() {
    assert!(documented_commands_match(README));
}

#[test]
fn documented_native_commands_refuse_wrong_flags_or_disabled_checks() {
    for (from, to) in [
        ("-Z unstable-options", "-Z export-json"),
        ("-Z concrete-playback", ""),
        ("--concrete-playback=print", "--concrete-playback=inplace"),
        ("--concrete-playback=print", "--concrete-playback=print --tests"),
        ("--concrete-playback=print", "--concrete-playback=print --no-default-checks"),
        ("--concrete-playback=print", "--concrete-playback=print --no-unwinding-checks"),
        ("--concrete-playback=print", "--concrete-playback=print --no-assertion-reach-checks"),
        ("--concrete-playback=print", "--concrete-playback=print --only-codegen"),
        ("--concrete-playback=print", "--concrete-playback=print --harness absent"),
        ("position-false-theorem.json", "position-positive.json"),
    ] {
        let changed = README.replacen(from, to, 1);
        assert_ne!(changed, README);
        assert!(!documented_commands_match(&changed), "accepted command change {to:?}");
    }
}
