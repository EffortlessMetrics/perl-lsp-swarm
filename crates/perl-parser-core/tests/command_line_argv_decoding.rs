//! Behavioral contract for structured `argv` decoding of a Perl invocation.
//!
//! The expectations below were checked against `perl` 5.38.2. Where a case is
//! easy to get wrong, the comment names the real interpreter behavior the
//! assertion pins, so a future edit that "simplifies" the decoder into a
//! `getopt`-shaped one fails here rather than silently misreading command lines.
//!
//! These tests never execute `perl`; the interpreter was the oracle used to
//! write them, not a runtime dependency of the suite.
#![allow(
    clippy::panic,
    reason = "test-target failure reporting: `panic!` carries the decoded value into the \
              failure message, which `assert!` on a `matches!` cannot. Matches the \
              file-scoped convention already used by this crate's other suites (#11736)."
)]

use perl_parser_core::command_line::{
    Ambiguity, AmbiguityKind, ArgvLead, ArgvSpan, ContextFact, ContextFactKind,
    InvocationDecodeError, ModuleForm, ModuleImportAction, NeutralSwitch, PerlInvocation,
    ProgramSource, RecordSeparatorDigits, SourceSwitch, TerminatingActionKind,
    UnsupportedSwitchKind, decode,
};

/// Decode `argv` with a leading interpreter argument, expecting success.
fn perl(argv: &[&str]) -> PerlInvocation {
    let mut full = vec!["perl"];
    full.extend_from_slice(argv);
    match decode(&full, ArgvLead::Interpreter) {
        Ok(invocation) => invocation,
        Err(error) => panic!("expected {full:?} to decode, got {error}"),
    }
}

/// Decode `argv` with a leading interpreter argument, expecting refusal.
fn perl_error(argv: &[&str]) -> InvocationDecodeError {
    let mut full = vec!["perl"];
    full.extend_from_slice(argv);
    match decode(&full, ArgvLead::Interpreter) {
        Ok(invocation) => panic!("expected {full:?} to be refused, decoded {invocation:?}"),
        Err(error) => error,
    }
}

/// The fragment texts in `argv` order.
fn fragments(invocation: &PerlInvocation) -> Vec<(SourceSwitch, &str)> {
    invocation
        .source_fragments
        .iter()
        .map(|fragment| (fragment.switch, fragment.text.as_str()))
        .collect()
}

/// The context facts in `argv` order, without their spans.
fn facts(invocation: &PerlInvocation) -> Vec<ContextFactKind> {
    invocation.context_facts.iter().map(|fact| fact.kind.clone()).collect()
}

fn neutral(invocation: &PerlInvocation) -> Vec<NeutralSwitch> {
    invocation.neutral_switches.iter().map(|switch| switch.switch).collect()
}

fn program_arguments(invocation: &PerlInvocation) -> Vec<&str> {
    invocation.program_arguments.iter().map(|argument| argument.text.as_str()).collect()
}

fn find_fact<'a>(invocation: &'a PerlInvocation, wanted: &ContextFactKind) -> &'a ContextFact {
    match invocation.context_facts.iter().find(|fact| &fact.kind == wanted) {
        Some(fact) => fact,
        None => panic!("expected fact {wanted:?}, have {:?}", facts(invocation)),
    }
}

/// Every decoded value, paired with the span that claims to locate it.
///
/// The span contract is only worth something if it is checked against the
/// original bytes, which is what [`spans_locate_their_values`] and the property
/// test below do with this list.
fn located_values(invocation: &PerlInvocation) -> Vec<(String, ArgvSpan)> {
    let mut values = Vec::new();
    for fragment in &invocation.source_fragments {
        values.push((fragment.text.clone(), fragment.span));
    }
    for fact in &invocation.context_facts {
        match &fact.kind {
            ContextFactKind::SplitPattern { pattern } => values.push((pattern.clone(), fact.span)),
            ContextFactKind::IncludeDirectory { directory } => {
                values.push((directory.clone(), fact.span));
            }
            ContextFactKind::LineEnding { octal_digits: Some(digits) } => {
                values.push((digits.clone(), fact.span));
            }
            ContextFactKind::RecordSeparator { digits: Some(digits) } => {
                let text = match digits {
                    RecordSeparatorDigits::Octal(text) | RecordSeparatorDigits::Hex(text) => text,
                };
                values.push((text.clone(), fact.span));
            }
            ContextFactKind::ModuleImport { spec, .. } => {
                values.push((spec.module.clone(), spec.module_span));
                if let (Some(arguments), Some(span)) =
                    (&spec.import_arguments, spec.import_arguments_span)
                {
                    values.push((arguments.clone(), span));
                }
            }
            _ => {}
        }
    }
    for switch in &invocation.neutral_switches {
        if let Some(value) = &switch.value {
            values.push((value.clone(), switch.span));
        }
    }
    for switch in &invocation.unsupported_switches {
        values.push((switch.spelling.clone(), switch.span));
    }
    for argument in invocation.program_arguments.iter().chain(&invocation.uninterpreted_arguments) {
        values.push((argument.text.clone(), argument.span));
    }
    if let ProgramSource::ScriptFile { path, span } = &invocation.program {
        values.push((path.clone(), *span));
    }
    values
}

/// Assert that every decoded value is exactly the bytes its span names.
///
/// An empty value still has to carry a span that lands inside its argument:
/// an empty `-e ''` fragment, a bare `-F`, and `-MFoo=` all produce one, and
/// skipping them would leave that whole class of spans unchecked.
fn spans_locate_their_values(argv: &[&str], invocation: &PerlInvocation) {
    for (value, span) in located_values(invocation) {
        let argument = match argv.get(span.argument_index) {
            Some(argument) => *argument,
            None => panic!("span {span:?} names argument {} of {argv:?}", span.argument_index),
        };
        assert!(
            span.start <= span.end && span.end <= argument.len(),
            "span {span:?} is not inside {argument:?} (len {})",
            argument.len()
        );
        let slice = argument.get(span.start..span.end);
        assert_eq!(
            slice,
            Some(value.as_str()),
            "span {span:?} over {argument:?} should locate {value:?}"
        );
    }
}

// ---- source fragments --------------------------------------------------

#[test]
fn source_fragments_accept_attached_and_separate_values() {
    // `perl -eprint "x"` and `perl -e 'print "x"'` are both legal.
    let attached = perl(&["-eprint 1"]);
    assert_eq!(fragments(&attached), vec![(SourceSwitch::E, "print 1")]);
    let separate = perl(&["-e", "print 1"]);
    assert_eq!(fragments(&separate), vec![(SourceSwitch::E, "print 1")]);
    assert!(attached.is_one_liner() && separate.is_one_liner());
}

#[test]
fn capital_e_is_a_distinct_source_switch() {
    // `-E` also enables the feature bundle, so collapsing it into `-e` would
    // lose the only signal that `say` is available.
    let invocation = perl(&["-E", "say 1"]);
    assert_eq!(fragments(&invocation), vec![(SourceSwitch::BigE, "say 1")]);
}

#[test]
fn repeated_fragments_are_kept_separately_and_in_order() {
    // perl joins these with a newline; keeping them apart is what lets a later
    // layer map a composed offset back to the argument it came from.
    let invocation = perl(&["-e", "print 1", "-E", "say 2", "-eprint 3"]);
    assert_eq!(
        fragments(&invocation),
        vec![
            (SourceSwitch::E, "print 1"),
            (SourceSwitch::BigE, "say 2"),
            (SourceSwitch::E, "print 3"),
        ]
    );
}

#[test]
fn an_empty_fragment_is_a_fragment() {
    // `perl -e '' -e 'print 1'` runs; dropping the empty fragment would shift
    // every later fragment's line number.
    let invocation = perl(&["-e", "", "-e", "print 1"]);
    assert_eq!(fragments(&invocation), vec![(SourceSwitch::E, ""), (SourceSwitch::E, "print 1")]);
}

#[test]
fn a_fragment_is_taken_verbatim_even_when_it_looks_like_a_switch() {
    // Real behavior: `perl -e -w` runs the program `-w`. A decoder that skips
    // dash-prefixed arguments when looking for a value would lose the program.
    let invocation = perl(&["-e", "-w"]);
    assert_eq!(fragments(&invocation), vec![(SourceSwitch::E, "-w")]);
    assert!(neutral(&invocation).is_empty(), "`-w` here is source, not a switch");

    // Even the terminator: `perl -e -- 'print "x"'` compiles the program `--`
    // and reports `syntax error at -e line 1, at EOF`, leaving `print "x"` as a
    // program argument. Checking for `--` before reading `-e`'s value would
    // decode a command line perl does not run.
    let terminator_as_source = perl(&["-e", "--", "print \"x\""]);
    assert_eq!(fragments(&terminator_as_source), vec![(SourceSwitch::E, "--")]);
    assert_eq!(terminator_as_source.terminator, None);
    assert_eq!(program_arguments(&terminator_as_source), vec!["print \"x\""]);
}

#[test]
fn a_non_ascii_switch_letter_is_refused_without_splitting_a_character() {
    // perl: `Unrecognized switch: -é`. The span must cover the whole character,
    // not its first byte.
    match decode(&["perl", "-é", "-e", "print"], ArgvLead::Interpreter) {
        Err(InvocationDecodeError::UnrecognizedSwitch { switch, span }) => {
            assert_eq!(switch, 'é');
            assert_eq!(span, ArgvSpan::new(1, 1, 3), "`é` is two bytes wide");
        }
        other => panic!("expected an unrecognized-switch refusal, got {other:?}"),
    }
}

#[test]
fn non_ascii_fragments_survive_decoding_with_byte_accurate_spans() {
    let argv = ["perl", "-e", "print \"héllo ☃\\n\""];
    let invocation = match decode(&argv, ArgvLead::Interpreter) {
        Ok(invocation) => invocation,
        Err(error) => panic!("expected {argv:?} to decode, got {error}"),
    };
    assert_eq!(fragments(&invocation), vec![(SourceSwitch::E, "print \"héllo ☃\\n\"")]);
    spans_locate_their_values(&argv, &invocation);
}

// ---- cluster bundling --------------------------------------------------

#[test]
fn lane_bundles_into_four_switches() {
    // `-lane` is `-l -a -n -e`, and the `-e` value is the next argument.
    let invocation = perl(&["-lane", "print $F[0]"]);
    assert_eq!(
        facts(&invocation),
        vec![
            ContextFactKind::LineEnding { octal_digits: None },
            ContextFactKind::AutoSplit,
            ContextFactKind::ReadLoop,
        ]
    );
    assert_eq!(fragments(&invocation), vec![(SourceSwitch::E, "print $F[0]")]);
}

#[test]
fn digit_runs_are_consumed_before_bundling_resumes() {
    // `-0777ne` is `-0777 -n -e`: the digits belong to `-0`, and `n` and `e`
    // are still switches. Reading `777ne` as one value loses the program.
    let invocation = perl(&["-0777ne", "print"]);
    assert_eq!(
        facts(&invocation),
        vec![
            ContextFactKind::RecordSeparator {
                digits: Some(RecordSeparatorDigits::Octal("777".to_owned()))
            },
            ContextFactKind::ReadLoop,
        ]
    );
    assert_eq!(fragments(&invocation), vec![(SourceSwitch::E, "print")]);
}

#[test]
fn line_ending_digits_are_octal_only() {
    // `perl -l8` is rejected by perl as `Unrecognized switch: -8`, because `-l`
    // takes octal digits and `8` is not one.
    let with_digits = perl(&["-l012", "-e", "print"]);
    assert_eq!(
        facts(&with_digits)[0],
        ContextFactKind::LineEnding { octal_digits: Some("012".to_owned()) }
    );
    assert!(matches!(
        perl_error(&["-l8", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '8', .. }
    ));
}

#[test]
fn octal_digit_runs_stop_where_perl_stops_reading_them() {
    // perl.c reads `3 + (*s == '0')` octal digits after `-l`, and three after
    // `-0`. So `perl -l1234` reports `Unrecognized switch: -4`, and
    // `perl -l0123` is a single four-digit value. An unbounded run would accept
    // command lines perl refuses outright.
    let leading_zero = perl(&["-l0123", "-e", "print"]);
    assert_eq!(
        facts(&leading_zero)[0],
        ContextFactKind::LineEnding { octal_digits: Some("0123".to_owned()) },
        "`-l` reads a fourth digit only when the run starts with `0`"
    );
    assert!(matches!(
        perl_error(&["-l1234", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '4', .. }
    ));
    assert!(matches!(
        perl_error(&["-l01234", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '4', .. }
    ));

    // `-0` has no leading-zero exception: perl reads three digits either way,
    // so `perl -00123` reports `Unrecognized switch: -3`.
    let separator = perl(&["-0777", "-e", "print"]);
    assert_eq!(
        facts(&separator)[0],
        ContextFactKind::RecordSeparator {
            digits: Some(RecordSeparatorDigits::Octal("777".to_owned()))
        }
    );
    for overlong in [vec!["-07777"], vec!["-01234"]] {
        let mut argv = overlong.clone();
        argv.extend_from_slice(&["-e", "print"]);
        assert!(
            matches!(
                decode(
                    &{
                        let mut full = vec!["perl"];
                        full.extend_from_slice(&argv);
                        full
                    },
                    ArgvLead::Interpreter
                ),
                Err(InvocationDecodeError::UnrecognizedSwitch { .. })
            ),
            "{overlong:?} leaves a digit perl refuses"
        );
    }
    assert!(matches!(
        perl_error(&["-00123", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '3', .. }
    ));
}

#[test]
fn a_value_taking_switch_swallows_the_rest_of_its_cluster() {
    // `perl -ine 'print'` is `-i` with the backup extension `ne`, and then
    // `print` is a *file name*, not a program. perl reports
    // `Can't open perl script "print"`. Treating `-i` as valueless would turn
    // this into a one-liner that was never asked for.
    let invocation = perl(&["-ine", "print"]);
    assert_eq!(neutral(&invocation), vec![NeutralSwitch::InPlaceEdit]);
    assert_eq!(invocation.neutral_switches[0].value.as_deref(), Some("ne"));
    assert!(!invocation.is_one_liner());
    assert!(matches!(
        &invocation.program,
        ProgramSource::ScriptFile { path, .. } if path == "print"
    ));
}

#[test]
fn unicode_flags_take_their_cluster_tail() {
    let invocation = perl(&["-CSD", "-e", "print"]);
    assert_eq!(neutral(&invocation), vec![NeutralSwitch::UnicodeFlags]);
    assert_eq!(invocation.neutral_switches[0].value.as_deref(), Some("SD"));
}

#[test]
fn configuration_and_debugger_take_a_value_only_in_their_colon_form() {
    // `perl -V:osname` queries one variable, but `perl -Vfoo` reports
    // `Unrecognized switch: -oo` — `-V` did not swallow `foo`, it decoded `-f`
    // and then choked on `oo`. Same shape for `-d`: `-d:Trace` loads
    // Devel::Trace, while `-dx` is `-d` followed by the `-x` switch.
    let query = perl(&["-V:osname", "-e", "print"]);
    assert_eq!(neutral(&query), vec![NeutralSwitch::Configuration]);
    assert_eq!(query.neutral_switches[0].value.as_deref(), Some(":osname"));

    let bundled = perl(&["-Vf", "-e", "print"]);
    assert_eq!(
        neutral(&bundled),
        vec![NeutralSwitch::Configuration, NeutralSwitch::NoSiteCustomize],
        "`-V` leaves `f` to the cluster"
    );
    assert_eq!(bundled.neutral_switches[0].value, None);

    // perl reports `Unrecognized switch: -oo` here, so the `o` must survive.
    assert!(matches!(
        perl_error(&["-Vfoo", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: 'o', .. }
    ));

    // `-d[t][:MOD]`: the optional `t` belongs to `-d`. `perl -dt:Trace` still
    // loads Devel::Trace, so reading `t` as the `-t` taint switch would both
    // invent a taint flag and then refuse the `:` that follows.
    for (argv, value) in [
        (vec!["-d:Trace"], Some(":Trace")),
        (vec!["-dt:Trace"], Some("t:Trace")),
        (vec!["-dt"], Some("t")),
        (vec!["-d"], None),
    ] {
        let mut full = argv.clone();
        full.extend_from_slice(&["-e", "print"]);
        let invocation = perl(&full);
        assert_eq!(neutral(&invocation), vec![NeutralSwitch::Debugger], "{argv:?}");
        assert_eq!(invocation.neutral_switches[0].value.as_deref(), value, "{argv:?}");
    }

    // But only that much: `perl -dtx` still ends with the `-x` switch.
    let debugger_bundled = perl(&["-dtx", "script.pl"]);
    assert_eq!(neutral(&debugger_bundled), vec![NeutralSwitch::Debugger]);
    assert_eq!(debugger_bundled.neutral_switches[0].value.as_deref(), Some("t"));
    assert_eq!(
        debugger_bundled.unsupported_switches[0].kind,
        UnsupportedSwitchKind::ScriptTextOffset,
        "`x` after `-dt` is still the -x switch"
    );

    // `perl -VZ` reports `Unrecognized switch: -Z`, so `-V` must not eat it.
    assert!(matches!(
        perl_error(&["-VZ", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: 'Z', .. }
    ));
}

#[test]
fn unicode_option_values_perl_rejects_are_refused() {
    // perl: `Unknown Unicode option letter 'f'.` The accepted alphabet is
    // `aioADEILOS`, or a decimal count; perl refuses the mixed form too
    // (`-C7S` reports `Unknown Unicode option letter 'S'`).
    for accepted in [vec!["-CSD"], vec!["-CIOE"], vec!["-C7"], vec!["-C255"], vec!["-C"]] {
        let mut argv = accepted.clone();
        argv.extend_from_slice(&["-e", "print"]);
        let invocation = perl(&argv);
        assert_eq!(neutral(&invocation), vec![NeutralSwitch::UnicodeFlags], "{accepted:?}");
    }
    assert!(matches!(
        perl_error(&["-Cfoo", "-e", "print"]),
        InvocationDecodeError::UnknownUnicodeOption { character: 'f', .. }
    ));
    assert!(matches!(
        perl_error(&["-C7S", "-e", "print"]),
        InvocationDecodeError::UnknownUnicodeOption { character: 'S', .. }
    ));

    // The numeric form has its own two refusals, which perl reports separately:
    // `Invalid number '0777' for -C option` (a leading zero is a malformed
    // decimal here, not an octal escape) and `Unknown Unicode option value 512`.
    for malformed in ["-C0777", "-C007", "-C99999999999"] {
        assert!(
            matches!(
                perl_error(&[malformed, "-e", "print"]),
                InvocationDecodeError::MalformedUnicodeOptionNumber { .. }
            ),
            "{malformed} should be refused as an unreadable number"
        );
    }
    for unsupported in ["-C512", "-C1023"] {
        assert!(
            matches!(
                perl_error(&[unsupported, "-e", "print"]),
                InvocationDecodeError::UnsupportedUnicodeOptionValue { .. }
            ),
            "{unsupported} sets bits perl does not define"
        );
    }
    // 511 is the largest value perl defines, and `-C0` is the one legal
    // leading zero.
    for accepted in ["-C511", "-C0", "-C500"] {
        let invocation = perl(&[accepted, "-e", "print"]);
        assert_eq!(neutral(&invocation), vec![NeutralSwitch::UnicodeFlags], "{accepted}");
    }
}

// ---- record separator radix -------------------------------------------

#[test]
fn hexadecimal_record_separators_need_the_marker_and_a_digit() {
    let hexadecimal = perl(&["-0x41", "-e", "print"]);
    assert_eq!(
        facts(&hexadecimal)[0],
        ContextFactKind::RecordSeparator {
            digits: Some(RecordSeparatorDigits::Hex("41".to_owned()))
        }
    );

    // `perl -0x -e1` reports `No Perl script found in input`: the `x` was the
    // `-x` switch, not an empty hex separator. Accepting a bare `x` here would
    // silently swallow a switch.
    let bare_marker = perl(&["-0x", "-e", "print"]);
    assert_eq!(facts(&bare_marker)[0], ContextFactKind::RecordSeparator { digits: None });
    assert_eq!(bare_marker.unsupported_switches.len(), 1);
    assert_eq!(bare_marker.unsupported_switches[0].kind, UnsupportedSwitchKind::ScriptTextOffset);

    // The marker needs hex digits all the way to the end of the cluster.
    // `perl -0x41n` reports `No Perl script found in input`, so perl read `-x`
    // with the value `41n` — not a `0x41` separator followed by `-n`. Taking
    // the hex prefix would invent a read loop that perl never runs.
    let poisoned_tail = perl(&["-0x41n", "script.pl"]);
    assert_eq!(facts(&poisoned_tail), vec![ContextFactKind::RecordSeparator { digits: None }]);
    assert_eq!(poisoned_tail.unsupported_switches[0].spelling, "x41n");
    assert!(
        !facts(&poisoned_tail).contains(&ContextFactKind::ReadLoop),
        "`n` inside the -x value is not a read loop"
    );

    // Only lowercase `x` marks the hexadecimal form. `perl -0X41` reports
    // `Unrecognized switch: -41`, because `X` is the -X switch and `41` is then
    // read as switches.
    assert!(matches!(
        perl_error(&["-0X41", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '4', .. }
    ));

    // Long hex runs are not capped the way octal is.
    let long_hex = perl(&["-0x000041", "-e", "print"]);
    assert_eq!(
        facts(&long_hex)[0],
        ContextFactKind::RecordSeparator {
            digits: Some(RecordSeparatorDigits::Hex("000041".to_owned()))
        }
    );
}

#[test]
fn uppercase_hex_marker_is_not_accepted_and_numeric_module_components_are_plain() {
    assert!(matches!(
        perl_error(&["-0X41", "script.pl"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '4', .. }
    ));

    for module in ["Foo::1", "Foo::1::2"] {
        let argument = format!("-M{module}");
        let invocation = perl(&[argument.as_str(), "-e", "print"]);
        let ContextFactKind::ModuleImport { spec, .. } = &invocation.context_facts[0].kind else {
            panic!("expected a module import");
        };
        assert!(spec.module_is_plain_name, "{module} is a valid module name");
        assert!(invocation.ambiguities.is_empty());
    }
}

#[test]
fn record_separator_digits_stop_at_the_first_non_octal_digit() {
    // perl reports `Unrecognized switch: -9` for `-09`.
    assert!(matches!(
        perl_error(&["-09", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: '9', .. }
    ));
}

#[test]
fn slurp_mode_has_its_own_switch() {
    let invocation = perl(&["-g", "-e", "print"]);
    assert_eq!(facts(&invocation), vec![ContextFactKind::SlurpMode]);
}

// ---- attached-only versus separate values -----------------------------

#[test]
fn include_directories_accept_attached_and_separate_values() {
    for argv in [vec!["-Ilib", "-e", "print"], vec!["-I", "lib"]] {
        let invocation = perl(&argv);
        assert_eq!(
            find_fact(
                &invocation,
                &ContextFactKind::IncludeDirectory { directory: "lib".to_owned() }
            )
            .kind,
            ContextFactKind::IncludeDirectory { directory: "lib".to_owned() }
        );
    }
}

#[test]
fn module_imports_do_not_accept_a_separate_value() {
    // `perl -M strict -e1` fails with `Missing argument to -M.`; a decoder that
    // helpfully consumed the next argument would accept a command line perl
    // refuses, and would then lose `strict` as a program argument.
    assert!(matches!(
        perl_error(&["-M", "strict", "-e", "print"]),
        InvocationDecodeError::MissingValue { switch: 'M', .. }
    ));
    assert!(matches!(
        perl_error(&["-m", "strict"]),
        InvocationDecodeError::MissingValue { switch: 'm', .. }
    ));
}

#[test]
fn in_place_editing_does_not_accept_a_separate_value() {
    // `perl -i .bak -e1` makes `.bak` the script name.
    let invocation = perl(&["-i", ".bak"]);
    assert_eq!(invocation.neutral_switches[0].value, None);
    assert!(matches!(
        &invocation.program,
        ProgramSource::ScriptFile { path, .. } if path == ".bak"
    ));
}

#[test]
fn split_patterns_are_attached_and_may_look_like_switches() {
    let invocation = perl(&["-F-l", "-ane", "print"]);
    assert_eq!(facts(&invocation)[0], ContextFactKind::SplitPattern { pattern: "-l".to_owned() });
    // The pattern is not re-scanned for switches.
    assert_eq!(facts(&invocation).len(), 3, "only -F, -a and -n are facts");
}

#[test]
fn a_punctuation_split_pattern_is_kept_verbatim() {
    let invocation = perl(&["-F/,/", "-ane", "print"]);
    assert_eq!(facts(&invocation)[0], ContextFactKind::SplitPattern { pattern: "/,/".to_owned() });
}

#[test]
fn a_bare_split_pattern_is_reported_as_ambiguous() {
    // perl accepts `-F` with nothing attached, but what it splits on is then
    // not determined by argv, so the decoder says so rather than inventing one.
    let invocation = perl(&["-F", "-ane", "print"]);
    assert_eq!(facts(&invocation)[0], ContextFactKind::SplitPattern { pattern: String::new() });
    assert!(
        invocation
            .ambiguities
            .iter()
            .any(|ambiguity| ambiguity.kind == AmbiguityKind::EmptySplitPattern)
    );
}

// ---- module expressions ------------------------------------------------

#[test]
fn module_imports_decode_form_negation_and_import_arguments() {
    let plain = perl(&["-Mstrict", "-e", "print"]);
    assert_eq!(
        facts(&plain)[0],
        ContextFactKind::ModuleImport {
            form: ModuleForm::Use,
            spec: module_spec("strict", None, false, true, ArgvSpan::new(1, 2, 8), None),
        }
    );

    let negated = perl(&["-M-strict", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&negated)[0] else {
        panic!("expected a module import");
    };
    assert!(spec.negated, "`-M-strict` is `no strict`");
    assert_eq!(spec.module, "strict");

    let without_import = perl(&["-mFoo", "-e", "print"]);
    let ContextFactKind::ModuleImport { form, .. } = &facts(&without_import)[0] else {
        panic!("expected a module import");
    };
    assert_eq!(*form, ModuleForm::UseSuppressingDefaultImport);
}

#[test]
fn import_arguments_are_split_from_the_module_at_the_first_equals() {
    let invocation = perl(&["-MList::Util=first,max", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[0] else {
        panic!("expected a module import");
    };
    assert_eq!(spec.module, "List::Util");
    assert_eq!(spec.import_arguments.as_deref(), Some("first,max"));
    assert!(spec.module_is_plain_name);
    assert!(invocation.ambiguities.is_empty());
    spans_locate_their_values(&["perl", "-MList::Util=first,max", "-e", "print"], &invocation);
}

#[test]
fn a_module_expression_that_is_not_a_module_name_is_reported_as_ambiguous() {
    // perl compiles `use <text>;` whatever the text is, so `-M'strict refs'`
    // works and `-M'Foo; ...'` would run arbitrary code at compile time.
    // Reporting it as a resolved module name would hide that.
    let invocation = perl(&["-Mstrict refs", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[0] else {
        panic!("expected a module import");
    };
    assert!(!spec.module_is_plain_name);
    assert_eq!(
        invocation.ambiguities.iter().map(|ambiguity| ambiguity.kind).collect::<Vec<_>>(),
        vec![AmbiguityKind::ModuleExpressionIsNotAModuleName]
    );

    // Whitespace is not the discriminator, and treating it as one would be a
    // security bug: `perl -M'strict;print 99' -e 'print "END\n"'` prints
    // `99END`, so a semicolon with no space runs injected code at compile time.
    let injected = perl(&["-Mstrict;print 99", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&injected)[0] else {
        panic!("expected a module import");
    };
    assert!(!spec.module_is_plain_name, "`strict;print 99` is code, not a module name");
    assert_eq!(
        injected.ambiguities.iter().map(|ambiguity| ambiguity.kind).collect::<Vec<_>>(),
        vec![AmbiguityKind::ModuleExpressionIsNotAModuleName]
    );

    // Only the first component carries an identifier-start rule. perl loads
    // `Foo::1` and even `Foo::` (as `Foo/.pm`), so reporting those as arbitrary
    // code would be a false positive on ordinary module names — the failure
    // mode this flag can least afford, since consumers use it to spot injection.
    for plain in ["Foo::1", "Foo::1Bar", "Foo::_x", "_Foo", "Foo9", "Foo::Bar::9", "Foo::"] {
        let argv = format!("-M{plain}");
        let invocation = perl(&[argv.as_str(), "-e", "print"]);
        let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[0] else {
            panic!("expected a module import");
        };
        assert!(spec.module_is_plain_name, "perl loads {plain:?} as a module");
        assert!(invocation.ambiguities.is_empty(), "{plain:?} raised a false ambiguity");
    }

    // `perl -M::Foo` is refused outright, `1Foo` is a syntax error, and
    // `Foo-Bar` compiles as an expression (`use Foo` minus `Bar`).
    //
    // The non-ASCII rows matter because perl's option scan is byte-wise: it
    // stops at the first non-ASCII byte and `-M` splices the rest in verbatim,
    // so what reaches the lexer is not a bareword. Verified against 5.38.2,
    // with no `use utf8` in play:
    //
    //   perl -MFooα     -e1 → Unrecognized character \xCE ... after use Foo
    //   perl -MFoo٢     -e1 → Unrecognized character \xD9 ... after use Foo
    //   perl -MFoo::Barα -e1 → Unrecognized character \xCE ... after Foo::Bar
    //   perl -MFoo::٢   -e1 → Unrecognized character \xD9 ... after use Foo::
    //   perl -MFoo2     -e1 → Can't locate Foo2.pm   (the ASCII control)
    for opaque in ["::Foo", "1Foo", "Foo-Bar", "Foo::٢", "Fooα", "Foo٢", "Foo::Barα"] {
        let argv = format!("-M{opaque}");
        let invocation = perl(&[argv.as_str(), "-e", "print"]);
        let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[0] else {
            panic!("expected a module import");
        };
        assert!(!spec.module_is_plain_name, "{opaque:?} should not read as a module name");
    }
}

#[test]
fn an_earlier_utf8_pragma_makes_later_unicode_module_names_plain() {
    // `-M` splices what perl's byte-wise option scan leaves behind into a `use`
    // statement, which the lexer then reads under whatever pragmas the earlier
    // `-M` arguments already put in force. So the same argument is a module
    // name or a syntax error depending on what came before it. Against 5.38.2:
    //
    //   perl -Mutf8 -MFooα -e1 → Can't locate Fooα.pm in @INC
    //   perl -MFooα -Mutf8 -e1 → Unrecognized character \xCE  (order matters)
    //
    // The pragma is keyed to the call perl makes, not the spelling:
    //
    //   -Mutf8   → Import   → enabled
    //   -Mutf8=x → Import   → enabled
    //   -mutf8   → NoCall   → unchanged (`use utf8 ()` runs no import)
    //   -M-utf8  → Unimport → disabled
    for (before, expected_plain) in [
        (vec!["-Mutf8"], true),
        (vec!["-Mutf8=x"], true),
        (vec!["-mutf8"], false),
        (vec!["-M-utf8"], false),
        (vec!["-Mutf8", "-M-utf8"], false),
        (vec!["-M-utf8", "-Mutf8"], true),
        (vec![], false),
    ] {
        let mut argv = before.clone();
        argv.push("-MFooα");
        argv.extend_from_slice(&["-e", "print"]);
        let invocation = perl(&argv);
        let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[before.len()] else {
            panic!("expected a module import after {before:?}");
        };
        assert_eq!(spec.module, "Fooα");
        assert_eq!(
            spec.module_is_plain_name, expected_plain,
            "`Fooα` after {before:?} should have plain={expected_plain}",
        );
    }

    // Under the pragma a Unicode *letter* leads a later component but a Unicode
    // *digit* still does not, and the first component is always ASCII because
    // perl's option scan never reads past a non-ASCII byte:
    //
    //   perl -Mutf8 -MFoo::α  -e1 → Can't locate Foo/α.pm
    //   perl -Mutf8 -MFoo::α٢ -e1 → Can't locate Foo/α٢.pm
    //   perl -Mutf8 -MFoo::٢  -e1 → Unrecognized character \x{662}
    //   perl -Mutf8 -Mαβ      -e1 → Module name required with -M option.
    for (module, expected_plain) in
        [("Foo::α", true), ("Foo::α٢", true), ("Fooα٢", true), ("Foo::٢", false)]
    {
        let argv = format!("-M{module}");
        let invocation = perl(&["-Mutf8", argv.as_str(), "-e", "print"]);
        let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[1] else {
            panic!("expected a module import for {module}");
        };
        assert_eq!(
            spec.module_is_plain_name, expected_plain,
            "`{module}` under utf8 should have plain={expected_plain}",
        );
    }
    assert!(matches!(
        perl_error(&["-Mutf8", "-Mαβ", "-e", "print"]),
        InvocationDecodeError::EmptyModuleName { switch: 'M', .. }
    ));

    // The `-m` spelling is refused at option-parse time either way: perl reports
    // `Can't use '...' after -mname` for both `perl -mFooα` and
    // `perl -Mutf8 -mFooα`, so the pragma does not reach it.
    for argv in [vec!["-mFooα", "-e", "print"], vec!["-Mutf8", "-mFooα", "-e", "print"]] {
        assert!(
            matches!(
                perl_error(&argv),
                InvocationDecodeError::UnexpectedModuleSuffix { character: 'α', .. }
            ),
            "{argv:?} must be refused at the option level",
        );
    }
}

#[test]
fn an_empty_module_name_is_refused() {
    // perl: `Module name required with -M option.`
    assert!(matches!(
        perl_error(&["-M=Foo", "-e", "print"]),
        InvocationDecodeError::EmptyModuleName { switch: 'M', .. }
    ));
}

#[test]
fn lowercase_module_switch_still_imports_with_explicit_arguments() {
    // `-m` suppresses the *default* import only, and negation swaps the call
    // for `unimport`. All six rows verified against the interpreter using
    // `strict`, whose import and unimport are both observable through whether
    // `$x = 1` is refused:
    //
    //   -Mstrict       refused   Import
    //   -M-strict      allowed   Unimport
    //   -M-strict=vars allowed   Unimport
    //   -mstrict       allowed   NoCall    (use strict ())
    //   -m-strict      refused   NoCall    (no strict () calls nothing)
    //   -m-strict=vars allowed   Unimport
    for (argv, expected) in [
        ("-Mstrict", ModuleImportAction::Import),
        ("-MPOSIX=floor", ModuleImportAction::Import),
        ("-M-strict", ModuleImportAction::Unimport),
        ("-M-strict=vars", ModuleImportAction::Unimport),
        ("-mstrict", ModuleImportAction::NoCall),
        ("-m-strict", ModuleImportAction::NoCall),
        ("-mPOSIX=floor", ModuleImportAction::Import),
        ("-m-strict=vars", ModuleImportAction::Unimport),
    ] {
        let invocation = perl(&[argv, "-e", "print"]);
        let ContextFactKind::ModuleImport { form, spec } = &facts(&invocation)[0] else {
            panic!("expected a module import for {argv}");
        };
        assert_eq!(spec.import_action(*form), expected, "{argv}");
    }

    // The spelling is still recorded separately from the effect: `-mFoo=a,b`
    // is written with the suppressing form yet calls `import`.
    let with_arguments = perl(&["-mPOSIX=floor", "-e", "print"]);
    let ContextFactKind::ModuleImport { form, spec } = &facts(&with_arguments)[0] else {
        panic!("expected a module import");
    };
    assert_eq!(*form, ModuleForm::UseSuppressingDefaultImport);
    assert_eq!(spec.import_arguments.as_deref(), Some("floor"));
}

#[test]
fn an_argument_that_is_not_a_module_name_claims_no_method_call() {
    // `-M` splices its argument into a `use`/`no` statement, and two accepted
    // shapes reach a compiler path with no method dispatch in it.
    //
    // A version declaration is the sharp case. Verified against perl 5.38.2:
    //
    //   perl -MNoSuchModule -e1  → Can't locate NoSuchModule.pm in @INC
    //   perl -M5.010        -e1  → no error and no @INC lookup at all
    //   perl -M5.010 -e 'say 1'  → say compiles, so the feature bundle is on,
    //                              which only `use VERSION` does
    //   perl -M5.010 -e '$x = 1' → allowed, so strict is off: nothing imported
    //   perl -M-5.010       -e1  → Perls since v5.10.0 too modern
    //                              (a version ceiling, not an unimport)
    //   perl -Mv5.10 -e 'say 1'  → same, in vstring spelling
    //
    // Arbitrary code is the other: `-M'strict;print 99'` compiles two
    // statements, and whether the first imports is a question about Perl
    // source rather than about argv.
    for argv in ["-M5.010", "-M-5.010", "-Mv5.10", "-M-v5.10", "-M5.010=foo", "-Mstrict;print 99"] {
        let invocation = perl(&[argv, "-e", "print"]);
        let ContextFactKind::ModuleImport { form, spec } = &facts(&invocation)[0] else {
            panic!("expected a module import for {argv}");
        };
        assert!(!spec.module_is_plain_name, "{argv} is not a module name");
        assert_eq!(
            spec.import_action(*form),
            ModuleImportAction::Undetermined,
            "{argv} must not claim a method call",
        );
    }

    // The `-m` spelling never reaches a version declaration: perl refuses the
    // `.` before it ever builds a statement.
    assert!(
        matches!(
            perl_error(&["-m5.010", "-e", "print"]),
            InvocationDecodeError::UnexpectedModuleSuffix { character: '.', .. }
        ),
        "perl -m5.010 dies with `Can't use '.' after -mname`",
    );

    // Negative control: the same accessor still answers for a plain name, so
    // `Undetermined` is not a blanket refusal.
    let plain = perl(&["-Mstrict", "-e", "print"]);
    let ContextFactKind::ModuleImport { form, spec } = &facts(&plain)[0] else {
        panic!("expected a module import");
    };
    assert_eq!(spec.import_action(*form), ModuleImportAction::Import);
}

#[test]
fn module_names_perl_refuses_at_option_parsing_are_refused() {
    // perl reads word characters and `::` here, then croaks
    // `Module name required with -M option.` if it read nothing.
    for malformed in ["-M+Foo", "-M--Foo", "-M.Foo", "-M Foo", "-M=Foo", "-M'Foo"] {
        assert!(
            matches!(
                perl_error(&[malformed, "-e", "print"]),
                InvocationDecodeError::EmptyModuleName { switch: 'M', .. }
            ),
            "{malformed} should be refused as a missing module name"
        );
    }
    assert!(matches!(
        perl_error(&["-m+Foo", "-e", "print"]),
        InvocationDecodeError::EmptyModuleName { switch: 'm', .. }
    ));

    // A lone `:` is its own refusal: perl reports
    // `Invalid module name :Foo with -M option: contains single ':'`.
    assert!(matches!(
        perl_error(&["-M:Foo", "-e", "print"]),
        InvocationDecodeError::SingleColonInModuleName { switch: 'M', .. }
    ));

    // `-M-Foo` is still the negated form, not a malformed one.
    let negated = perl(&["-M-strict", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&negated)[0] else {
        panic!("expected a module import");
    };
    assert!(spec.negated);
}

#[test]
fn lowercase_module_switch_refuses_a_suffix_that_is_not_import_arguments() {
    // perl: `Can't use ';' after -mname.` — only `=` may follow the name under
    // `-m`, while `-M` splices the same text into the use statement.
    for (argv, character) in [("-mFoo;print 1", ';'), ("-mFoo Bar", ' '), ("-mFoo'Bar", '\'')] {
        assert!(
            matches!(
                perl_error(&[argv, "-e", "print"]),
                InvocationDecodeError::UnexpectedModuleSuffix { character: found, .. }
                    if found == character
            ),
            "{argv} should be refused on {character:?}"
        );
    }

    // The same tails are legal under `-M`.
    for accepted in ["-MFoo;print 1", "-MFoo Bar"] {
        let invocation = perl(&[accepted, "-e", "print"]);
        assert!(
            matches!(&facts(&invocation)[0], ContextFactKind::ModuleImport { .. }),
            "{accepted} decodes under -M"
        );
    }
}

#[test]
fn the_legacy_apostrophe_is_a_package_separator_not_arbitrary_code() {
    // `perl -MFoo'Bar` loads Foo::Bar with `Old package separator "'" deprecated`,
    // so flagging it as arbitrary code is a false positive on a real import.
    let invocation = perl(&["-MFoo'Bar", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&invocation)[0] else {
        panic!("expected a module import");
    };
    assert_eq!(spec.module, "Foo'Bar");
    assert!(spec.module_is_plain_name, "the apostrophe is a package separator");
    assert!(invocation.ambiguities.is_empty());

    // A trailing apostrophe is not a name: `perl -MFoo'` dies with
    // `Can't find string terminator "'"`, because it opens a string.
    let unterminated = perl(&["-MFoo'", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&unterminated)[0] else {
        panic!("expected a module import");
    };
    assert!(!spec.module_is_plain_name, "a trailing apostrophe opens a string");

    // Whereas a trailing `::` still names a module (`Foo::` loads `Foo/.pm`).
    let trailing_colons = perl(&["-MFoo::", "-e", "print"]);
    let ContextFactKind::ModuleImport { spec, .. } = &facts(&trailing_colons)[0] else {
        panic!("expected a module import");
    };
    assert!(spec.module_is_plain_name);
}

// ---- terminator, operands and program arguments ------------------------

#[test]
fn the_terminator_ends_switch_scanning() {
    let invocation = perl(&["-e", "print", "--", "-foo", "bar"]);
    assert_eq!(invocation.terminator, Some(ArgvSpan::new(3, 0, 2)));
    assert_eq!(program_arguments(&invocation), vec!["-foo", "bar"]);
    assert!(neutral(&invocation).is_empty());
}

#[test]
fn switch_scanning_continues_past_a_fragment_until_an_operand() {
    // `perl -e '...' -w foo` really does enable warnings: `-w` is still a
    // switch. Only `foo`, the first non-switch argument, ends scanning.
    let invocation = perl(&["-e", "print", "-w", "foo", "-bar"]);
    assert_eq!(neutral(&invocation), vec![NeutralSwitch::Warnings]);
    assert_eq!(program_arguments(&invocation), vec!["foo", "-bar"]);
}

#[test]
fn an_explicit_script_file_is_not_a_one_liner() {
    // After the program file, every remaining argument belongs to the program,
    // including one that looks like a switch.
    let invocation = perl(&["script.pl", "-w", "foo"]);
    assert!(!invocation.is_one_liner());
    assert!(matches!(
        &invocation.program,
        ProgramSource::ScriptFile { path, .. } if path == "script.pl"
    ));
    assert_eq!(program_arguments(&invocation), vec!["-w", "foo"]);
    assert!(neutral(&invocation).is_empty(), "`-w` after the script is a program argument");
}

#[test]
fn the_terminator_can_make_a_switch_shaped_operand_the_script() {
    // `perl -- -e 'print 1'` reports `Can't open perl script "-e"`.
    let invocation = perl(&["--", "-e", "print 1"]);
    assert!(!invocation.is_one_liner());
    assert!(matches!(
        &invocation.program,
        ProgramSource::ScriptFile { path, .. } if path == "-e"
    ));
    assert_eq!(program_arguments(&invocation), vec!["print 1"]);
}

#[test]
fn a_bare_dash_is_a_standard_input_program_not_a_switch() {
    let invocation = perl(&["-", "-e", "1"]);
    assert!(!invocation.is_one_liner());
    assert!(matches!(&invocation.program, ProgramSource::StandardInput { .. }));
    assert_eq!(program_arguments(&invocation), vec!["-e", "1"]);
}

#[test]
fn switches_without_a_program_leave_the_program_unspecified() {
    let invocation = perl(&["-w"]);
    assert_eq!(invocation.program, ProgramSource::Unspecified);
    assert!(!invocation.is_one_liner());
    let bare = perl(&[]);
    assert_eq!(bare.program, ProgramSource::Unspecified);
    assert!(bare.program_arguments.is_empty());
}

// ---- neutral and unsupported switches ----------------------------------

#[test]
fn recognized_analysis_neutral_switches_are_kept_but_produce_no_facts() {
    let invocation = perl(&["-cwT", "-e", "print"]);
    assert_eq!(
        neutral(&invocation),
        vec![NeutralSwitch::CompileOnly, NeutralSwitch::Warnings, NeutralSwitch::TaintChecks]
    );
    assert!(invocation.context_facts.is_empty());
}

#[test]
fn long_switches_are_limited_to_the_two_perl_accepts() {
    let invocation = perl(&["--version"]);
    assert_eq!(
        invocation.terminating_action.map(|action| action.kind),
        Some(TerminatingActionKind::Version)
    );
    assert!(matches!(
        perl_error(&["--foo", "-e", "print"]),
        InvocationDecodeError::UnrecognizedLongSwitch { .. }
    ));
}

#[test]
fn help_and_version_stop_decoding_because_perl_stops() {
    // `perl -v -Z` prints the version banner and exits 0: perl acts on `-v`
    // during switch processing, so `-Z` is never read. A decoder that kept
    // scanning would refuse a command line that works.
    for (argv, kind) in [
        (vec!["-v", "-Z"], TerminatingActionKind::Version),
        (vec!["-h", "-Z"], TerminatingActionKind::Usage),
        (vec!["--version", "-Z"], TerminatingActionKind::Version),
        (vec!["--help", "-Z"], TerminatingActionKind::Usage),
    ] {
        let invocation = perl(&argv);
        assert_eq!(
            invocation.terminating_action.map(|action| action.kind),
            Some(kind),
            "{argv:?} should terminate decoding"
        );
        assert_eq!(invocation.program, ProgramSource::NotReached);
        assert_eq!(
            invocation
                .uninterpreted_arguments
                .iter()
                .map(|argument| argument.text.as_str())
                .collect::<Vec<_>>(),
            vec!["-Z"],
            "text after a terminating switch is never interpreted"
        );
    }

    // Ordering still matters: `perl -Z -v` reports `Unrecognized switch: -Z`.
    assert!(matches!(
        perl_error(&["-Z", "-v"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: 'Z', .. }
    ));

    // A fragment written before the terminating switch is still recorded — it
    // was on the command line — but it is not a one-liner to analyze, because
    // `perl -e 'print "body"' -v` prints the version banner and never runs it.
    let fragment_then_version = perl(&["-e", "print \"body\"", "-v"]);
    assert_eq!(fragments(&fragment_then_version), vec![(SourceSwitch::E, "print \"body\"")]);
    assert_eq!(fragment_then_version.program, ProgramSource::NotReached);
    assert!(!fragment_then_version.is_one_liner(), "nothing runs, so nothing is analyzable");
}

#[test]
fn unsupported_switches_are_reported_rather_than_ignored() {
    // `-x` moves where program text starts inside a file this decoder never
    // reads. Silently dropping it would publish a source claim that is wrong.
    let invocation = perl(&["-x/tmp", "script.pl"]);
    assert_eq!(invocation.unsupported_switches.len(), 1);
    assert_eq!(invocation.unsupported_switches[0].kind, UnsupportedSwitchKind::ScriptTextOffset);
    assert_eq!(invocation.unsupported_switches[0].spelling, "x/tmp");

    let debugging = perl(&["-Dt", "-e", "print"]);
    assert_eq!(debugging.unsupported_switches[0].kind, UnsupportedSwitchKind::DebuggingFlags);
}

#[test]
fn an_unrecognized_switch_is_refused_rather_than_skipped() {
    // perl: `Unrecognized switch: -Z`.
    assert!(matches!(
        perl_error(&["-Z", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: 'Z', .. }
    ));
    // And inside a cluster, after valid letters have already been consumed.
    assert!(matches!(
        perl_error(&["-nZ", "-e", "print"]),
        InvocationDecodeError::UnrecognizedSwitch { switch: 'Z', .. }
    ));
}

// ---- missing values ----------------------------------------------------

#[test]
fn value_taking_switches_at_the_end_of_argv_are_refused() {
    // perl: `No code specified for -e.` and `No directory specified for -I.`
    assert!(matches!(
        perl_error(&["-ne"]),
        InvocationDecodeError::MissingValue { switch: 'e', .. }
    ));
    assert!(matches!(perl_error(&["-I"]), InvocationDecodeError::MissingValue { switch: 'I', .. }));
    // An empty separate value is a directory perl also refuses.
    assert!(matches!(
        perl_error(&["-I", "", "-e", "print"]),
        InvocationDecodeError::MissingValue { switch: 'I', .. }
    ));
}

// ---- lead declaration --------------------------------------------------

#[test]
fn the_lead_declaration_decides_whether_argv_zero_is_the_interpreter() {
    let with_interpreter = perl(&["-e", "print"]);
    assert_eq!(with_interpreter.interpreter, Some(ArgvSpan::new(0, 0, 4)));

    let switches_only = match decode(&["-e", "print"], ArgvLead::SwitchesOnly) {
        Ok(invocation) => invocation,
        Err(error) => panic!("expected a switches-only argv to decode, got {error}"),
    };
    assert_eq!(switches_only.interpreter, None);
    assert_eq!(fragments(&switches_only), vec![(SourceSwitch::E, "print")]);

    let empty: [&str; 0] = [];
    assert_eq!(
        decode(&empty, ArgvLead::Interpreter),
        Err(InvocationDecodeError::MissingInterpreter)
    );
    match decode(&empty, ArgvLead::SwitchesOnly) {
        Ok(invocation) => assert_eq!(invocation.program, ProgramSource::Unspecified),
        Err(error) => panic!("an empty switches-only argv is legal, got {error}"),
    }
}

// ---- provenance --------------------------------------------------------

#[test]
fn every_decoded_value_is_located_by_its_span() {
    let argv = [
        "perl",
        "-0777",
        "-Ilib",
        "-MList::Util=first",
        "-F:",
        "-ane",
        "print $F[0]",
        "--",
        "operand",
    ];
    let invocation = match decode(&argv, ArgvLead::Interpreter) {
        Ok(invocation) => invocation,
        Err(error) => panic!("expected {argv:?} to decode, got {error}"),
    };
    spans_locate_their_values(&argv, &invocation);

    // Spot-check the exact coordinates rather than only the round trip, so a
    // span that is self-consistently wrong is still caught.
    let separator = &invocation.context_facts[0];
    assert_eq!(separator.span, ArgvSpan::new(1, 2, 5), "`777` starts after `-0`");
    let include =
        find_fact(&invocation, &ContextFactKind::IncludeDirectory { directory: "lib".to_owned() });
    assert_eq!(include.span, ArgvSpan::new(2, 2, 5));
    let fragment = &invocation.source_fragments[0];
    assert_eq!(fragment.span, ArgvSpan::new(6, 0, "print $F[0]".len()));
}

#[test]
fn a_separate_value_keeps_the_switch_locatable_in_its_own_argument() {
    // Found by the accounting property below: with `-E -`, the fragment's value
    // lives in argument 2 and the `-E` that introduced it in argument 1. A model
    // carrying one span per item leaves the switch's own argument unreferenced,
    // so a diagnostic about the switch has nowhere to point.
    let invocation = perl(&["-E", "-", "--"]);
    let fragment = &invocation.source_fragments[0];
    assert_eq!(fragment.text, "-", "the value is taken verbatim");
    assert_eq!(fragment.span, ArgvSpan::new(2, 0, 1));
    assert_eq!(fragment.switch_span, ArgvSpan::new(1, 1, 2), "`E` is the second byte of `-E`");
    assert_eq!(invocation.terminator, Some(ArgvSpan::new(3, 0, 2)));

    // The same split applies to `-I`, the other switch with a separate value.
    let include = perl(&["-I", "lib", "-e", "print"]);
    let fact =
        find_fact(&include, &ContextFactKind::IncludeDirectory { directory: "lib".to_owned() });
    assert_eq!(fact.span, ArgvSpan::new(2, 0, 3));
    assert_eq!(fact.switch_span, ArgvSpan::new(1, 1, 2));
}

#[test]
fn a_valueless_switch_spans_its_own_letter() {
    let invocation = perl(&["-lane", "print"]);
    let ContextFact { span, switch_span, .. } = invocation.context_facts[1];
    assert_eq!(span, ArgvSpan::new(1, 2, 3), "`a` is the third byte of `-lane`");
    assert_eq!(switch_span, span, "a valueless switch is its own value location");
    assert_eq!(span.len(), 1);
    assert!(!span.is_empty());
}

#[test]
fn ambiguities_point_at_the_value_that_raised_them() {
    let invocation = perl(&["-Mstrict refs", "-e", "print"]);
    assert_eq!(
        invocation.ambiguities,
        vec![Ambiguity {
            kind: AmbiguityKind::ModuleExpressionIsNotAModuleName,
            span: ArgvSpan::new(1, 2, "-Mstrict refs".len()),
        }]
    );
}

// ---- property coverage -------------------------------------------------

mod properties {
    use super::{located_values, spans_locate_their_values};
    use perl_parser_core::command_line::{ArgvLead, ProgramSource, decode};
    use proptest::prelude::*;

    /// Arguments built from the pieces a real command line is made of, so the
    /// generator reaches switch clusters and values rather than only noise.
    fn argument() -> impl Strategy<Value = String> {
        prop_oneof![
            2 => "-[a-zA-Z0-9]{0,4}",
            2 => "-[a-zA-Z]{1,2}[^\\p{Cc}]{0,6}",
            // Digit-heavy tails: the octal-run limits live past four
            // characters, which the branches above cannot reach.
            2 => "-[l0][0-9]{0,8}",
            1 => "-0[xX][0-9a-fA-F]{0,6}[a-z]{0,2}",
            1 => Just("--".to_owned()),
            1 => Just("-".to_owned()),
            2 => "[^\\p{Cc}]{0,8}",
        ]
    }

    proptest! {
        /// Arbitrary argv never panics and never reports a value it cannot locate.
        #[test]
        fn decoding_is_total_and_spans_stay_inside_their_arguments(
            argv in prop::collection::vec(argument(), 0..8)
        ) {
            let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
            for lead in [ArgvLead::Interpreter, ArgvLead::SwitchesOnly] {
                if let Ok(invocation) = decode(&borrowed, lead) {
                    spans_locate_their_values(&borrowed, &invocation);
                }
            }
        }

        /// The decoder is a pure function of its inputs.
        #[test]
        fn decoding_is_deterministic(argv in prop::collection::vec(argument(), 0..8)) {
            let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
            prop_assert_eq!(
                decode(&borrowed, ArgvLead::Interpreter),
                decode(&borrowed, ArgvLead::Interpreter)
            );
        }

        /// A decode that succeeds accounts for every argument: nothing is
        /// silently dropped on the floor.
        #[test]
        fn every_argument_is_accounted_for(argv in prop::collection::vec(argument(), 1..8)) {
            let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
            if let Ok(invocation) = decode(&borrowed, ArgvLead::SwitchesOnly) {
                let mut touched = vec![false; borrowed.len()];
                let mut mark = |index: usize| {
                    if let Some(slot) = touched.get_mut(index) {
                        *slot = true;
                    }
                };
                for (_, span) in located_values(&invocation) {
                    mark(span.argument_index);
                }
                for fact in &invocation.context_facts {
                    mark(fact.span.argument_index);
                    mark(fact.switch_span.argument_index);
                }
                for switch in &invocation.neutral_switches {
                    mark(switch.span.argument_index);
                    mark(switch.switch_span.argument_index);
                }
                for fragment in &invocation.source_fragments {
                    mark(fragment.span.argument_index);
                    mark(fragment.switch_span.argument_index);
                }
                for unsupported in &invocation.unsupported_switches {
                    mark(unsupported.span.argument_index);
                }
                for uninterpreted in &invocation.uninterpreted_arguments {
                    mark(uninterpreted.span.argument_index);
                }
                if let Some(action) = invocation.terminating_action {
                    mark(action.span.argument_index);
                }
                if let Some(terminator) = invocation.terminator {
                    mark(terminator.argument_index);
                }
                match &invocation.program {
                    ProgramSource::ScriptFile { span, .. }
                    | ProgramSource::StandardInput { span } => mark(span.argument_index),
                    ProgramSource::CommandLineFragments
                    | ProgramSource::Unspecified
                    | ProgramSource::NotReached => {}
                }
                prop_assert!(
                    touched.iter().all(|seen| *seen),
                    "unaccounted arguments in {borrowed:?}: {touched:?}"
                );
            }
        }
    }
}

/// Build the expected `ModuleSpec` for a plain, un-negated import.
fn module_spec(
    module: &str,
    import_arguments: Option<&str>,
    negated: bool,
    plain: bool,
    module_span: ArgvSpan,
    import_arguments_span: Option<ArgvSpan>,
) -> perl_parser_core::command_line::ModuleSpec {
    perl_parser_core::command_line::ModuleSpec {
        negated,
        module: module.to_owned(),
        module_span,
        import_arguments: import_arguments.map(str::to_owned),
        import_arguments_span,
        module_is_plain_name: plain,
    }
}
