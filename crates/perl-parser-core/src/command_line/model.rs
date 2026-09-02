//! Typed model of one decoded Perl interpreter invocation.
//!
//! Every decoded value carries an [`ArgvSpan`], so nothing in this model is a
//! bare string whose origin has been forgotten.

use thiserror::Error;

/// Where a decoded value came from: which `argv` entry, and which byte range
/// inside that entry.
///
/// Ranges are byte offsets into the UTF-8 encoding of that single argument, not
/// into any composed source text. They are half-open: `start..end`.
///
/// Where a value exists the span locates exactly that value, even when the value
/// is empty — a bare `-F` carries an empty pattern, and its span is the
/// zero-width position where the pattern would have been. A switch that carries
/// no value at all spans the switch letter instead, so every decoded item still
/// has a location. To point a diagnostic at the switch rather than its value,
/// use the item's `switch_span`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArgvSpan {
    /// Index of the argument in the `argv` slice that was decoded.
    pub argument_index: usize,
    /// Byte offset of the first byte of the value within that argument.
    pub start: usize,
    /// Byte offset one past the last byte of the value within that argument.
    pub end: usize,
}

impl ArgvSpan {
    /// Build a span over `start..end` inside `argument_index`.
    #[must_use]
    pub const fn new(argument_index: usize, start: usize, end: usize) -> Self {
        Self { argument_index, start, end }
    }

    /// Length of the span in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span covers no bytes, as an empty `-e ''` fragment does.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Whether `argv[0]` is the interpreter or already the first switch.
///
/// This is a caller declaration rather than a guess: deciding it by inspecting
/// `argv[0]` would mean guessing whether `-e` is a switch or an oddly named
/// interpreter, and that guess is exactly the ambiguity this type removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgvLead {
    /// `argv[0]` is the interpreter, such as `perl` or `/usr/bin/perl5.38`.
    /// Its text is not interpreted; only its position is recorded.
    Interpreter,
    /// `argv[0]` is already the first switch or operand.
    SwitchesOnly,
}

/// Which source switch contributed a fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceSwitch {
    /// `-e`: one line of program.
    E,
    /// `-E`: like `-e`, and additionally enables the current feature bundle.
    BigE,
}

/// One `-e` or `-E` program fragment, with the exact text Perl received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFragment {
    /// The switch that introduced this fragment.
    pub switch: SourceSwitch,
    /// The fragment text exactly as it appeared in `argv`; it may be empty.
    pub text: String,
    /// Where the fragment text came from. For a separate value this is a
    /// different argument from [`SourceFragment::switch_span`].
    pub span: ArgvSpan,
    /// Where the `-e`/`-E` letter itself is.
    pub switch_span: ArgvSpan,
}

/// `-M` versus `-m`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleForm {
    /// `-M`: `use Module;` — the module's default import list runs.
    Use,
    /// `-m`: `use Module ();` — nothing is imported.
    UseWithoutImport,
}

/// The decoded `-M`/`-m` argument.
///
/// Perl splices this text into a `use` statement rather than resolving a module
/// name, so the module component is reported exactly as written and classified
/// rather than validated away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSpec {
    /// `-M-Foo`: the leading `-` turns the `use` into a `no`.
    pub negated: bool,
    /// Text before the first `=`, with any negation marker removed.
    pub module: String,
    /// Where `module` came from.
    pub module_span: ArgvSpan,
    /// Text after the first `=`, when one is present. Perl comma-splits this
    /// into the import list; the split is left to the consumer.
    pub import_arguments: Option<String>,
    /// Where `import_arguments` came from.
    pub import_arguments_span: Option<ArgvSpan>,
    /// Whether `module` is a plain `Foo::Bar` module name. When this is false
    /// Perl still compiles `use <module>;`, so the text is arbitrary code rather
    /// than an import, and an [`AmbiguityKind::ModuleExpressionIsNotAModuleName`]
    /// is recorded.
    pub module_is_plain_name: bool,
}

/// The digits written after `-0`, exactly as given.
///
/// The separator these digits denote is not computed here: mapping them onto
/// `$/` — including the paragraph and slurp special cases — is runtime context,
/// not argv structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RecordSeparatorDigits {
    /// `-0<octal>`, such as `-0777`.
    Octal(String),
    /// `-0x<hex>` or `-0X<hex>`, such as `-0x41`.
    Hex(String),
}

/// What a source- or compile-context-affecting switch established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextFactKind {
    /// `-n`: the program runs inside an input-reading loop.
    ReadLoop,
    /// `-p`: like `-n`, and `$_` is printed at the end of each iteration.
    PrintLoop,
    /// `-a`: each input record is autosplit into `@F`.
    AutoSplit,
    /// `-F`: the autosplit pattern, exactly as written. Perl accepts an empty
    /// pattern, which is recorded as [`AmbiguityKind::EmptySplitPattern`].
    SplitPattern {
        /// The pattern text.
        pattern: String,
    },
    /// `-l`: line-ending processing, with the optional octal digits as written.
    LineEnding {
        /// Octal digits written after `-l`, if any.
        octal_digits: Option<String>,
    },
    /// `-0`: input record separator, with the digits as written. `-0` alone
    /// carries no digits.
    RecordSeparator {
        /// The digits written after `-0`, if any.
        digits: Option<RecordSeparatorDigits>,
    },
    /// `-g`: slurp the whole input as one record.
    SlurpMode,
    /// `-I`: a directory prepended to `@INC`.
    IncludeDirectory {
        /// The directory text exactly as written; it is not resolved or checked.
        directory: String,
    },
    /// `-M` or `-m`: a compile-time module import.
    ModuleImport {
        /// Whether the import came from `-M` or `-m`.
        form: ModuleForm,
        /// The decoded module expression.
        spec: ModuleSpec,
    },
}

/// One context fact and where its value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextFact {
    /// What the switch established.
    pub kind: ContextFactKind,
    /// The switch's value, or the switch letter when the switch carries no value
    /// at all. For `-I lib` this is a different argument from
    /// [`ContextFact::switch_span`].
    pub span: ArgvSpan,
    /// Where the switch letter itself is.
    pub switch_span: ArgvSpan,
}

/// A switch Perl recognizes that changes neither the program source nor the
/// compile-time context a static analysis would model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NeutralSwitch {
    /// `-C`: Unicode and I/O layer flags.
    UnicodeFlags,
    /// `-c`: compile only, do not run.
    CompileOnly,
    /// `-d`: run under the debugger.
    Debugger,
    /// `-f`: suppress `sitecustomize.pl`.
    NoSiteCustomize,
    /// `-i`: edit files in place, with an optional backup extension.
    InPlaceEdit,
    /// `-s`: strip `-switch` arguments from `@ARGV` into package variables.
    /// This moves program arguments at runtime; it does not change the source.
    ProgramSwitches,
    /// `-S`: search `PATH` for the program file.
    SearchPath,
    /// `-t`: taint warnings.
    TaintWarnings,
    /// `-T`: taint checks.
    TaintChecks,
    /// `-u`: dump core after compilation.
    DumpCore,
    /// `-U`: allow unsafe operations.
    AllowUnsafe,
    /// `-V`: print the configuration summary, or query one variable. Unlike
    /// `-v`, this does not stop switch processing: `perl -Vfoo` still reports
    /// `Unrecognized switch: -oo`.
    Configuration,
    /// `-w`: enable many warnings.
    Warnings,
    /// `-W`: enable all warnings.
    AllWarnings,
    /// `-X`: disable all warnings.
    NoWarnings,
}

/// One recognized neutral switch and its optional attached value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeutralSwitchUse {
    /// Which switch was written.
    pub switch: NeutralSwitch,
    /// The attached value, for the switches that take one (`-C`, `-i`, `-V`).
    pub value: Option<String>,
    /// The value's location, or the switch letter's when it carries no value.
    pub span: ArgvSpan,
    /// Where the switch letter itself is. For a long switch this is the whole
    /// argument.
    pub switch_span: ArgvSpan,
}

/// A switch that makes Perl print and exit *during* switch processing.
///
/// Perl acts on these immediately, so nothing after them is interpreted:
/// `perl -v -Z` prints the version banner and exits successfully even though
/// `-Z` is not a switch Perl recognizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerminatingActionKind {
    /// `-v` or `--version`: print the version banner.
    Version,
    /// `-h` or `--help`: print usage.
    Usage,
}

/// One terminating action and where it was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminatingAction {
    /// What Perl does before exiting.
    pub kind: TerminatingActionKind,
    /// The switch letter, or the whole argument for a long spelling.
    pub span: ArgvSpan,
}

/// Why a recognized switch is not modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnsupportedSwitchKind {
    /// `-x[directory]`: Perl skips leading text in the program file until a
    /// `#!`-line mentioning `perl`. That moves where the program starts inside a
    /// file this decoder never reads, so the resulting source offset cannot be
    /// reported honestly from `argv` alone.
    ScriptTextOffset,
    /// `-D[flags]`: available only from a `DEBUGGING` build, so whether the
    /// switch is even accepted depends on the interpreter rather than on `argv`.
    DebuggingFlags,
}

/// A switch that was recognized but deliberately not modelled, kept visible
/// rather than silently dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedSwitch {
    /// Why it is not modelled.
    pub kind: UnsupportedSwitchKind,
    /// The switch letter and any attached value, as written.
    pub spelling: String,
    /// Where `spelling` came from.
    pub span: ArgvSpan,
}

/// A decoded value whose meaning Perl resolves in a way a reader is likely to
/// misjudge. An ambiguity is a report, not a failure: decoding continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmbiguityKind {
    /// `-M`/`-m` whose module component is not a plain module name. Perl
    /// compiles `use <text>;` regardless, so the text is arbitrary code — a
    /// meaningful distinction, because `-M'Foo; ...'` runs at compile time.
    ModuleExpressionIsNotAModuleName,
    /// `-F` written with no pattern. Perl accepts it, but the resulting split
    /// behavior is not determined by `argv`.
    EmptySplitPattern,
}

/// One ambiguity and the value that raised it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ambiguity {
    /// What is ambiguous.
    pub kind: AmbiguityKind,
    /// Where the ambiguous value came from.
    pub span: ArgvSpan,
}

/// Where the program text comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramSource {
    /// One or more `-e`/`-E` fragments supply the program. This is the only
    /// variant that makes the invocation a one-liner.
    CommandLineFragments,
    /// An operand names a program file. Perl reads the file; this decoder does
    /// not.
    ScriptFile {
        /// The operand text exactly as written; it is not resolved or checked.
        path: String,
        /// Where the operand came from.
        span: ArgvSpan,
    },
    /// A bare `-` operand: Perl reads the program from standard input.
    StandardInput {
        /// Where the `-` operand came from.
        span: ArgvSpan,
    },
    /// No fragment and no operand. Perl still reads standard input, but nothing
    /// in `argv` said so, and that difference is worth keeping.
    Unspecified,
    /// A terminating switch ran first, so Perl never looked for a program.
    NotReached,
}

/// One argument passed through to the program in `@ARGV`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramArgument {
    /// The argument text exactly as written.
    pub text: String,
    /// Where it came from.
    pub span: ArgvSpan,
}

/// One decoded Perl interpreter invocation.
///
/// Fragments, facts, switches and arguments each appear in `argv` order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerlInvocation {
    /// The interpreter argument's location, when the caller declared
    /// [`ArgvLead::Interpreter`]. Its text is never interpreted.
    pub interpreter: Option<ArgvSpan>,
    /// The `-e`/`-E` fragments, in `argv` order. Perl joins them with newlines;
    /// joining is left to the layer that composes source.
    pub source_fragments: Vec<SourceFragment>,
    /// Switches that change the source or the compile-time context.
    pub context_facts: Vec<ContextFact>,
    /// Recognized switches that change neither.
    pub neutral_switches: Vec<NeutralSwitchUse>,
    /// Recognized switches this decoder deliberately does not model.
    pub unsupported_switches: Vec<UnsupportedSwitch>,
    /// Values whose Perl-side meaning is easy to misjudge.
    pub ambiguities: Vec<Ambiguity>,
    /// The switch that made Perl print and exit, when one was written. Decoding
    /// stops there, because Perl stops there.
    pub terminating_action: Option<TerminatingAction>,
    /// Arguments Perl never interpreted, because a terminating switch preceded
    /// them. They are neither switches nor program arguments.
    pub uninterpreted_arguments: Vec<ProgramArgument>,
    /// The `--` terminator's location, when one was written.
    pub terminator: Option<ArgvSpan>,
    /// Where the program text comes from.
    pub program: ProgramSource,
    /// The arguments Perl passes to the program in `@ARGV`.
    pub program_arguments: Vec<ProgramArgument>,
}

impl PerlInvocation {
    /// Whether the program text came from `-e`/`-E` fragments.
    ///
    /// An explicit program file and a program read from standard input are not
    /// one-liners, and reporting them as one is the mistake this method exists
    /// to make hard.
    #[must_use]
    pub fn is_one_liner(&self) -> bool {
        matches!(self.program, ProgramSource::CommandLineFragments)
    }
}

/// Why an `argv` vector could not be decoded.
///
/// Each variant corresponds to an invocation Perl itself rejects, so refusing is
/// not stricter than the interpreter.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InvocationDecodeError {
    /// [`ArgvLead::Interpreter`] was declared but `argv` was empty.
    #[error("no interpreter argument to decode")]
    MissingInterpreter,
    /// A cluster contained a letter Perl does not recognize.
    #[error("unrecognized switch `-{switch}` in argument {}", span.argument_index)]
    UnrecognizedSwitch {
        /// The unrecognized letter.
        switch: char,
        /// Where the letter is.
        span: ArgvSpan,
    },
    /// A `--`-prefixed argument other than `--version`, `--help` or the bare
    /// `--` terminator.
    #[error("unrecognized switch `{spelling}` in argument {}", span.argument_index)]
    UnrecognizedLongSwitch {
        /// The argument as written.
        spelling: String,
        /// Where it is.
        span: ArgvSpan,
    },
    /// A value-taking switch had no value: nothing was attached, and either no
    /// argument followed or the switch does not accept a separate one.
    #[error("switch `-{switch}` requires a value in argument {}", span.argument_index)]
    MissingValue {
        /// The switch letter.
        switch: char,
        /// Where the switch letter is.
        span: ArgvSpan,
    },
    /// `-M`/`-m` was given an argument whose module component is empty, such as
    /// `-M=Foo`. Perl reports `Module name required with -M option.`
    #[error("switch `-{switch}` requires a module name in argument {}", span.argument_index)]
    EmptyModuleName {
        /// The switch letter, `M` or `m`.
        switch: char,
        /// Where the argument is.
        span: ArgvSpan,
    },
    /// `-C` was given a value that is neither a decimal count nor a string of
    /// Perl's Unicode option letters. Perl reports
    /// `Unknown Unicode option letter '<c>'.`
    #[error("`-C` does not accept the option letter `{character}` in argument {}", span.argument_index)]
    UnknownUnicodeOption {
        /// The first character Perl would reject.
        character: char,
        /// Where that character is.
        span: ArgvSpan,
    },
}
