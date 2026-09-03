//! The `argv` decoder itself.
//!
//! The switch grammar is Perl's, not `getopt`'s: clusters bundle, several
//! switches swallow the rest of their cluster as a value, two of them fall back
//! to the next argument, and scanning stops at `--` or at the first operand.

use super::model::{
    Ambiguity, AmbiguityKind, ArgvLead, ArgvSpan, ContextFact, ContextFactKind,
    InvocationDecodeError, ModuleForm, ModuleSpec, NeutralSwitch, NeutralSwitchUse, PerlInvocation,
    ProgramArgument, ProgramSource, RecordSeparatorDigits, SourceFragment, SourceSwitch,
    TerminatingAction, TerminatingActionKind, UnsupportedSwitch, UnsupportedSwitchKind,
};

/// The Unicode option letters `-C` accepts. Any other letter makes Perl report
/// `Unknown Unicode option letter`; a value of only decimal digits is the
/// numeric form and is accepted instead.
const UNICODE_OPTION_LETTERS: &str = "aioADEILOS";

/// The largest numeric `-C` value perl defines. `perl -C511` runs and
/// `perl -C512` reports `Unknown Unicode option value 512`.
const UNICODE_OPTION_VALUE_MAX: u32 = 511;

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
        terminating_action: None,
        uninterpreted_arguments: Vec::new(),
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
            if invocation.terminating_action.is_some() {
                break;
            }
            continue;
        }
        index = decode_cluster(argv, index, &mut invocation)?;
        if invocation.terminating_action.is_some() {
            break;
        }
    }

    // Perl acted and exited inside switch processing, so everything left is
    // text it never looked at.
    if invocation.terminating_action.is_some() {
        for (offset, argument) in argv.iter().skip(index).enumerate() {
            let argument = argument.as_ref();
            invocation.uninterpreted_arguments.push(ProgramArgument {
                text: argument.to_owned(),
                span: whole_argument_span(index + offset, argument),
            });
        }
        invocation.program = ProgramSource::NotReached;
        return Ok(invocation);
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
    let kind = match argument {
        "--version" => TerminatingActionKind::Version,
        "--help" => TerminatingActionKind::Usage,
        _ => {
            return Err(InvocationDecodeError::UnrecognizedLongSwitch {
                spelling: argument.to_owned(),
                span: whole_argument_span(index, argument),
            });
        }
    };
    invocation.terminating_action =
        Some(TerminatingAction { kind, span: whole_argument_span(index, argument) });
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

    /// Consume a value whose byte length `measure` computes from the remaining
    /// cluster text. A length of zero consumes nothing.
    ///
    /// This exists because several perl switches take a value only in one
    /// shape — `-V:configvar`, `-d[t][:MOD]` — and must otherwise leave the
    /// cluster alone so bundling continues.
    fn take_prefixed_value(
        &mut self,
        measure: impl Fn(&'a str) -> usize,
    ) -> Option<(&'a str, ArgvSpan)> {
        let remaining = self.remaining();
        let taken = measure(remaining).min(remaining.len());
        let value = remaining.get(..taken)?;
        if value.is_empty() {
            return None;
        }
        let start = self.at;
        self.at += taken;
        Some((value, ArgvSpan::new(self.index, start, self.at)))
    }

    /// Consume the leading run of characters matching `accept`, at most `limit`
    /// of them.
    ///
    /// The limit is not a detail: perl reads a bounded number of octal digits
    /// and then keeps decoding the cluster, so an unbounded run would swallow
    /// switches perl still sees.
    fn take_run(&mut self, accept: impl Fn(char) -> bool, limit: usize) -> (&'a str, ArgvSpan) {
        let start = self.at;
        let remaining = self.remaining();
        let taken = remaining
            .char_indices()
            .take(limit)
            .take_while(|(_, character)| accept(*character))
            .map(|(offset, character)| offset + character.len_utf8())
            .last()
            .unwrap_or(0);
        let value = remaining.get(..taken).unwrap_or_default();
        self.at = start + taken;
        (value, ArgvSpan::new(self.index, start, self.at))
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
                let form = if letter == 'M' {
                    ModuleForm::Use
                } else {
                    ModuleForm::UseSuppressingDefaultImport
                };
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
                let (pattern, span) = cluster.rest();
                if pattern.is_empty() {
                    // Point the ambiguity at the switch: an empty value's own
                    // span is zero-width and has nothing to underline.
                    invocation.ambiguities.push(Ambiguity {
                        kind: AmbiguityKind::EmptySplitPattern,
                        span: letter_span,
                    });
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
                // perl.c reads `3 + (*s == '0')` octal digits here, so `-l0123`
                // is one four-digit value but `-l1234` is `-l123` then `-4`.
                let limit = if cluster.remaining().starts_with('0') { 4 } else { 3 };
                let (digits, digits_span) = cluster.take_run(is_octal_digit, limit);
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
            // `-C` and `-i` swallow the rest of the cluster as their value.
            'C' | 'i' => {
                let (value, value_span) = cluster.rest();
                if letter == 'C' {
                    check_unicode_options(value, value_span)?;
                }
                let switch = if letter == 'C' {
                    NeutralSwitch::UnicodeFlags
                } else {
                    NeutralSwitch::InPlaceEdit
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
            // `-V[:configvar]`: a value only in the colon form. `perl -Vfoo`
            // reports `Unrecognized switch: -oo`, so swallowing the tail would
            // hide switches perl still decodes.
            'V' => {
                let value = cluster.take_prefixed_value(|remaining| {
                    usize::from(remaining.starts_with(':')) * remaining.len()
                });
                push_neutral(invocation, NeutralSwitch::Configuration, value, letter_span);
            }
            // `-d[t][:MOD]`: the optional `t` belongs to `-d`, which is why
            // `-dt:Trace` still loads Devel::Trace. Anything after that is the
            // next switch again: `-dtx` ends with the `-x` switch.
            'd' => {
                let value = cluster.take_prefixed_value(|remaining| {
                    let flag = usize::from(remaining.starts_with('t'));
                    let rest = remaining.get(flag..).unwrap_or_default();
                    if rest.starts_with(':') { remaining.len() } else { flag }
                });
                push_neutral(invocation, NeutralSwitch::Debugger, value, letter_span);
            }
            // Perl acts on these immediately and exits, so decoding stops here.
            'v' | 'h' => {
                let kind = if letter == 'v' {
                    TerminatingActionKind::Version
                } else {
                    TerminatingActionKind::Usage
                };
                invocation.terminating_action = Some(TerminatingAction { kind, span: letter_span });
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

fn push_neutral(
    invocation: &mut PerlInvocation,
    switch: NeutralSwitch,
    value: Option<(&str, ArgvSpan)>,
    switch_span: ArgvSpan,
) {
    let (value, span) = match value {
        Some((text, span)) => (Some(text.to_owned()), span),
        None => (None, switch_span),
    };
    invocation.neutral_switches.push(NeutralSwitchUse { switch, value, span, switch_span });
}

fn push_fact(
    invocation: &mut PerlInvocation,
    kind: ContextFactKind,
    span: ArgvSpan,
    switch_span: ArgvSpan,
) {
    invocation.context_facts.push(ContextFact { kind, span, switch_span });
}

/// Reject a `-C` value perl would reject.
///
/// Perl accepts either a decimal count or a string of its Unicode option
/// letters, and refuses the mixed forms too: `perl -C7S` reports
/// `Unknown Unicode option letter 'S'`.
fn check_unicode_options(value: &str, span: ArgvSpan) -> Result<(), InvocationDecodeError> {
    // The two forms never mix: `-C7a`, `-CS7` and `-Ca7` are all rejected, each
    // naming the first character that does not belong to the form already
    // started.
    if value.starts_with(|character: char| character.is_ascii_digit()) {
        return check_unicode_option_number(value, span);
    }
    for (offset, character) in value.char_indices() {
        if !UNICODE_OPTION_LETTERS.contains(character) {
            let start = span.start + offset;
            return Err(InvocationDecodeError::UnknownUnicodeOption {
                character,
                span: ArgvSpan::new(span.argument_index, start, start + character.len_utf8()),
            });
        }
    }
    Ok(())
}

/// Check the numeric `-C` form.
///
/// Perl refuses a number it cannot read at all — `perl -C0777` and `perl -C007`
/// both report `Invalid number ... for -C option`, because a leading zero is not
/// an octal escape here but a malformed decimal — and separately refuses a
/// readable number that sets bits it does not define, as `perl -C512` reports
/// `Unknown Unicode option value 512`.
fn check_unicode_option_number(value: &str, span: ArgvSpan) -> Result<(), InvocationDecodeError> {
    let malformed =
        || InvocationDecodeError::MalformedUnicodeOptionNumber { value: value.to_owned(), span };
    if !value.chars().all(|character| character.is_ascii_digit()) {
        // A digit-led value that is not all digits is a letter error, and perl
        // names the letter rather than the number.
        for (offset, character) in value.char_indices() {
            if !character.is_ascii_digit() {
                let start = span.start + offset;
                return Err(InvocationDecodeError::UnknownUnicodeOption {
                    character,
                    span: ArgvSpan::new(span.argument_index, start, start + character.len_utf8()),
                });
            }
        }
    }
    if value.len() > 1 && value.starts_with('0') {
        return Err(malformed());
    }
    let Ok(number) = value.parse::<u32>() else {
        return Err(malformed());
    };
    if number > UNICODE_OPTION_VALUE_MAX {
        return Err(InvocationDecodeError::UnsupportedUnicodeOptionValue {
            value: value.to_owned(),
            span,
        });
    }
    Ok(())
}

fn is_octal_digit(character: char) -> bool {
    matches!(character, '0'..='7')
}

/// Read the digits after `-0`.
///
/// Two boundaries here are easy to get wrong and both are pinned against real
/// perl. The hexadecimal form is taken only when the marker is followed by hex
/// digits *all the way to the end of the cluster*: `perl -0x41n` is `-0`
/// followed by `-x` with the value `41n`, not a `0x41` separator plus `-n`. The
/// octal form reads at most three digits, so `-01234` is `-0123` then `-4`,
/// which perl rejects as an unrecognized switch.
fn take_record_separator_digits(
    cluster: &mut Cluster<'_>,
    letter_span: ArgvSpan,
) -> (Option<RecordSeparatorDigits>, ArgvSpan) {
    let remaining = cluster.remaining();
    let hexadecimal = remaining.strip_prefix('x').is_some_and(|after| {
        !after.is_empty() && after.chars().all(|character| character.is_ascii_hexdigit())
    });

    if hexadecimal {
        let _marker = cluster.next_letter();
        let (digits, span) =
            cluster.take_run(|character| character.is_ascii_hexdigit(), usize::MAX);
        return (Some(RecordSeparatorDigits::Hex(digits.to_owned())), span);
    }

    let (digits, span) = cluster.take_run(is_octal_digit, OCTAL_SEPARATOR_DIGIT_LIMIT);
    if digits.is_empty() {
        return (None, letter_span);
    }
    (Some(RecordSeparatorDigits::Octal(digits.to_owned())), span)
}

/// Octal digits perl reads after `-0`, before it resumes decoding the cluster.
const OCTAL_SEPARATOR_DIGIT_LIMIT: usize = 3;

fn neutral_switch(letter: char) -> Option<NeutralSwitch> {
    Some(match letter {
        'c' => NeutralSwitch::CompileOnly,
        'f' => NeutralSwitch::NoSiteCustomize,
        's' => NeutralSwitch::ProgramSwitches,
        'S' => NeutralSwitch::SearchPath,
        't' => NeutralSwitch::TaintWarnings,
        'T' => NeutralSwitch::TaintChecks,
        'u' => NeutralSwitch::DumpCore,
        'U' => NeutralSwitch::AllowUnsafe,
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

    // perl scans word characters and `::` here and refuses what it cannot read,
    // so a decode that accepted `-M+Foo` would publish a fact for a command line
    // that never runs.
    let name_length = match scan_module_option_name(body) {
        Ok(length) => length,
        Err(ModuleNameScanError::SingleColon) => {
            return Err(InvocationDecodeError::SingleColonInModuleName { switch, span });
        }
    };
    if name_length == 0 {
        return Err(InvocationDecodeError::EmptyModuleName { switch, span });
    }

    let scanned = body.get(..name_length).unwrap_or_default();
    let trailing = body.get(name_length..).unwrap_or_default();

    // Only a `=` immediately after the scanned name introduces import arguments.
    // Anything else is spliced into the `use` statement by `-M` and refused
    // outright by `-m`: perl reports `Can't use ... after -mname` for the
    // lowercase form while the uppercase one loads the module.
    let (module, import_arguments) = match trailing.strip_prefix('=') {
        Some(arguments) => (scanned, Some(arguments)),
        None => {
            if switch == 'm'
                && let Some(character) = trailing.chars().next()
            {
                let start = body_start + name_length;
                return Err(InvocationDecodeError::UnexpectedModuleSuffix {
                    character,
                    span: ArgvSpan::new(span.argument_index, start, start + character.len_utf8()),
                });
            }
            (body, None)
        }
    };

    let module_span = ArgvSpan::new(span.argument_index, body_start, body_start + module.len());
    let import_arguments_span = import_arguments.map(|arguments| {
        let start = body_start + module.len() + 1;
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

/// Why perl refused to read a module name at the option level.
enum ModuleNameScanError {
    /// A `:` that is not part of `::`.
    SingleColon,
}

/// Measure the module name perl reads at the option level: ASCII word
/// characters and `::`.
///
/// Byte-wise on purpose — perl scans bytes here, so a non-ASCII character ends
/// the name rather than extending it. Only ASCII bytes are stepped over, so the
/// returned length is always a character boundary.
fn scan_module_option_name(body: &str) -> Result<usize, ModuleNameScanError> {
    let bytes = body.as_bytes();
    let mut at = 0usize;
    while let Some(byte) = bytes.get(at) {
        if *byte == b':' {
            if bytes.get(at + 1) == Some(&b':') {
                at += 2;
                continue;
            }
            return Err(ModuleNameScanError::SingleColon);
        }
        if byte.is_ascii_alphanumeric() || *byte == b'_' {
            at += 1;
            continue;
        }
        break;
    }
    Ok(at)
}

/// Whether `text` is a plain `Foo::Bar` module name.
///
/// Perl compiles `use <text>;` whatever `text` is, so this classifies rather
/// than validates: a false answer means the argument is arbitrary code.
///
/// Only the *first* component carries an identifier-start rule. Perl loads
/// `Foo::1` and `Foo::1Bar` happily, and even `Foo::` (as `Foo/.pm`), while
/// `1Foo` is a syntax error and `::Foo` is refused outright. Applying the
/// identifier rule to every component would report ordinary module names as
/// arbitrary code, which is the false positive this classification cannot
/// afford: the flag exists to mark real code injection.
///
/// The apostrophe is Perl's legacy package separator, so `-MFoo'Bar` loads
/// `Foo::Bar` with a deprecation warning and is a module name too. It differs
/// from `::` in one way that matters: a trailing `::` still names a module
/// (`Foo::` loads `Foo/.pm`) while a trailing apostrophe opens a string, so
/// `-MFoo'` dies on an unterminated string rather than loading anything.
fn is_plain_module_name(text: &str) -> bool {
    const LEGACY_SEPARATOR: char = '\'';

    let Some((first, mut rest)) = split_leading_component(text) else {
        return false;
    };
    let mut characters = first.chars();
    let leads = matches!(characters.next(), Some(c) if c.is_ascii_alphabetic() || c == '_');
    if !leads || !characters.all(is_name_character) {
        return false;
    }

    loop {
        if rest.is_empty() {
            return true;
        }
        let (separator_is_legacy, after) = if let Some(after) = rest.strip_prefix("::") {
            (false, after)
        } else if let Some(after) = rest.strip_prefix(LEGACY_SEPARATOR) {
            (true, after)
        } else {
            return false;
        };
        match split_leading_component(after) {
            Some((component, remainder)) => {
                let mut characters = component.chars();
                let leads =
                    matches!(characters.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_');
                if !leads || !characters.all(is_name_character) {
                    return false;
                }
                rest = remainder;
            }
            // An empty component ends a `::` name but never a legacy one.
            None if separator_is_legacy => return false,
            None => return after.is_empty(),
        }
    }
}

/// Split off the leading run of name characters, if any.
fn split_leading_component(text: &str) -> Option<(&str, &str)> {
    let length: usize = text
        .chars()
        .take_while(|character| is_name_character(*character))
        .map(char::len_utf8)
        .sum();
    if length == 0 {
        return None;
    }
    Some((text.get(..length)?, text.get(length..)?))
}

/// A character Perl accepts inside a package-name component.
///
/// ASCII only, matching [`scan_module_option_name`]. Perl's own option scan is
/// byte-wise, and a bareword built from what it leaves behind is not a name:
/// `perl -MFooα` and `perl -MFoo٢` both die with `Unrecognized character`
/// before anything is loaded, while `perl -MFoo2` looks for `Foo2.pm`.
fn is_name_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}
