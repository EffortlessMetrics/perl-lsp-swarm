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

/// `-M` versus `-m`. This is the spelling, not the effective import behavior:
/// see [`ModuleSpec::import_action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleForm {
    /// `-M`: `use Module;` — the module's default import list runs.
    Use,
    /// `-m`: perl appends `()`, so `-mFoo` is `use Foo ()` and imports nothing.
    ///
    /// That suppression applies to the *default* import only. `-mFoo=a,b` still
    /// compiles `use Foo split(/,/,q{a,b})`, and perl really does call
    /// `Foo->import("a","b")` — `perl -mPOSIX=floor -e 'print floor(1.5)'`
    /// prints `1` while `perl -mPOSIX -e ...` cannot find `floor`.
    UseSuppressingDefaultImport,
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
    /// Whether `module` is *decidably* a plain `Foo::Bar` module name — one
    /// this layer can classify from argv alone, which means an ASCII one.
    ///
    /// A false answer has two causes, told apart by the recorded ambiguity.
    /// [`AmbiguityKind::ModuleExpressionIsNotAModuleName`] means the argument is
    /// arbitrary code whatever Perl source context applies: `-M'strict;print
    /// 99'` runs at compile time, and so does `-M'strict;print "α"'` — a
    /// non-ASCII byte *inside* the code changes nothing.
    /// [`AmbiguityKind::ModuleNameDependsOnSourceContext`] means only that this
    /// layer cannot decide. It is **not** a claim that the argument is a module
    /// name under `utf8` — see that variant's documentation for the
    /// over-approximation it carries.
    ///
    /// Which applies is decided by reading the argument under both available
    /// readings, not by the single character that ended the name. `-MFooα`
    /// stops at non-ASCII text and *is* a plain name once `utf8` grants that
    /// text name status, so it is undecidable. `-MFooα;print 999` stops at the
    /// same character but still breaks at the `;` under that reading, so it is
    /// arbitrary code either way — verified: it prints `999` under `-Mutf8` and
    /// dies without it. Stopping at the break character alone would report that
    /// injection as merely undecidable.
    pub module_is_plain_name: bool,
}

/// What perl calls on the module for one `-M`/`-m` spec.
///
/// A boolean cannot answer this. `-M-Foo` compiles `no Foo;`, which calls
/// `unimport` rather than nothing, and a consumer modelling compile-time effects
/// needs to know which of the two runs — while an argument that is not a module
/// name at all may call neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleImportAction {
    /// `Module->import(...)` runs.
    Import,
    /// `Module->unimport(...)` runs — the negated `-M-Foo` form.
    Unimport,
    /// Neither runs: perl appends `()`, as in `-mFoo` (`use Foo ()`) and
    /// `-m-Foo` (`no Foo ()`).
    NoCall,
    /// The argument is not a plain module name, so no method call can be
    /// claimed from argv structure alone. See
    /// [`ModuleSpec::module_is_plain_name`].
    Undetermined,
}

impl ModuleSpec {
    /// What perl calls on the module for this spec under `form`.
    ///
    /// Answered only for a plain module name. Perl splices the argument into a
    /// `use`/`no` statement, and two accepted shapes reach a compiler path with
    /// no method dispatch in it at all:
    ///
    /// - a **version declaration**. `-M5.010` is `use 5.010;` and `-M-5.010` is
    ///   `no 5.010;`. Neither loads a file nor calls a method: `perl -M5.010`
    ///   attempts no `@INC` lookup (compare `perl -MNoSuchModule`, which reports
    ///   `Can't locate NoSuchModule.pm`), enables the feature bundle so that
    ///   `say` compiles, and leaves `strict` off; `perl -M-5.010` reports
    ///   `Perls since v5.10.0 too modern`, a version ceiling rather than an
    ///   `unimport`. `-Mv5.10` behaves the same way. The `-m` spelling never
    ///   reaches this shape — `perl -m5.010` dies with `Can't use '.' after
    ///   -mname`;
    /// - **arbitrary code**. `-M'strict;print 99'` compiles two statements, and
    ///   whether the first one imports is a question about Perl source, not
    ///   about argv.
    ///
    /// Both are already marked by [`ModuleSpec::module_is_plain_name`], so that
    /// one flag draws the line here too and this layer invents no
    /// version-literal grammar of its own.
    ///
    /// For a plain name a call happens unless the `-m` spelling carries no
    /// arguments, and the call is `unimport` exactly when the spec is negated.
    /// Checked against the interpreter with `strict`, whose import and unimport
    /// are both observable:
    ///
    /// | invocation | `$x = 1` under `use strict` | action |
    /// | --- | --- | --- |
    /// | `-Mstrict` | refused | `Import` |
    /// | `-M-strict` | allowed | `Unimport` |
    /// | `-M-strict=vars` | allowed | `Unimport` |
    /// | `-mstrict` | allowed | `NoCall` |
    /// | `-m-strict` | refused | `NoCall` |
    /// | `-m-strict=vars` | allowed | `Unimport` |
    #[must_use]
    pub fn import_action(&self, form: ModuleForm) -> ModuleImportAction {
        if !self.module_is_plain_name {
            return ModuleImportAction::Undetermined;
        }
        let calls = match form {
            ModuleForm::Use => true,
            ModuleForm::UseSuppressingDefaultImport => self.import_arguments.is_some(),
        };
        if !calls {
            ModuleImportAction::NoCall
        } else if self.negated {
            ModuleImportAction::Unimport
        } else {
            ModuleImportAction::Import
        }
    }
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
    /// `-0x<hex>`, such as `-0x41`. Only lowercase `x` marks this form:
    /// `perl -0X41` reports `Unrecognized switch: -41`.
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
    /// `-M`/`-m` whose module *name* broke at non-ASCII text, so whether it
    /// names a module cannot be decided from argv.
    ///
    /// Non-ASCII text elsewhere in the argument does not qualify: an expression
    /// that already stopped being a name at an ASCII character is arbitrary
    /// code, and records
    /// [`AmbiguityKind::ModuleExpressionIsNotAModuleName`] instead.
    ///
    /// `-M` splices what Perl's byte-wise option scan leaves behind into a
    /// `use` statement, which the lexer then reads under whatever pragmas the
    /// earlier arguments already put in force:
    ///
    /// ```text
    /// perl -MFooα                    -e1 → Unrecognized character \xCE
    /// perl -Mutf8 -MFooα             -e1 → Can't locate Fooα.pm in @INC
    /// perl -Mstrict;use utf8 -MFooα  -e1 → Can't locate Fooα.pm in @INC
    /// perl -Mutf8 -Mstrict;no utf8 -MFooα -e1 → Unrecognized character \xCE
    /// ```
    ///
    /// Deciding between them means knowing whether `utf8` is in force, and the
    /// third and fourth lines show that an *arbitrary expression* can turn it on
    /// or off — so answering requires parsing the Perl source this decoder
    /// deliberately does not read. Settling it also needs Perl's Unicode
    /// identifier rules, which admit combining marks and connector punctuation
    /// (`perl -Mutf8 -MFoo::á` and `-MFoo::a‿` both load). This layer therefore
    /// reports the question instead of guessing an answer; the module text and
    /// its span are still recorded exactly.
    ///
    /// # This is "cannot decide", not "a name under `utf8`"
    ///
    /// The scan that decides this treats *every* non-ASCII character as a name
    /// character, which is deliberately wider than Perl's real identifier rule
    /// — being a superset is what makes the decision possible without Unicode
    /// property tables. A rejection is therefore conclusive, but acceptance
    /// means only "possible":
    ///
    /// ```text
    /// perl -Mutf8 -MFoo€ -e1 → Unrecognized character \x{20ac}
    /// perl -Mutf8 -MFoo☃ -e1 → Unrecognized character \x{2603}
    /// perl -Mutf8 -MFoo± -e1 → Unrecognized character \x{b1}
    /// ```
    ///
    /// Those are not module names under any pragma, yet they are reported here
    /// rather than as
    /// [`AmbiguityKind::ModuleExpressionIsNotAModuleName`], because separating
    /// them needs exactly the identifier tables this layer declines to carry.
    /// So a consumer must treat this variant as *unknown* and never as evidence
    /// that the text names a module. Narrowing it is tracked with the rest of
    /// the version- and context-dependent acceptance questions.
    ModuleNameDependsOnSourceContext,
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
    /// Text Perl never interpreted, because a terminating switch preceded it.
    /// It is neither switches nor program arguments.
    ///
    /// Usually whole later arguments, but the first entry can be the unread
    /// tail of the terminating switch's own cluster: perl stops reading at the
    /// switch letter, so `perl -hZ` exits 0 like `perl -h` and the `Z` is never
    /// a switch. Its span then covers only that tail, not the whole argument.
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
    /// `-M`/`-m` was given a module name containing a lone `:`. Perl reports
    /// `Invalid module name <name> with -M option: contains single ':'`.
    #[error("switch `-{switch}` module name has a single `:` in argument {}", span.argument_index)]
    SingleColonInModuleName {
        /// The switch letter, `M` or `m`.
        switch: char,
        /// Where the argument is.
        span: ArgvSpan,
    },
    /// `-m` was followed by something other than `=` after the module name.
    /// Perl reports `Can't use '<c>' after -mname.` — `-M` splices the same
    /// text into the `use` statement instead.
    #[error("`-m` cannot be followed by `{character}` in argument {}", span.argument_index)]
    UnexpectedModuleSuffix {
        /// The first character perl would reject.
        character: char,
        /// Where that character is.
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
    /// `-C` was given a number Perl will not read, such as one with a leading
    /// zero or one too large to fit. Perl reports
    /// `Invalid number '<value>' for -C option.`
    #[error("`-C` cannot read the number `{value}` in argument {}", span.argument_index)]
    MalformedUnicodeOptionNumber {
        /// The digits as written.
        value: String,
        /// Where they are.
        span: ArgvSpan,
    },
    /// `-C` was given a number that sets bits Perl does not define. Perl reports
    /// `Unknown Unicode option value <n>.`
    #[error("`-C` does not define the value `{value}` in argument {}", span.argument_index)]
    UnsupportedUnicodeOptionValue {
        /// The digits as written.
        value: String,
        /// Where they are.
        span: ArgvSpan,
    },
}
