//! The `argv` decoder itself.
//!
//! The switch grammar is Perl's, not `getopt`'s: clusters bundle, several
//! switches swallow the rest of their cluster as a value, two of them fall back
//! to the next argument, and scanning stops at `--` or at the first operand.

use super::model::{
    Ambiguity, AmbiguityKind, ArgvLead, ArgvSpan, ContextFact, ContextFactKind,
    InvocationDecodeError, ModuleForm, ModuleSpec, NeutralSwitch, NeutralSwitchUse, PerlInvocation,
    ProgramArgument, ProgramSource, RecordSeparatorDigits, SourceFragment, SourceSwitch,
    UnsupportedSwitch, UnsupportedSwitchKind,
};

/// Decode an already-tokenized Perl invocation.
///
/// `lead` says whether `argv[0]` is the interpreter; it is a caller declaration
/// because inspecting `argv[0]` to decide would be a guess.
///
/// # Errors
///
/// Returns [`InvocationDecodeError`] for the invocations Perl itself rejects: an
/// unrecognized switch, a value-taking switch with no value, or `-M`/`-m` with
/// an empty module name. Switches that are recognized but not modelled, and
/// values whose meaning is easy to misjudge, are reported inside the decoded
/// invocation instead of failing it.
///
/// # Examples
///
/// ```
/// use perl_parser_core::command_line::{ArgvLead, decode};
///
/// let invocation = decode(&["perl", "-lane", "print $F[0]"], ArgvLead::Interpreter)?;
/// assert!(invocation.is_one_liner());
/// assert_eq!(invocation.source_fragments.len(), 1);
/// assert_eq!(invocation.source_fragments[0].text, "print $F[0]");
/// // `-l`, `-a` and `-n` are three separate facts from one cluster.
/// assert_eq!(invocation.context_facts.len(), 3);
/// # Ok::<(), perl_parser_core::command_line::InvocationDecodeError>(())
/// ```
pub fn decode<S: AsRef<str>>(
    argv: &[S],
    lead: ArgvLead,
) -> Result<PerlInvocation, InvocationDecodeError> {
    let mut invocation = PerlInvocation {
        interpreter: None,
        source_fragments: Vec::new(),
        context_facts: Vec::new(),
        neutral_switches: Vec::new(),
        unsupported_switches: Vec::new(),
        ambiguities: Vec::new(),
        terminator: None,
        program: ProgramSource::Unspecified,
        program_arguments: Vec::new(),
    };

    let mut index = 0usize;

    if lead == ArgvLead::Interpreter {
        let first = argv.first().ok_or(InvocationDecodeError::MissingInterpreter)?;
        invocation.interpreter = Some(whole_argument_span(0, first.as_ref()));
        index = 1;
    }

    while let Some(argument) = argv.get(index).map(AsRef::as_ref) {
        if argument == "--" {
            invocation.terminator = Some(whole_argument_span(index, argument));
            index += 1;
            break;
        }
        if !is_switch_cluster(argument) {
            break;
        }
        if argument.starts_with("--") {
            decode_long_switch(argument, index, &mut invocation)?;
            index += 1;
            break;
        }
        let neutral_before = invocation.neutral_switches.len();
        index = decode_cluster(argv, index, &mut invocation)?;
        if invocation.neutral_switches[neutral_before..]
            .iter()
            .any(|switch| matches!(switch.switch, NeutralSwitch::Usage | NeutralSwitch::Version))
        {
            break;
        }
    }

    invocation.program = if invocation.source_fragments.is_empty() {
        match argv.get(index).map(AsRef::as_ref) {
            Some(operand) => {
                let span = whole_argument_span(index, operand);
                index += 1;
                if operand == "-" {
                    ProgramSource::StandardInput { span }
                } else {
                    ProgramSource::ScriptFile { path: operand.to_owned(), span }
                }
            }
            None => ProgramSource::Unspecified,
        }
    } else {
        ProgramSource::CommandLineFragments
    };

    for (offset, argument) in argv.iter().skip(index).enumerate() {
        let argument = argument.as_ref();
        invocation.program_arguments.push(ProgramArgument {
            text: argument.to_owned(),
            span: whole_argument_span(index + offset, argument),
        });
    }

    Ok(invocation)
}

/// A `-`-prefixed argument that is not the bare `-` operand.
fn is_switch_cluster(argument: &str) -> bool {
    argument.starts_with('-') && argument != "-"
}

fn whole_argument_span(index: usize, argument: &str) -> ArgvSpan {
    ArgvSpan::new(index, 0, argument.len())
}

fn decode_long_switch(
    argument: &str,
    index: usize,
    invocation: &mut PerlInvocation,
) -> Result<(), InvocationDecodeError> {
    let switch = match argument {
        "--version" => NeutralSwitch::LongVersion,
        "--help" => NeutralSwitch::LongHelp,
        _ => {
            return Err(InvocationDecodeError::UnrecognizedLongSwitch {
                spelling: argument.to_owned(),
                span: whole_argument_span(index, argument),
            });
        }
    };
    let span = whole_argument_span(index, argument);
    invocation.neutral_switches.push(NeutralSwitchUse {
        switch,
        value: None,
        span,
        switch_span: span,
    });
    Ok(())
}

/// A reading position inside one switch cluster.
///
/// Every switch letter Perl recognizes is ASCII, so the cursor only ever stops
/// on a character boundary; non-ASCII text can still appear in a value, which
/// the cursor hands back whole.
struct Cluster<'a> {
    text: &'a str,
    index: usize,
    at: usize,
}

impl<'a> Cluster<'a> {
    /// Position the cursor just after the cluster's leading `-`.
    fn new(text: &'a str, index: usize) -> Self {
        Self { text, index, at: 1.min(text.len()) }
    }

    fn remaining(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn next_letter(&mut self) -> Option<(char, ArgvSpan)> {
        let letter = self.remaining().chars().next()?;
        let span = ArgvSpan::new(self.index, self.at, self.at + letter.len_utf8());
        self.at += letter.len_utf8();
        Some((letter, span))
    }

    /// Consume the rest of the cluster as one value.
    fn rest(&mut self) -> (&'a str, ArgvSpan) {
        let value = self.remaining();
        let span = ArgvSpan::new(self.index, self.at, self.text.len());
        self.at = self.text.len();
        (value, span)
    }

    /// Consume the leading run of characters matching `accept`.
    fn take_run(&mut self, accept: impl Fn(char) -> bool) -> (&'a str, ArgvSpan) {
        let start = self.at;
        let remaining = self.remaining();
        let taken = remaining.find(|character: char| !accept(character)).unwrap_or(remaining.len());
        let value = remaining.get(..taken).unwrap_or_default();
        self.at = start + taken;
        (value, ArgvSpan::new(self.index, start, self.at))
    }

    fn take_limited_run(
        &mut self,
        accept: impl Fn(char) -> bool,
        limit: usize,
    ) -> (&'a str, ArgvSpan) {
        let start = self.at;
        let mut end = start;
        let mut count = 0;
        for character in self.remaining().chars() {
            if count == limit || !accept(character) {
                break;
            }
            end += character.len_utf8();
            count += 1;
        }
        self.at = end;
        (self.text.get(start..end).unwrap_or_default(), ArgvSpan::new(self.index, start, end))
    }
}

/// Decode one cluster, returning the index of the next unread argument.
fn decode_cluster<S: AsRef<str>>(
    argv: &[S],
    index: usize,
    invocation: &mut PerlInvocation,
) -> Result<usize, InvocationDecodeError> {
    let argument = argv.get(index).map(AsRef::as_ref).unwrap_or_default();
    let mut cluster = Cluster::new(argument, index);
    let mut next_index = index + 1;

    while let Some((letter, letter_span)) = cluster.next_letter() {
        match letter {
            // Source switches: attached value, else the next argument verbatim.
            'e' | 'E' => {
                let switch = if letter == 'e' { SourceSwitch::E } else { SourceSwitch::BigE };
                let (attached, attached_span) = cluster.rest();
                let (text, span) = if attached.is_empty() {
                    let separate = argv.get(next_index).map(AsRef::as_ref).ok_or(
                        InvocationDecodeError::MissingValue { switch: letter, span: letter_span },
                    )?;
                    let span = whole_argument_span(next_index, separate);
                    next_index += 1;
                    (separate, span)
                } else {
                    (attached, attached_span)
                };
                invocation.source_fragments.push(SourceFragment {
                    switch,
                    text: text.to_owned(),
                    span,
                    switch_span: letter_span,
                });
                break;
            }
            // `-I` also accepts a separate value, but rejects an empty one.
            'I' => {
                let (attached, attached_span) = cluster.rest();
                let (directory, span) = if attached.is_empty() {
                    let separate = argv
                        .get(next_index)
                        .map(AsRef::as_ref)
                        .filter(|value| !value.is_empty())
                        .ok_or(InvocationDecodeError::MissingValue {
                            switch: letter,
                            span: letter_span,
                        })?;
                    let span = whole_argument_span(next_index, separate);
                    next_index += 1;
                    (separate, span)
                } else {
                    (attached, attached_span)
                };
                push_fact(
                    invocation,
                    ContextFactKind::IncludeDirectory { directory: directory.to_owned() },
                    span,
                    letter_span,
                );
                break;
            }
            // Attached-only values.
            'M' | 'm' => {
                let (attached, attached_span) = cluster.rest();
                if attached.is_empty() {
                    return Err(InvocationDecodeError::MissingValue {
                        switch: letter,
                        span: letter_span,
                    });
                }
                let form =
                    if letter == 'M' { ModuleForm::Use } else { ModuleForm::UseWithoutImport };
                let spec = decode_module_spec(attached, attached_span, letter)?;
                if !spec.module_is_plain_name {
                    invocation.ambiguities.push(Ambiguity {
                        kind: AmbiguityKind::ModuleExpressionIsNotAModuleName,
                        span: spec.module_span,
                    });
                }
                push_fact(
                    invocation,
                    ContextFactKind::ModuleImport { form, spec },
                    attached_span,
                    letter_span,
                );
                break;
            }
            'F' => {
                let (pattern, pattern_span) = cluster.rest();
                let span = if pattern.is_empty() { letter_span } else { pattern_span };
                if pattern.is_empty() {
                    invocation
                        .ambiguities
                        .push(Ambiguity { kind: AmbiguityKind::EmptySplitPattern, span });
                }
                push_fact(
                    invocation,
                    ContextFactKind::SplitPattern { pattern: pattern.to_owned() },
                    span,
                    letter_span,
                );
                break;
            }
            // Optional digit runs; bundling continues after them.
            'l' => {
                let (digits, digits_span) = cluster.take_limited_run(is_octal_digit, 3);
                let span = if digits.is_empty() { letter_span } else { digits_span };
                let octal_digits = (!digits.is_empty()).then(|| digits.to_owned());
                push_fact(
                    invocation,
                    ContextFactKind::LineEnding { octal_digits },
                    span,
                    letter_span,
                );
            }
            '0' => {
                let (digits, span) = take_record_separator_digits(&mut cluster, letter_span);
                push_fact(
                    invocation,
                    ContextFactKind::RecordSeparator { digits },
                    span,
                    letter_span,
                );
            }
            // Valueless source-affecting switches.
            'n' => push_fact(invocation, ContextFactKind::ReadLoop, letter_span, letter_span),
            'p' => push_fact(invocation, ContextFactKind::PrintLoop, letter_span, letter_span),
            'a' => push_fact(invocation, ContextFactKind::AutoSplit, letter_span, letter_span),
            'g' => push_fact(invocation, ContextFactKind::SlurpMode, letter_span, letter_span),
            // Recognized but not modelled; both swallow the rest of the cluster.
            'x' | 'D' => {
                let (value, _) = cluster.rest();
                let kind = if letter == 'x' {
                    UnsupportedSwitchKind::ScriptTextOffset
                } else {
                    UnsupportedSwitchKind::DebuggingFlags
                };
                let span = ArgvSpan::new(index, letter_span.start, letter_span.end + value.len());
                let mut spelling = String::with_capacity(letter.len_utf8() + value.len());
                spelling.push(letter);
                spelling.push_str(value);
                invocation.unsupported_switches.push(UnsupportedSwitch { kind, spelling, span });
                break;
            }
            // Neutral switches taking an optional attached value.
            'C' | 'i' | 'V' => {
                let (value, value_span) = cluster.rest();
                let switch = match letter {
                    'C' => NeutralSwitch::UnicodeFlags,
                    'i' => NeutralSwitch::InPlaceEdit,
                    _ => NeutralSwitch::Configuration,
                };
                let span = if value.is_empty() { letter_span } else { value_span };
                invocation.neutral_switches.push(NeutralSwitchUse {
                    switch,
                    value: (!value.is_empty()).then(|| value.to_owned()),
                    span,
                    switch_span: letter_span,
                });
                break;
            }
            // Neutral switches taking no value; bundling continues.
            _ => {
                let switch =
                    neutral_switch(letter).ok_or(InvocationDecodeError::UnrecognizedSwitch {
                        switch: letter,
                        span: letter_span,
                    })?;
                invocation.neutral_switches.push(NeutralSwitchUse {
                    switch,
                    value: None,
                    span: letter_span,
                    switch_span: letter_span,
                });
            }
        }
    }

    Ok(next_index)
}

fn push_fact(
    invocation: &mut PerlInvocation,
    kind: ContextFactKind,
    span: ArgvSpan,
    switch_span: ArgvSpan,
) {
    invocation.context_facts.push(ContextFact { kind, span, switch_span });
}

fn is_octal_digit(character: char) -> bool {
    matches!(character, '0'..='7')
}

/// Read the digits after `-0`.
///
/// The hexadecimal form needs both the `x` marker and at least one hex digit:
/// `perl -0x` is `-0` followed by the `-x` switch, not an empty hex separator.
fn take_record_separator_digits(
    cluster: &mut Cluster<'_>,
    letter_span: ArgvSpan,
) -> (Option<RecordSeparatorDigits>, ArgvSpan) {
    let remaining = cluster.remaining();
    let hexadecimal = remaining
        .strip_prefix('x')
        .or_else(|| remaining.strip_prefix('X'))
        .is_some_and(|after| after.starts_with(|character: char| character.is_ascii_hexdigit()));

    if hexadecimal {
        let _marker = cluster.next_letter();
        let (digits, span) = cluster.take_run(|character| character.is_ascii_hexdigit());
        return (Some(RecordSeparatorDigits::Hex(digits.to_owned())), span);
    }

    let (digits, span) = cluster.take_limited_run(is_octal_digit, 3);
    if digits.is_empty() {
        return (None, letter_span);
    }
    (Some(RecordSeparatorDigits::Octal(digits.to_owned())), span)
}

fn neutral_switch(letter: char) -> Option<NeutralSwitch> {
    Some(match letter {
        'c' => NeutralSwitch::CompileOnly,
        'd' => NeutralSwitch::Debugger,
        'f' => NeutralSwitch::NoSiteCustomize,
        'h' => NeutralSwitch::Usage,
        's' => NeutralSwitch::ProgramSwitches,
        'S' => NeutralSwitch::SearchPath,
        't' => NeutralSwitch::TaintWarnings,
        'T' => NeutralSwitch::TaintChecks,
        'u' => NeutralSwitch::DumpCore,
        'U' => NeutralSwitch::AllowUnsafe,
        'v' => NeutralSwitch::Version,
        'w' => NeutralSwitch::Warnings,
        'W' => NeutralSwitch::AllWarnings,
        'X' => NeutralSwitch::NoWarnings,
        _ => return None,
    })
}

/// Decode a `-M`/`-m` argument into negation, module expression and import
/// arguments, keeping each part's location.
fn decode_module_spec(
    text: &str,
    span: ArgvSpan,
    switch: char,
) -> Result<ModuleSpec, InvocationDecodeError> {
    let (negated, body, body_start) = match text.strip_prefix('-') {
        Some(body) => (true, body, span.start + 1),
        None => (false, text, span.start),
    };

    let (module, import_arguments) = match body.split_once('=') {
        Some((module, arguments)) => (module, Some(arguments)),
        None => (body, None),
    };

    if module.is_empty() {
        return Err(InvocationDecodeError::EmptyModuleName { switch, span });
    }

    let module_span = ArgvSpan::new(span.argument_index, body_start, body_start + module.len());
    let import_arguments_span = import_arguments.map(|arguments| {
        let start = module_span.end + 1;
        ArgvSpan::new(span.argument_index, start, start + arguments.len())
    });

    Ok(ModuleSpec {
        negated,
        module: module.to_owned(),
        module_span,
        import_arguments: import_arguments.map(str::to_owned),
        import_arguments_span,
        module_is_plain_name: is_plain_module_name(module),
    })
}

/// Whether `text` is a plain `Foo::Bar` module name.
///
/// Perl compiles `use <text>;` whatever `text` is, so this classifies rather
/// than validates: a false answer means the argument is arbitrary code.
fn is_plain_module_name(text: &str) -> bool {
    !text.is_empty()
        && text.split("::").all(|segment| {
            let mut characters = segment.chars();
            match characters.next() {
                Some(first) if first.is_alphabetic() || first == '_' => {
                    characters.all(|character| character.is_alphanumeric() || character == '_')
                }
                _ => false,
            }
        })
}
