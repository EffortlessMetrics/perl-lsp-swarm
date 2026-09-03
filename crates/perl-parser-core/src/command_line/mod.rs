//! Structured decoding of a Perl interpreter invocation.
//!
//! A shell command string and a Perl invocation are different things. Collapsing
//! them loses the boundary between shell quoting, Perl option syntax, and Perl
//! source, so this module starts from **already-tokenized `argv`** and refuses to
//! reason about how the host shell produced it. Turning a command string into
//! `argv` is a separate, shell-specific concern and is deliberately not modelled
//! here.
//!
//! [`decode`] turns that `argv` into a [`PerlInvocation`]: the `-e`/`-E` source
//! fragments, the switch facts that change how the program is compiled or run,
//! the recognized switches that change neither, the `--` terminator, the program
//! operand, and the remaining program arguments. Every decoded value carries an
//! [`ArgvSpan`] naming the argument it came from and its byte range inside that
//! argument, so a later layer can map a position in composed source back to the
//! exact command coordinate it came from.
//!
//! # Layer boundary
//!
//! This is structured-argv decoding only. It reports what the command line
//! *says*, not what Perl would then *do*:
//!
//! - `-n`/`-p` are recorded as facts; the implicit read loop is not synthesized;
//! - `-l` and `-0` record their digits exactly as written; `$\` and `$/` are not
//!   computed;
//! - `-F` implies `-a` (and `-n` on Perl 5.20 and later), but only the switches
//!   actually written are recorded — implication belongs to the layer that models
//!   runtime context;
//! - `-M`/`-m` record the module expression; no module is resolved or loaded;
//! - nothing is executed, no environment is read, and no file is opened.
//!
//! # Fidelity
//!
//! The switch grammar implemented here was checked against `perl` 5.38.2. The
//! behaviors that are easy to get wrong, and that the tests pin, are:
//!
//! - switch scanning stops at `--` or at the first argument that is not a
//!   `-`-prefixed cluster; a bare `-` is an operand, not a switch;
//! - `-e`, `-E` and `-I` accept their value attached (`-Ilib`) or as the next
//!   argument (`-I lib`); `-e` takes that next argument verbatim, even when it
//!   looks like another switch;
//! - `-M`, `-m`, `-F` and `-i` accept an attached value only — `perl -M strict`
//!   fails with `Missing argument to -M.`;
//! - a value-taking switch consumes the rest of its cluster, so `-ine` is `-i`
//!   with the extension `ne`, not `-i -n -e`;
//! - `-l` and `-0` consume octal digits and then keep bundling, so `-lane` is
//!   `-l -a -n -e` and `-0777ne` is `-0777 -n -e`;
//! - those digit runs are bounded exactly as perl bounds them — three digits
//!   after `-0`, and `3 + (first == '0')` after `-l` — so `-l0123` is one value
//!   while `-l1234` is `-l123` followed by an unrecognized `-4`;
//! - `-0x` is a hexadecimal separator only when the lowercase marker is
//!   followed by hex digits all the way to the end of the cluster; `-0x41n` is
//!   `-0` and then `-x` with the value `41n`, and `-0X41` is `-0 -X` and then
//!   an unrecognized `-41`;
//! - `-V` and `-d` take an attached value only in their `:` form, so `-Vfoo` is
//!   `-V -f` and then an unrecognized `o`, and `-dx` is `-d` then `-x`;
//! - `-C` accepts a decimal count or the option letters `aioADEILOS`, never a
//!   mixture, and a value perl rejects is refused here too;
//! - `-v`, `-h`, `--version` and `--help` make perl print and exit while it is
//!   still reading switches, so decoding stops there: `perl -v -Z` succeeds.

mod decode;
mod model;

pub use decode::decode;
pub use model::{
    Ambiguity, AmbiguityKind, ArgvLead, ArgvSpan, ContextFact, ContextFactKind,
    InvocationDecodeError, ModuleForm, ModuleImportAction, ModuleSpec, NeutralSwitch,
    NeutralSwitchUse, PerlInvocation, ProgramArgument, ProgramSource, RecordSeparatorDigits,
    SourceFragment, SourceSwitch, TerminatingAction, TerminatingActionKind, UnsupportedSwitch,
    UnsupportedSwitchKind,
};
