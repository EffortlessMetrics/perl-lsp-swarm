//! The bounded loaded-module reload transaction executor (reload train R02,
//! #10098).
//!
//! R01 (#10097) froze *what* a reload transaction means; this module
//! executes one. It owns exactly three things and nothing else:
//!
//! 1. the **channel seam** ([`ReloadRuntimeChannel`]) through which a
//!    transaction reaches a live debuggee, split so that the call which
//!    crosses the possibly-applied boundary is a *different method* from
//!    the read-only ones;
//! 2. the **command builder** ([`plan_commands`]), which derives every
//!    debugger command from the bound subject under a strict allowlist so
//!    no raw path, debugger command, or Perl expression can reach the
//!    runtime;
//! 3. the **state machine** ([`execute_reload`]), which walks the frozen
//!    phases and classifies exactly one terminal
//!    [`LoadedModuleReloadOutcome`].
//!
//! # The one law this module exists to enforce
//!
//! `query_inc_entries` (`debug_adapter/output.rs`) maps a framed-query
//! timeout to an empty list. That is correct for a read-only query and
//! catastrophic for a mutation: the transport cannot distinguish "the
//! command never ran" from "the command ran and the answer was lost".
//!
//! So the seam refuses to let a caller make that mistake:
//! [`ChannelSettlement::NotIssued`] means the bytes never reached the
//! debuggee, and [`ChannelSettlement::Unsettled`] means they may have. The
//! executor turns the second into
//! [`LoadedModuleReloadOutcome::IndeterminatePossiblyApplied`] with a
//! generation advance, *always*. The invariant
//! [`ReloadExecution::mutation_issued`] ⟺ generation advanced is asserted
//! exhaustively in this module's tests.
//!
//! # Reachability
//!
//! Nothing here is routed from a DAP request. The capability projection
//! stays [`super::ReloadCapabilityProjection::Unadvertised`] until R04
//! (#10104) proves the transaction through the public binary, and the
//! adapter-side wiring belongs to R03 (#10102). This module is the
//! executor those leaves consume; it advertises nothing on its own.
//!
//! # What this module does not claim
//!
//! Executing a reload does **not** migrate existing blessed instances,
//! closures, captured lexicals, or already-resolved methods, and does not
//! remove symbols the old source defined. Those limits are the frozen
//! [`super::PERL_RUNTIME_LIMITATIONS`]; a `Reloaded` outcome here means
//! exactly "the module source was re-executed under bounded semantics and
//! the runtime read back the refreshed registration" — never more.

use super::eligibility::LoadedModuleReloadEligibility;
use super::generation::{GenerationAdvance, RuntimeModuleGenerationClock};
use super::mechanism::ReloadMechanism;
use super::subject::{LoadedModuleSubject, SubjectCurrentnessView};
use super::transaction::{
    IndeterminateCause, LoadedModuleReloadOutcome, LoadedModuleReloadPlan, PreMutationFailureCause,
    ReloadTransactionPhase,
};

/// Marker stem for the read-only preflight observation.
const PREFLIGHT_MARKER: &str = "PERLLSP_RELOAD_PREFLIGHT";
/// Marker stem for the mutation acknowledgement.
const MUTATION_MARKER: &str = "PERLLSP_RELOAD_MUTATION";
/// Marker stem for the post-mutation read-back observation.
const READBACK_MARKER: &str = "PERLLSP_RELOAD_READBACK";

/// Bind a marker stem to one transaction's operation identity.
///
/// A fixed marker is forgeable by the debuggee's own output: a module
/// whose body or `BEGIN` block prints `PERLLSP_RELOAD_MUTATION 0` lands
/// that line in the same frame as the real acknowledgement, and a
/// first-match parser reads the module's text as the transaction's
/// answer. Binding the marker to the operation identity means output
/// written before this transaction existed cannot name it, and parsing
/// takes the *last* match so a same-frame echo cannot pre-empt the real
/// terminal value either.
fn marker_for(stem: &str, operation_identity: u64) -> String {
    format!("{stem}_{operation_identity}")
}

/// Encode a string as Perl-safe hex for `pack("H*", ...)`.
///
/// The alternative — quoting the value into `q(...)` — forces a choice
/// between rejecting legitimate paths and risking injection. Windows
/// separators (`C:\ws\lib\App\Core.pm`) and parenthesised directories
/// (`/opt/Perl (local)/lib/App/Core.pm`) are ordinary paths, but a
/// backslash can escape a `q()` delimiter and a paren can close it early.
///
/// Hex sidesteps the dilemma: the command text contains only `[0-9a-f]`,
/// so no input can terminate the quote or introduce a statement, and
/// every byte sequence a filesystem can produce round-trips exactly.
fn perl_hex(value: &str) -> String {
    value.as_bytes().iter().fold(String::with_capacity(value.len() * 2), |mut out, byte| {
        use std::fmt::Write as _;
        // Writing to a String cannot fail; the result is discarded rather
        // than unwrapped so no panicking accessor appears here.
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// Why a framed exchange did not produce an answer.
///
/// Every variant means the same thing to a *mutation*: the runtime may
/// have been changed. The distinction exists for diagnosis and for the
/// terminal cause code, never to let one of them be treated as clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnsettledKind {
    /// The framed answer did not arrive before the deadline.
    Timeout,
    /// The transport closed before the framed answer arrived.
    TransportLoss,
    /// The operation was cancelled while in flight.
    Cancelled,
}

impl UnsettledKind {
    /// All unsettled kinds in closed order.
    pub const ALL: [UnsettledKind; 3] =
        [UnsettledKind::Timeout, UnsettledKind::TransportLoss, UnsettledKind::Cancelled];

    /// Stable diagnostic code.
    pub const fn as_str(self) -> &'static str {
        match self {
            UnsettledKind::Timeout => "timeout",
            UnsettledKind::TransportLoss => "transport_loss",
            UnsettledKind::Cancelled => "cancelled",
        }
    }

    /// The post-boundary indeterminate cause this kind maps to.
    ///
    /// A cancellation after the boundary cannot prove non-application, so
    /// it is an ambiguous acknowledgement rather than a clean cancel.
    pub const fn post_boundary_cause(self) -> IndeterminateCause {
        match self {
            UnsettledKind::Timeout => IndeterminateCause::TimeoutAfterMutationBegan,
            UnsettledKind::TransportLoss => IndeterminateCause::TransportLossAfterMutationBegan,
            UnsettledKind::Cancelled => IndeterminateCause::AmbiguousAcknowledgement,
        }
    }
}

/// The result of one framed exchange with the debuggee.
///
/// The split between [`ChannelSettlement::NotIssued`] and
/// [`ChannelSettlement::Unsettled`] is the whole safety property of this
/// seam: an implementation that cannot tell the two apart must report
/// `Unsettled`, which is the conservative answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelSettlement {
    /// The commands ran and these framed output lines came back.
    Acknowledged(Vec<String>),
    /// The commands were never written to the debuggee: no session, no
    /// stdin, or a write error before any byte reached it. Nothing ran.
    NotIssued(String),
    /// The commands may have run, but no framed answer settled.
    Unsettled(UnsettledKind),
}

/// The transport a reload transaction drives.
///
/// Implementations own framing, deadlines, and cancellation. They must not
/// collapse a lost answer into an empty success: returning
/// `Acknowledged(vec![])` for a timed-out mutation is exactly the bug this
/// seam exists to make unrepresentable.
pub trait ReloadRuntimeChannel {
    /// The live currentness view for the revalidation immediately before
    /// mutation. `None` when the debuggee is not stopped and command-ready.
    fn currentness_view(&mut self) -> Option<SubjectCurrentnessView>;

    /// Run read-only observation commands.
    fn run_readonly(&mut self, commands: &[String]) -> ChannelSettlement;

    /// Run the one mutation command set.
    ///
    /// Returning anything other than [`ChannelSettlement::NotIssued`]
    /// asserts that the bytes reached the debuggee, which crosses the
    /// possibly-applied boundary irrevocably.
    fn run_mutation(&mut self, commands: &[String]) -> ChannelSettlement;
}

/// Why a subject cannot be turned into executable debugger commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandPlanError {
    /// The runtime `%INC` key is not a plain relative Perl module key.
    ///
    /// The key is the only subject field interpolated into debugger
    /// command text, so it carries a strict allowlist. Anything else —
    /// quotes, parentheses, semicolons, whitespace, newlines, `..`
    /// traversal, an absolute path, or a non-`.pm` suffix — is refused
    /// before any command is built.
    UnsafeModuleKey,
    /// The resolved runtime path cannot be quoted into command text.
    ///
    /// The mutation binds `require` to the admitted absolute path, so the
    /// path is interpolated too and carries its own allowlist. Paths may
    /// legitimately contain spaces, so the guard bans only what can break
    /// `q(...)` quoting or the surrounding statement.
    UnsafeRuntimePath,
    /// The mechanism has no executable implementation in this cohort.
    MechanismNotExecutable,
}

impl CommandPlanError {
    /// All command-plan errors in closed order.
    pub const ALL: [CommandPlanError; 3] = [
        CommandPlanError::UnsafeModuleKey,
        CommandPlanError::UnsafeRuntimePath,
        CommandPlanError::MechanismNotExecutable,
    ];

    /// Stable diagnostic code.
    pub const fn as_str(self) -> &'static str {
        match self {
            CommandPlanError::UnsafeModuleKey => "unsafe_module_key",
            CommandPlanError::UnsafeRuntimePath => "unsafe_runtime_path",
            CommandPlanError::MechanismNotExecutable => "mechanism_not_executable",
        }
    }

    /// The refusal disposition this error projects to.
    ///
    /// An unusable key is an inexact identity; an unimplemented mechanism
    /// is an unsupported runtime family. Neither ever admits the subject.
    ///
    /// # Note for client-facing copy
    ///
    /// Both mappings reuse an existing frozen disposition rather than
    /// widening the closed vocabulary, which is the required direction —
    /// but each carries a second meaning the R01 contract's own wording
    /// does not spell out, and anything rendering these codes to a user
    /// needs to know that:
    ///
    /// - `source_not_exact_or_stale` is documented as incomplete binding,
    ///   a stale generation, or a digest mismatch. Here it *also* means a
    ///   malformed or hostile `%INC` key — "this identity is unusable",
    ///   not "your source changed since you saved".
    /// - `unsupported_runtime` is documented as the runtime not supporting
    ///   the mechanism family. Here it *also* means the mechanism is not
    ///   implemented in this build — a fact about us, not the runtime.
    ///
    /// Neither can be split without reopening #10097, so the distinction
    /// lives here rather than in a fourteenth disposition.
    pub const fn refusal(self) -> LoadedModuleReloadEligibility {
        match self {
            CommandPlanError::UnsafeModuleKey | CommandPlanError::UnsafeRuntimePath => {
                LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
            CommandPlanError::MechanismNotExecutable => {
                LoadedModuleReloadEligibility::UnsupportedRuntime
            }
        }
    }
}

/// Whether a mechanism has an executable implementation in this cohort.
///
/// Only [`ReloadMechanism::IncDeletionAndRequire`] does. The `do`/require
/// helper needs package handling this cohort has not earned, the
/// workspace helper needs its own injection authority and lifecycle, and
/// Class::Refresh is a measured compatibility subject that never becomes
/// product authority by being installed. Each of those refuses rather
/// than silently degrading to the `%INC` path.
pub const fn mechanism_is_executable(mechanism: ReloadMechanism) -> bool {
    matches!(mechanism, ReloadMechanism::IncDeletionAndRequire)
}

/// Whether a runtime `%INC` key is safe to interpolate into command text.
///
/// Accepts only `Segment(/Segment)*.pm` where a segment is one or more of
/// `[A-Za-z0-9_-]` plus internal `.`. Rejects absolute paths, empty
/// segments, `..` traversal, and every quoting or statement-separating
/// character.
fn module_key_is_safe(inc_key: &str) -> bool {
    if inc_key.is_empty() || inc_key.len() > 255 || !inc_key.ends_with(".pm") {
        return false;
    }
    let mut segments = 0_usize;
    for segment in inc_key.split('/') {
        segments += 1;
        if segment.is_empty() || segment == ".." || segment == "." {
            return false;
        }
        if !segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
            return false;
        }
    }
    segments > 0
}

/// Whether a resolved runtime path can be carried into command text.
///
/// Because the path is hex-encoded rather than quoted, this is a
/// plausibility check on the *identity*, not an injection guard — the
/// injection question is answered by [`perl_hex`], which cannot emit a
/// character with syntactic meaning.
///
/// That distinction matters. An allowlist tight enough to make `q(...)`
/// quoting safe has to reject backslashes and parentheses, which would
/// make `C:\ws\lib\App\Core.pm` and `/opt/Perl (local)/lib/App/Core.pm`
/// permanently unreloadable. Those are ordinary paths, not attacks, and a
/// guard that refuses them is a bug rather than caution.
///
/// So this rejects only what cannot be a usable path at all: empty,
/// absurdly long, containing a NUL — which no filesystem path may contain
/// and which would truncate the decoded value inside Perl — or **not
/// absolute**.
///
/// The absoluteness requirement is load-bearing, not tidiness. `require`
/// only skips the `@INC` search for an absolute path; a relative one is
/// searched exactly like a bare module key:
///
/// ```text
/// $ perl -e 'use lib "b"; require "a/App/Core.pm"'
/// Can't locate a/App/Core.pm in @INC (@INC entries checked: b ...)
/// ```
///
/// So a relative bound path would silently undo the whole reason the
/// mutation names a path instead of the key, and could execute a
/// same-named module from somewhere else on `@INC`. `LoadedModuleSubject`
/// documents this field as the runtime-resolved *absolute* path; this
/// enforces it rather than trusting it.
fn runtime_path_is_safe(path: &str) -> bool {
    !path.trim().is_empty()
        && path.len() <= 4096
        && !path.contains('\u{0}')
        && path_is_absolute(path)
}

/// Whether a debuggee-side path is absolute, for POSIX or Windows.
///
/// This deliberately does not use `std::path`: the path describes the
/// debuggee's filesystem, which need not match the host the adapter is
/// compiled for, so host-conditional semantics would be wrong.
fn path_is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    // POSIX root, or a Windows UNC / rooted path.
    if bytes.first() == Some(&b'/') || path.starts_with("\\\\") {
        return true;
    }
    // Windows drive-letter root: `C:\...` or `C:/...`.
    matches!(
        (bytes.first(), bytes.get(1), bytes.get(2)),
        (Some(drive), Some(b':'), Some(b'\\' | b'/')) if drive.is_ascii_alphabetic()
    )
}

/// The three command sets one transaction issues, derived from the subject.
///
/// The fields are private and read-only by accessor. `plan_commands` owns
/// command derivation — that ownership is what keeps the `%INC`-key
/// allowlist meaningful — so a constructed plan must not be mutable
/// afterwards. Public `Vec<String>` fields would let a consumer swap in
/// arbitrary debugger text after validation and defeat the guard entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadCommandPlan {
    preflight: Vec<String>,
    mutation: Vec<String>,
    read_back: Vec<String>,
}

impl ReloadCommandPlan {
    /// Read-only preflight: is the subject still registered, and where?
    pub fn preflight(&self) -> &[String] {
        &self.preflight
    }

    /// The single mutation command set. Issuing it crosses the boundary.
    pub fn mutation(&self) -> &[String] {
        &self.mutation
    }

    /// Read-only post-mutation read-back of the registration.
    pub fn read_back(&self) -> &[String] {
        &self.read_back
    }
}

/// Build the command plan for one bound subject and mechanism.
///
/// Every command is derived from the bound subject; nothing is taken from
/// a caller-supplied string. The `%INC` key is the only interpolated
/// field and passes [`module_key_is_safe`] first.
///
/// The preflight deliberately does **not** compile the replacement source.
/// Compiling Perl runs `BEGIN` blocks, so an in-debuggee "syntax check"
/// would itself be a runtime mutation and would blur the possibly-applied
/// boundary this contract exists to keep sharp. Preflight therefore
/// establishes only what is provably side-effect-free: that the subject is
/// still registered in `%INC` at the resolved path the subject was bound
/// to. Compile-preflight, if it is ever wanted, needs its own out-of-band
/// authority and is not part of this cohort.
pub fn plan_commands(
    subject: &LoadedModuleSubject,
    mechanism: ReloadMechanism,
) -> Result<ReloadCommandPlan, CommandPlanError> {
    if !mechanism_is_executable(mechanism) {
        return Err(CommandPlanError::MechanismNotExecutable);
    }
    let key = subject.inc_key();
    if !module_key_is_safe(key) {
        return Err(CommandPlanError::UnsafeModuleKey);
    }
    let path = subject.resolved_runtime_path();
    if !runtime_path_is_safe(path) {
        return Err(CommandPlanError::UnsafeRuntimePath);
    }
    // Both values reach Perl as hex, decoded inside the debuggee, so the
    // command text carries no character that could close a quote or start
    // a statement — and every legitimate platform path survives verbatim.
    let key_hex = perl_hex(key);
    let path_hex = perl_hex(path);
    let operation = subject.operation_identity();
    let observe = |stem: &str| {
        let marker = marker_for(stem, operation);
        format!(
            "p do {{ my $k = pack(q(H*),q({key_hex})); \
             \"{marker} \" . (exists $INC{{$k}} ? q(present) : q(absent)) \
             . \" \" . (defined $INC{{$k}} ? unpack(q(H*), $INC{{$k}}) : q(-)) }}"
        )
    };
    let mutation_marker = marker_for(MUTATION_MARKER, operation);
    Ok(ReloadCommandPlan {
        preflight: vec![observe(PREFLIGHT_MARKER)],
        // `require` the *admitted absolute path*, never the `%INC` key.
        //
        // `require $key` searches the debuggee's current `@INC`. If an
        // include root was added or reordered since the module was loaded,
        // that search can resolve the same key to a different file —
        // possibly outside the launch authority admission checked — and the
        // wrong code would already have executed by the time the read-back
        // reports a path mismatch. Perl treats a path-looking argument as a
        // literal filename and skips the `@INC` search entirely, so this
        // binds the mutation to the exact file preflight approved.
        //
        // `require ABS` registers `$INC{ABS}`, not `$INC{KEY}`, so the
        // bookkeeping is restored afterwards: the stray absolute entry is
        // dropped and the canonical key is repointed at the same path the
        // subject is bound to.
        // The absolute-path `%INC` entry is scratch space for this
        // transaction, but it may also have been a real registration
        // already — a module loaded once by key and once by absolute path
        // has both. Deleting it and not putting it back would silently
        // cost that alias its idempotence: the next `require` of the
        // absolute path would re-execute the module instead of returning
        // immediately. So on success the prior value is restored, and only
        // an entry this transaction invented is removed.
        //
        // The failure branch is not symmetric, and both halves of it are
        // deliberate.
        //
        // Perl marks a *failed* `require` by leaving `$INC{$p}` present
        // but undefined, and every later `require` of that exact path then
        // dies with `Attempt to reload ... aborted` — permanently, for the
        // life of the process. Verified on 5.38.2. So the entry must be
        // removed after a failure, or this transaction would leave a
        // latent fatal in unrelated application code that merely happens
        // to require the same absolute path.
        //
        // It is removed rather than restored to `$prev`. A failed
        // `require` leaves the module *partially executed* — `BEGIN`
        // blocks may have run and subs may have been redefined before it
        // died — so restoring either registration would advertise that
        // half-built state as the intact previous module, and the next
        // `require` would return immediately on a package that never
        // finished loading. Deleting says "not cleanly loaded", which is
        // the truth, and leaves recovery possible.
        mutation: vec![format!(
            "p do {{ my $k = pack(q(H*),q({key_hex})); my $p = pack(q(H*),q({path_hex})); \
             my $had = exists $INC{{$p}}; my $prev = $INC{{$p}}; \
             delete $INC{{$k}}; delete $INC{{$p}}; \
             my $ok = eval {{ require $p; 1 }} ? 1 : 0; \
             if ($ok) {{ if ($had) {{ $INC{{$p}} = $prev; }} else {{ delete $INC{{$p}}; }} \
             $INC{{$k}} = $p; }} else {{ delete $INC{{$p}}; }} \
             \"{mutation_marker} $ok\" }}"
        )],
        read_back: vec![observe(READBACK_MARKER)],
    })
}

/// Decode a Perl `unpack("H*", ...)` payload back to a string.
///
/// Returns `None` for anything that is not an even-length run of hex
/// digits, or whose bytes are not valid UTF-8 — an unreadable observation
/// is never silently treated as a path that happens not to match.
fn decode_perl_hex(hex: &str) -> Option<String> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let raw = hex.as_bytes();
    for pair in raw.chunks_exact(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        // Both digits are < 16, so the product fits a u8 without any
        // fallible conversion.
        bytes.push((hi * 16 + lo) as u8);
    }
    String::from_utf8(bytes).ok()
}

/// A parsed `%INC` registration observation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistrationObservation {
    present: bool,
    path: String,
}

/// Strip ANSI CSI sequences and stray C0 controls from one framed line.
///
/// perl5db decorates its prompt (`  DB<2> ` wrapped in underline/bold
/// escapes) and writes to stderr when stdin is not a terminal, so a real
/// framed line arrives with control bytes around the payload. Marker
/// fields are compared verbatim against the bound subject, so the
/// decoration is removed before parsing rather than tolerated afterwards.
fn sanitize_frame_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // CSI: ESC '[' parameters, terminated by a byte in @..~
            if chars.peek() == Some(&'[') {
                chars.next();
                for inner in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&inner) {
                        break;
                    }
                }
            }
            continue;
        }
        if c == '\t' || !c.is_control() {
            out.push(c);
        }
    }
    out
}

/// Parse the registration marker out of framed output lines.
///
/// Returns `None` when the marker is absent: an answer that does not carry
/// the marker is not an observation, and is never read as "absent".
///
/// The path is everything after the state word to end of line, trimmed —
/// **not** the next whitespace-delimited token. A module resolved under a
/// directory containing a space (`/ws/my lib/App/Core.pm`) would otherwise
/// parse as `/ws/my`, mismatch the bound subject, and refuse a perfectly
/// current subject as `AmbiguousRuntimeMapping`.
fn parse_registration(lines: &[String], marker: &str) -> Option<RegistrationObservation> {
    let mut found = None;
    for line in lines {
        let line = sanitize_frame_line(line);
        // Match the marker plus its separator, so one marker cannot be
        // read inside a longer one that merely starts with it (operation
        // 9's `..._9` is a prefix of operation 91's `..._91`).
        let needle = format!("{marker} ");
        let Some(index) = line.rfind(&needle) else {
            continue;
        };
        let rest = line.get(index + needle.len()..).unwrap_or("").trim();
        let (state, remainder) = match rest.split_once(char::is_whitespace) {
            Some((state, remainder)) => (state, remainder.trim()),
            None => (rest, ""),
        };
        let present = match state {
            "present" => true,
            "absent" => false,
            _ => continue,
        };
        // The path arrives hex-encoded so it round-trips exactly: a path
        // with leading or trailing whitespace would otherwise be trimmed
        // by this parser and never match the bound subject, refusing every
        // reload of it. `-` is the sentinel for "no value".
        let path = if remainder == "-" {
            String::new()
        } else {
            match decode_perl_hex(remainder) {
                Some(decoded) => decoded,
                // An unreadable payload is not an observation at all.
                None => continue,
            }
        };
        // Keep scanning: the debuggee's own output can carry a marker-like
        // line, and the transaction's own answer is the last one in frame.
        found = Some(RegistrationObservation { present, path });
    }
    found
}

/// Parse the mutation acknowledgement flag out of framed output lines.
///
/// `Some(true)` means the debuggee reported the `require` succeeded;
/// `Some(false)` means it reported failure; `None` means no marker came
/// back at all, which after the boundary is an ambiguous acknowledgement,
/// never a success and never a clean failure.
fn parse_mutation_ack(lines: &[String], marker: &str) -> Option<bool> {
    let mut found = None;
    for line in lines {
        let line = sanitize_frame_line(line);
        let needle = format!("{marker} ");
        let Some(index) = line.rfind(&needle) else {
            continue;
        };
        let rest = line.get(index + needle.len()..).unwrap_or("");
        match rest.split_whitespace().next() {
            // Keep scanning rather than returning: reloaded module code
            // that prints a marker-like line lands in the same frame, and
            // the acknowledgement `p` emits is the last value in it.
            Some("1") => found = Some(true),
            Some("0") => found = Some(false),
            _ => continue,
        }
    }
    found
}

/// The outcome of one executed reload transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadExecution {
    /// The terminal outcome in the frozen R01 vocabulary.
    pub outcome: LoadedModuleReloadOutcome,
    /// The phase the transaction reached.
    pub phase_reached: ReloadTransactionPhase,
    /// Whether the mutation bytes reached the debuggee.
    ///
    /// This is the possibly-applied boundary as an observable fact: it is
    /// true exactly when the runtime-module generation advanced.
    pub mutation_issued: bool,
    /// The mechanism the transaction executed under.
    pub mechanism: ReloadMechanism,
    /// What the generation clock did.
    pub generation: GenerationAdvance,
}

impl ReloadExecution {
    /// Whether the outcome may be projected to a client as clean.
    pub fn projects_as_clean(&self) -> bool {
        self.outcome.projects_as_clean()
    }
}

/// Assemble an execution result, applying the outcome to the clock.
fn settle(
    outcome: LoadedModuleReloadOutcome,
    phase_reached: ReloadTransactionPhase,
    mutation_issued: bool,
    mechanism: ReloadMechanism,
    clock: &mut RuntimeModuleGenerationClock,
) -> ReloadExecution {
    let generation = clock.apply(&outcome);
    ReloadExecution { outcome, phase_reached, mutation_issued, mechanism, generation }
}

/// Execute one bounded loaded-module reload transaction.
///
/// The plan must already be admitted by [`super::plan_reload`]; this
/// function revalidates currentness immediately before mutation and then
/// walks the frozen phases:
///
/// ```text
/// preflight  revalidate identity, then observe registration (read-only)
/// prepare    build the command plan
/// mutate     issue exactly one mutation  ← possibly-applied boundary
/// read back  observe the registration again
/// commit     advance the runtime-module generation
/// ```
///
/// Once the mutation is issued, every path returns either `Reloaded` or
/// `IndeterminatePossiblyApplied`; there is no route back to a clean
/// pre-mutation failure, because there is no evidence that could earn one.
///
/// # Serialization
///
/// The `&mut` on both the channel and the clock makes two concurrent
/// executions over *the same* channel or clock impossible — the borrow
/// checker rejects it, so admission through read-back cannot interleave
/// for a single transport.
///
/// That is not the same as debuggee-wide exclusivity. Two channels onto
/// one debuggee, or two clocks for one process, would each satisfy the
/// borrow checker and still interleave mutations. Whoever owns the
/// adapter must therefore keep one channel and one clock per debuggee and
/// route reload operations through the serialized broker — #10098 calls
/// for one serialized broker operation, and R03 (#10102) owns that
/// wiring. The executor cannot enforce it from here and does not pretend
/// to.
pub fn execute_reload<C: ReloadRuntimeChannel + ?Sized>(
    plan: &LoadedModuleReloadPlan,
    mechanism: ReloadMechanism,
    channel: &mut C,
    clock: &mut RuntimeModuleGenerationClock,
) -> ReloadExecution {
    let subject = plan.subject();
    // Markers are bound to this transaction's operation identity, so the
    // debuggee's own output cannot name this exchange's answer.
    let operation = subject.operation_identity();
    let preflight_marker = marker_for(PREFLIGHT_MARKER, operation);
    let mutation_marker = marker_for(MUTATION_MARKER, operation);
    let readback_marker = marker_for(READBACK_MARKER, operation);

    // Admission: the command plan is derivable at all. An unsafe key or an
    // unimplemented mechanism refuses here, before the debuggee is touched.
    let commands = match plan_commands(subject, mechanism) {
        Ok(commands) => commands,
        Err(error) => {
            return settle(
                LoadedModuleReloadOutcome::Refused { disposition: error.refusal() },
                ReloadTransactionPhase::Admission,
                false,
                mechanism,
                clock,
            );
        }
    };

    // Admission: the generation clock must still be able to move.
    //
    // `RuntimeModuleGenerationClock::apply` saturates at `u64::MAX` and
    // still reports `Advanced`, while `reference_is_stale` compares
    // generations strictly (`<`). At exhaustion those two combine into a
    // fail-open: a mutation would report a generation advance that did not
    // happen, and every reference minted at `u64::MAX` would stay current
    // across it — exactly the "old identities survive a possibly applied
    // outcome" shape the invalidation contract forbids.
    //
    // `RuntimeModuleGeneration::is_exhausted`'s own doc requires treating
    // everything at that ceiling as stale rather than risking a reused
    // generation. The executor cannot honour that after mutating, so it
    // refuses before mutating: an exhausted clock can no longer establish
    // currentness for anything, which is `source_not_exact_or_stale`.
    if clock.current().is_exhausted() {
        return settle(
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale,
            },
            ReloadTransactionPhase::Admission,
            false,
            mechanism,
            clock,
        );
    }

    // Preflight: revalidate the exact identity against the live view. A
    // stale plan is refused here rather than mutating whatever now
    // occupies the subject's key.
    let refuse_preflight = |disposition, clock: &mut RuntimeModuleGenerationClock| {
        settle(
            LoadedModuleReloadOutcome::Refused { disposition },
            ReloadTransactionPhase::Preflight,
            false,
            mechanism,
            clock,
        )
    };
    let Some(view) = channel.currentness_view() else {
        return refuse_preflight(LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady, clock);
    };
    if !subject.is_current_against(&view)
        || subject.session_generation() != plan.admitted_session_generation()
        || subject.suspension_generation() != plan.admitted_suspension_generation()
    {
        return refuse_preflight(LoadedModuleReloadEligibility::SourceNotExactOrStale, clock);
    }

    // Preflight observation: still registered, at the bound path?
    match channel.run_readonly(commands.preflight()) {
        ChannelSettlement::Acknowledged(lines) => {
            match parse_registration(&lines, &preflight_marker) {
                Some(observation) if !observation.present => {
                    return refuse_preflight(LoadedModuleReloadEligibility::NotLoaded, clock);
                }
                Some(observation) if observation.path != subject.resolved_runtime_path() => {
                    // The key now resolves somewhere else: the runtime
                    // mapping no longer binds exactly one subject.
                    return refuse_preflight(
                        LoadedModuleReloadEligibility::AmbiguousRuntimeMapping,
                        clock,
                    );
                }
                Some(_) => {}
                None => {
                    // No marker came back. Nothing was mutated, so this is
                    // an ordinary pre-mutation failure.
                    return settle(
                        LoadedModuleReloadOutcome::FailedBeforeMutation {
                            phase: ReloadTransactionPhase::Preflight,
                            cause: PreMutationFailureCause::PrepareFailed,
                        },
                        ReloadTransactionPhase::Preflight,
                        false,
                        mechanism,
                        clock,
                    );
                }
            }
        }
        ChannelSettlement::NotIssued(_)
        | ChannelSettlement::Unsettled(UnsettledKind::Timeout)
        | ChannelSettlement::Unsettled(UnsettledKind::TransportLoss) => {
            return settle(
                LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::Preflight,
                    cause: PreMutationFailureCause::PrepareFailed,
                },
                ReloadTransactionPhase::Preflight,
                false,
                mechanism,
                clock,
            );
        }
        ChannelSettlement::Unsettled(UnsettledKind::Cancelled) => {
            return settle(
                LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::Preflight,
                    cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
                },
                ReloadTransactionPhase::Preflight,
                false,
                mechanism,
                clock,
            );
        }
    }

    // Last currentness check before the boundary.
    //
    // The mutation's `require` reads the file from disk, so the bytes that
    // execute are whatever is saved at that instant — not the bytes whose
    // digest admission approved. A save landing between the first check and
    // the mutation would run source this transaction never admitted. This
    // re-reads the live view after preflight so the window is the smallest
    // the transaction can make it.
    //
    // It cannot be closed entirely: any gap between observing a digest and
    // Perl reading the file is a race, and this cohort's mechanism has no
    // immutable artifact to bind the admitted digest to. The residual is
    // stated in the module docs and is a real limit of
    // `IncDeletionAndRequire`, not a defect this executor can fix alone.
    match channel.currentness_view() {
        Some(view) if subject.is_current_against(&view) => {}
        Some(_) => {
            return refuse_preflight(LoadedModuleReloadEligibility::SourceNotExactOrStale, clock);
        }
        None => {
            return refuse_preflight(
                LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady,
                clock,
            );
        }
    }

    // The boundary. Everything after this point is possibly applied.
    let mutation = channel.run_mutation(commands.mutation());
    let ack = match mutation {
        ChannelSettlement::NotIssued(_) => {
            // The bytes never reached the debuggee: still pre-mutation.
            return settle(
                LoadedModuleReloadOutcome::FailedBeforeMutation {
                    phase: ReloadTransactionPhase::Prepare,
                    cause: PreMutationFailureCause::PrepareFailed,
                },
                ReloadTransactionPhase::Prepare,
                false,
                mechanism,
                clock,
            );
        }
        ChannelSettlement::Unsettled(kind) => {
            return settle(
                LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                    phase: ReloadTransactionPhase::RuntimeMutationBegins,
                    cause: kind.post_boundary_cause(),
                },
                ReloadTransactionPhase::RuntimeMutationBegins,
                true,
                mechanism,
                clock,
            );
        }
        ChannelSettlement::Acknowledged(lines) => parse_mutation_ack(&lines, &mutation_marker),
    };

    // Read-back runs whatever the acknowledgement said: a failed `require`
    // still deleted the `%INC` entry, so the registration is the only
    // evidence that can distinguish a completed reload from a partial one.
    let indeterminate = |cause, clock: &mut RuntimeModuleGenerationClock| {
        settle(
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause,
            },
            ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
            true,
            mechanism,
            clock,
        )
    };
    let read_back = match channel.run_readonly(commands.read_back()) {
        ChannelSettlement::Acknowledged(lines) => lines,
        ChannelSettlement::NotIssued(_) => {
            return indeterminate(IndeterminateCause::ReadBackInconclusive, clock);
        }
        ChannelSettlement::Unsettled(kind) => {
            return indeterminate(kind.post_boundary_cause(), clock);
        }
    };
    let Some(observation) = parse_registration(&read_back, &readback_marker) else {
        return indeterminate(IndeterminateCause::ReadBackInconclusive, clock);
    };

    // The only route to `Reloaded`: the debuggee acknowledged the require
    // succeeded *and* read back a refreshed registration at the bound
    // path. A prompt, an empty frame, or a missing flag never gets here.
    let reloaded = ack == Some(true)
        && observation.present
        && observation.path == subject.resolved_runtime_path();
    if reloaded {
        settle(
            LoadedModuleReloadOutcome::Reloaded,
            ReloadTransactionPhase::CommitGeneration,
            true,
            mechanism,
            clock,
        )
    } else {
        indeterminate(IndeterminateCause::ReadBackInconclusive, clock)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reload::generation::RuntimeModuleGeneration;
    use crate::reload::subject::{ModuleClassification, SubjectCandidate};
    use crate::reload::transaction::{phase_permits_outcome, plan_reload};
    use crate::reload::{GenerationEffect, ReloadAdmissionObservation};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    const KEY: &str = "App/Core.pm";
    const PATH: &str = "/ws/lib/App/Core.pm";
    /// Operation identity every test subject carries.
    const OP: u64 = 9;

    /// The operation-bound marker a transaction for `OP` actually emits.
    fn mark(stem: &str) -> String {
        marker_for(stem, OP)
    }

    fn candidate() -> SubjectCandidate {
        SubjectCandidate {
            session_generation: Some(4),
            suspension_generation: Some(12),
            observation_generation: Some(3),
            inc_key: KEY.to_string(),
            resolved_runtime_path: PATH.to_string(),
            saved_content_digest: "sha256:9f2c".to_string(),
            logical_source_uri: "file:///ws/lib/App/Core.pm".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
            launch_root: "/ws".to_string(),
            module_classification: Some(ModuleClassification::SourceBackedPerlModule),
            operation_identity: 9,
        }
    }

    fn admitted_observation() -> ReloadAdmissionObservation {
        ReloadAdmissionObservation {
            stopped_and_command_ready: true,
            runtime_supported: true,
            loaded_in_runtime: true,
            within_launch_authority: true,
            runtime_mapping_unambiguous: true,
            identity_binding_complete: true,
            identity_current: true,
            client_source_matches_saved: true,
            module_classification: ModuleClassification::SourceBackedPerlModule,
            active_frame_in_target: false,
        }
    }

    fn current_view() -> SubjectCurrentnessView {
        SubjectCurrentnessView {
            session_generation: 4,
            suspension_generation: 12,
            observation_generation: 3,
            saved_content_digest: "sha256:9f2c".to_string(),
            perl_identity: "perl 5.42.0".to_string(),
        }
    }

    fn admitted_plan() -> Result<LoadedModuleReloadPlan, Box<dyn std::error::Error>> {
        let subject = candidate().bind().map_err(|_| "candidate must bind")?;
        plan_reload(&subject, &admitted_observation()).map_err(|_| "plan must admit".into())
    }

    /// A scripted channel: each exchange returns the next queued settlement.
    ///
    /// `view` answers the first currentness check and `later_view`, when
    /// set, answers every later one — the seam that models a save landing
    /// between admission and the mutation.
    struct ScriptedChannel {
        view: Option<SubjectCurrentnessView>,
        later_view: Option<Option<SubjectCurrentnessView>>,
        readonly: Vec<ChannelSettlement>,
        mutation: ChannelSettlement,
        issued_mutations: Vec<Vec<String>>,
        readonly_calls: usize,
        view_calls: usize,
    }

    impl ScriptedChannel {
        fn new(readonly: Vec<ChannelSettlement>, mutation: ChannelSettlement) -> ScriptedChannel {
            ScriptedChannel {
                view: Some(current_view()),
                later_view: None,
                readonly,
                mutation,
                issued_mutations: Vec::new(),
                readonly_calls: 0,
                view_calls: 0,
            }
        }

        /// Build an observation frame the way the debuggee does: the
        /// path travels hex-encoded.
        fn ok(marker: &str, present: bool, path: &str) -> ChannelSettlement {
            let state = if present { "present" } else { "absent" };
            let payload = if path == "-" { "-".to_string() } else { perl_hex(path) };
            ChannelSettlement::Acknowledged(vec![format!("{marker} {state} {payload}")])
        }

        /// The happy path: preflight present, mutation ok, read-back present.
        fn happy() -> ScriptedChannel {
            ScriptedChannel::new(
                vec![
                    ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH),
                    ScriptedChannel::ok(&mark(READBACK_MARKER), true, PATH),
                ],
                ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
            )
        }
    }

    impl ReloadRuntimeChannel for ScriptedChannel {
        fn currentness_view(&mut self) -> Option<SubjectCurrentnessView> {
            self.view_calls += 1;
            match (self.view_calls, &self.later_view) {
                (1, _) => self.view.clone(),
                (_, Some(later)) => later.clone(),
                (_, None) => self.view.clone(),
            }
        }

        fn run_readonly(&mut self, _commands: &[String]) -> ChannelSettlement {
            let settlement = self
                .readonly
                .get(self.readonly_calls)
                .cloned()
                .unwrap_or(ChannelSettlement::Unsettled(UnsettledKind::Timeout));
            self.readonly_calls += 1;
            settlement
        }

        fn run_mutation(&mut self, commands: &[String]) -> ChannelSettlement {
            self.issued_mutations.push(commands.to_vec());
            self.mutation.clone()
        }
    }

    fn run(channel: &mut ScriptedChannel) -> Result<ReloadExecution, Box<dyn std::error::Error>> {
        let plan = admitted_plan()?;
        let mut clock = RuntimeModuleGenerationClock::new();
        Ok(execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, channel, &mut clock))
    }

    // ---------------------------------------------------------------
    // The load-bearing invariant
    // ---------------------------------------------------------------

    /// Every reachable execution: the phase/outcome pair is contract-valid,
    /// and the mutation-issued flag agrees exactly with the generation.
    ///
    /// This is the possibly-applied boundary stated as one property. It is
    /// the test that fails if any future branch invents a route from an
    /// issued mutation back to a clean terminal state.
    #[test]
    fn every_execution_holds_the_possibly_applied_boundary() -> TestResult {
        let readonly_settlements = || {
            vec![
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH),
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), false, PATH),
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, "/other/App/Core.pm"),
                ChannelSettlement::Acknowledged(vec!["  DB<2> ".to_string()]),
                ChannelSettlement::NotIssued("no stdin".to_string()),
                ChannelSettlement::Unsettled(UnsettledKind::Timeout),
                ChannelSettlement::Unsettled(UnsettledKind::TransportLoss),
                ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
            ]
        };
        let mutation_settlements = vec![
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
            ChannelSettlement::Acknowledged(vec![format!("{} 0", mark(MUTATION_MARKER))]),
            ChannelSettlement::Acknowledged(vec!["  DB<3> ".to_string()]),
            ChannelSettlement::NotIssued("write failed".to_string()),
            ChannelSettlement::Unsettled(UnsettledKind::Timeout),
            ChannelSettlement::Unsettled(UnsettledKind::TransportLoss),
            ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
        ];
        let read_back_settlements = || {
            vec![
                ScriptedChannel::ok(&mark(READBACK_MARKER), true, PATH),
                ScriptedChannel::ok(&mark(READBACK_MARKER), false, PATH),
                ScriptedChannel::ok(&mark(READBACK_MARKER), true, "/other/App/Core.pm"),
                ChannelSettlement::Acknowledged(vec!["  DB<4> ".to_string()]),
                ChannelSettlement::NotIssued("gone".to_string()),
                ChannelSettlement::Unsettled(UnsettledKind::Timeout),
                ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
            ]
        };

        let mut executions = 0_usize;
        let mut saw_reloaded = false;
        let mut saw_indeterminate = false;
        let mut saw_refused = false;
        let mut saw_failed = false;

        for view in [Some(current_view()), None] {
            for preflight in readonly_settlements() {
                for mutation in &mutation_settlements {
                    for read_back in read_back_settlements() {
                        let mut channel = ScriptedChannel::new(
                            vec![preflight.clone(), read_back],
                            mutation.clone(),
                        );
                        channel.view = view.clone();
                        let execution = run(&mut channel)?;
                        executions += 1;

                        // 1. The phase/outcome pair is contract-valid.
                        assert!(
                            phase_permits_outcome(execution.phase_reached, &execution.outcome),
                            "invalid phase/outcome pair: {execution:?}"
                        );

                        // 2. Issuing the mutation and advancing the
                        //    generation are the same event.
                        let advanced = execution.generation.advanced();
                        assert_eq!(
                            execution.mutation_issued, advanced,
                            "mutation_issued must equal generation advance: {execution:?}"
                        );
                        assert_eq!(
                            execution.outcome.generation_effect() == GenerationEffect::Advance,
                            advanced,
                            "generation effect must match the clock: {execution:?}"
                        );

                        // 3. An issued mutation never projects as clean
                        //    unless it is a fully evidenced reload.
                        if execution.mutation_issued {
                            assert!(
                                matches!(
                                    execution.outcome,
                                    LoadedModuleReloadOutcome::Reloaded
                                        | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
                                ),
                                "issued mutation must be reloaded or indeterminate: {execution:?}"
                            );
                        } else {
                            assert!(
                                !matches!(
                                    execution.outcome,
                                    LoadedModuleReloadOutcome::Reloaded
                                        | LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. }
                                ),
                                "unissued mutation must not claim runtime effect: {execution:?}"
                            );
                        }

                        match execution.outcome {
                            LoadedModuleReloadOutcome::Reloaded => saw_reloaded = true,
                            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied { .. } => {
                                saw_indeterminate = true;
                            }
                            LoadedModuleReloadOutcome::Refused { .. } => saw_refused = true,
                            LoadedModuleReloadOutcome::FailedBeforeMutation { .. } => {
                                saw_failed = true;
                            }
                        }
                    }
                }
            }
        }

        // The sweep is non-vacuous: all four terminal classes occur.
        assert!(executions >= 700, "sweep must be broad, ran {executions}");
        assert!(saw_reloaded && saw_indeterminate && saw_refused && saw_failed);
        Ok(())
    }

    // ---------------------------------------------------------------
    // The twelve fixture races from #10098
    // ---------------------------------------------------------------

    /// 1. Ordinary module reload succeeds end to end.
    #[test]
    fn race_01_ordinary_module_reloads() -> TestResult {
        let execution = run(&mut ScriptedChannel::happy())?;
        assert_eq!(execution.outcome, LoadedModuleReloadOutcome::Reloaded);
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::CommitGeneration);
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// 2. The mechanism is unavailable: refuse, never fall back to `%INC`.
    #[test]
    fn race_02_unavailable_mechanism_refuses_without_fallback() -> TestResult {
        for mechanism in [
            ReloadMechanism::DoOrRequireHelper,
            ReloadMechanism::WorkspaceRuntimeHelperObserver,
            ReloadMechanism::ClassRefreshCompatibilitySubject,
        ] {
            let plan = admitted_plan()?;
            let mut clock = RuntimeModuleGenerationClock::new();
            let mut channel = ScriptedChannel::happy();
            let execution = execute_reload(&plan, mechanism, &mut channel, &mut clock);
            assert_eq!(
                execution.outcome,
                LoadedModuleReloadOutcome::Refused {
                    disposition: LoadedModuleReloadEligibility::UnsupportedRuntime
                },
                "{mechanism:?} must refuse"
            );
            assert!(!execution.mutation_issued);
            assert!(
                channel.issued_mutations.is_empty(),
                "{mechanism:?} must not silently execute the %INC path"
            );
            assert!(!clock.current().is_exhausted());
            assert_eq!(clock.current(), Default::default());
        }
        Ok(())
    }

    /// 3. Preflight cannot observe the registration: pre-mutation failure,
    ///    and nothing was written to the debuggee.
    #[test]
    fn race_03_preflight_failure_never_mutates() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![ChannelSettlement::NotIssued("no session".to_string())],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::PrepareFailed,
            }
        );
        assert!(!execution.mutation_issued);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 4. An active frame enters the target between plan and execution.
    ///    Admission owns this class; the executor must never admit it.
    #[test]
    fn race_04_active_frame_refuses_at_admission() -> TestResult {
        let subject = candidate().bind().map_err(|_| "bind")?;
        let observation =
            ReloadAdmissionObservation { active_frame_in_target: true, ..admitted_observation() };
        assert_eq!(
            plan_reload(&subject, &observation),
            Err(LoadedModuleReloadEligibility::ActiveFrameInTarget)
        );
        Ok(())
    }

    /// 5. The saved source changes between plan and execution: the digest
    ///    no longer matches the live view, so the plan is stale.
    #[test]
    fn race_05_source_changed_after_plan_refuses() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        channel.view = Some(SubjectCurrentnessView {
            saved_content_digest: "sha256:different".to_string(),
            ..current_view()
        });
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 6. The same module key now resolves under a different include root:
    ///    the runtime mapping is ambiguous, so nothing is mutated.
    #[test]
    fn race_06_same_name_other_include_root_refuses() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, "/other/lib/App/Core.pm")],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::AmbiguousRuntimeMapping
            }
        );
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 7. The runtime rejects the reload (`require` died). The `%INC` entry
    ///    was already deleted, so this is possibly applied — never a clean
    ///    failure.
    #[test]
    fn race_07_runtime_rejection_is_possibly_applied() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH),
                ScriptedChannel::ok(&mark(READBACK_MARKER), false, "-"),
            ],
            ChannelSettlement::Acknowledged(vec![format!("{} 0", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::ReadBackInconclusive,
            }
        );
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        assert!(!execution.projects_as_clean());
        Ok(())
    }

    /// 8. Timeout *before* the boundary: nothing ran, so no generation moves.
    #[test]
    fn race_08_timeout_before_boundary_advances_nothing() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![ChannelSettlement::Unsettled(UnsettledKind::Timeout)],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::PrepareFailed,
            }
        );
        assert!(!execution.generation.advanced());
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// 9. Timeout and transport loss *after* mutation begins: possibly
    ///    applied, generation advanced, never projected clean or empty.
    ///
    ///    This is the exact case `query_inc_entries` answers with an empty
    ///    list for a read-only query. A mutation must not.
    #[test]
    fn race_09_loss_after_boundary_is_never_clean_or_empty() -> TestResult {
        for (kind, expected) in [
            (UnsettledKind::Timeout, IndeterminateCause::TimeoutAfterMutationBegan),
            (UnsettledKind::TransportLoss, IndeterminateCause::TransportLossAfterMutationBegan),
        ] {
            let mut channel = ScriptedChannel::new(
                vec![ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH)],
                ChannelSettlement::Unsettled(kind),
            );
            let execution = run(&mut channel)?;
            assert_eq!(
                execution.outcome,
                LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                    phase: ReloadTransactionPhase::RuntimeMutationBegins,
                    cause: expected,
                },
                "{kind:?}"
            );
            assert!(execution.mutation_issued);
            assert!(execution.generation.advanced());
            assert!(!execution.projects_as_clean());
        }
        Ok(())
    }

    /// 10. Cancellation on both sides of the boundary. Before: a clean
    ///     pre-mutation cancel. After: possibly applied, because a cancel
    ///     cannot prove non-application.
    #[test]
    fn race_10_cancellation_splits_at_the_boundary() -> TestResult {
        let mut before = ScriptedChannel::new(
            vec![ChannelSettlement::Unsettled(UnsettledKind::Cancelled)],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut before)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::FailedBeforeMutation {
                phase: ReloadTransactionPhase::Preflight,
                cause: PreMutationFailureCause::CancelledBeforeMutationBegan,
            }
        );
        assert!(!execution.generation.advanced());
        assert!(before.issued_mutations.is_empty());

        let mut after = ScriptedChannel::new(
            vec![ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH)],
            ChannelSettlement::Unsettled(UnsettledKind::Cancelled),
        );
        let execution = run(&mut after)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeMutationBegins,
                cause: IndeterminateCause::AmbiguousAcknowledgement,
            }
        );
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// 11. The debuggee exits during the transaction: the read-back is
    ///     never issued, and the outcome stays possibly applied.
    #[test]
    fn race_11_debuggee_exit_during_transaction() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH),
                ChannelSettlement::NotIssued("process exited".to_string()),
            ],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::ReadBackInconclusive,
            }
        );
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// 12. A repeated request against a stale plan: the session generation
    ///     moved, so the plan is refused rather than re-executed.
    #[test]
    fn race_12_repeated_request_against_stale_plan_refuses() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        channel.view = Some(SubjectCurrentnessView { suspension_generation: 13, ..current_view() });
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(channel.issued_mutations.is_empty());

        // A replaced session is refused the same way.
        let mut replaced = ScriptedChannel::happy();
        replaced.view = Some(SubjectCurrentnessView { session_generation: 5, ..current_view() });
        let execution = run(&mut replaced)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(replaced.issued_mutations.is_empty());
        Ok(())
    }

    /// The debuggee not being stopped and command-ready refuses before any
    /// observation is attempted.
    #[test]
    fn not_command_ready_refuses_before_observing() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        channel.view = None;
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady
            }
        );
        assert_eq!(channel.readonly_calls, 0);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    // ---------------------------------------------------------------
    // No arbitrary command, path, or expression surface
    // ---------------------------------------------------------------

    /// A prompt is not an acknowledgement. Framed output that carries no
    /// mutation marker is ambiguous, not success.
    #[test]
    fn prompt_alone_is_not_an_acknowledgement() -> TestResult {
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH),
                ScriptedChannel::ok(&mark(READBACK_MARKER), true, PATH),
            ],
            ChannelSettlement::Acknowledged(vec![
                "  DB<2> ".to_string(),
                "ok".to_string(),
                "1".to_string(),
            ]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::ReadBackInconclusive,
            }
        );
        assert!(!execution.projects_as_clean());
        Ok(())
    }

    /// The module key allowlist refuses every injection shape, and the
    /// refusal happens before any command text is built.
    #[test]
    fn unsafe_module_keys_are_refused_before_any_command() -> TestResult {
        let hostile = [
            "App/Core.pm'); system('id'); ('",
            "App/Core.pm\nq foo",
            "/abs/App/Core.pm",
            "../../etc/passwd.pm",
            "App/Core.pm; print 1",
            "App Core.pm",
            "App/(Core).pm",
            "App/Core.pl",
            "App//Core.pm",
            "App/Core.pm\"",
            "",
        ];
        for key in hostile {
            assert!(!module_key_is_safe(key), "{key:?} must be refused");
            let hostile_candidate = SubjectCandidate { inc_key: key.to_string(), ..candidate() };
            // A key that cannot bind at all is already refused upstream;
            // only bindable-but-unsafe keys reach the command planner.
            if let Ok(subject) = hostile_candidate.bind() {
                assert_eq!(
                    plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire),
                    Err(CommandPlanError::UnsafeModuleKey),
                    "{key:?} must not produce commands"
                );
            }
        }
        // The ordinary key is accepted, so the guard is not vacuous.
        assert!(module_key_is_safe(KEY));
        assert!(module_key_is_safe("Deep/Nested/Mod-2.0/Thing.pm"));
        Ok(())
    }

    /// An unsafe key refuses the whole transaction without touching the
    /// debuggee, and reports an inexact-identity disposition.
    #[test]
    fn unsafe_key_refuses_the_transaction() -> TestResult {
        let hostile = SubjectCandidate { inc_key: "App/Core.pm; die".to_string(), ..candidate() }
            .bind()
            .map_err(|_| "hostile candidate must still bind")?;
        let plan = plan_reload(&hostile, &admitted_observation())
            .map_err(|_| "admission is observation-driven")?;
        let mut clock = RuntimeModuleGenerationClock::new();
        let mut channel = ScriptedChannel::happy();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::Admission);
        assert_eq!(channel.readonly_calls, 0);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// The generated commands interpolate only the bound `%INC` key, and
    /// the mutation is exactly one command.
    #[test]
    fn commands_are_derived_only_from_the_bound_subject() -> TestResult {
        let subject = candidate().bind().map_err(|_| "bind")?;
        let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
            .map_err(|_| "safe key must plan")?;
        assert_eq!(commands.mutation().len(), 1, "exactly one mutation command");
        assert_eq!(commands.preflight().len(), 1);
        assert_eq!(commands.read_back().len(), 1);
        for command in commands
            .preflight()
            .iter()
            .chain(commands.mutation().iter())
            .chain(commands.read_back().iter())
        {
            assert!(!command.contains('\n'), "no command may embed a newline: {command}");
            // The key is carried as hex, not as literal text.
            assert!(command.contains(&perl_hex(KEY)), "key must be hex-encoded: {command}");
            assert!(!command.contains(KEY), "key must not appear literally: {command}");
        }
        assert!(commands.mutation()[0].contains("delete $INC"));
        // The mutation requires the admitted absolute path, not the key,
        // so a reordered @INC cannot redirect it to another file.
        // The mutation requires the admitted path, decoded from hex, so a
        // reordered @INC cannot redirect it to another file.
        assert!(commands.mutation()[0].contains(&perl_hex(PATH)));
        assert!(commands.mutation()[0].contains("require $p"));
        // The read-only observations never interpolate the path; they only
        // report what the runtime says so it can be compared.
        // The read-only observations never carry the path at all; they
        // report what the runtime says so it can be compared.
        assert!(!commands.preflight()[0].contains(&perl_hex(PATH)));
        assert!(!commands.read_back()[0].contains(&perl_hex(PATH)));
        // Preflight is read-only: it never deletes, requires, or evals.
        assert!(!commands.preflight()[0].contains("delete"));
        assert!(!commands.preflight()[0].contains("require"));
        assert!(!commands.preflight()[0].contains("eval"));
        assert!(!commands.read_back()[0].contains("delete"));
        assert!(!commands.read_back()[0].contains("require"));
        Ok(())
    }

    /// Exactly one mechanism is executable; the record still describes all
    /// four, so refusal is a decision rather than an omission.
    #[test]
    fn exactly_one_mechanism_is_executable() {
        let executable: Vec<ReloadMechanism> =
            ReloadMechanism::ALL.into_iter().filter(|m| mechanism_is_executable(*m)).collect();
        assert_eq!(executable, vec![ReloadMechanism::IncDeletionAndRequire]);
        assert_eq!(super::super::mechanism_records().len(), 4);
    }

    // ---------------------------------------------------------------
    // Live proof: the generated commands against a real perl5db debuggee
    // ---------------------------------------------------------------
    //
    // The scripted tests above prove the state machine. They cannot prove
    // that the *command text* is valid Perl, that `p do { ... }` survives
    // perl5db's evaluator, or that `delete $INC` + `require` actually
    // replaces the running code. This fixture proves exactly that, against
    // a real `perl -d` process, and then feeds the debuggee's own output
    // back through the real parser and executor.
    //
    // This is not public-binary proof: it drives perl5db directly rather
    // than through the shipped `perl-dap` adapter. Exact-binary and
    // installed proof are R04 (#10104).

    /// Live debuggee output: everything perl5db wrote to stdout.
    struct LiveRun {
        stdout: String,
    }

    impl LiveRun {
        /// Framed lines carrying a marker, as the framed capture would
        /// hand them to the executor.
        fn lines_with(&self, marker: &str) -> Vec<String> {
            self.stdout
                .lines()
                .filter(|line| line.contains(marker))
                .map(|line| line.to_string())
                .collect()
        }
    }

    /// A channel replaying one real debuggee's recorded output.
    ///
    /// The settlements are real perl5db lines, parsed by the production
    /// parser; only the transport is replayed.
    struct ReplayChannel {
        preflight: Vec<String>,
        mutation: Vec<String>,
        read_back: Vec<String>,
        readonly_calls: usize,
    }

    impl ReloadRuntimeChannel for ReplayChannel {
        fn currentness_view(&mut self) -> Option<SubjectCurrentnessView> {
            Some(current_view())
        }

        fn run_readonly(&mut self, _commands: &[String]) -> ChannelSettlement {
            let lines = if self.readonly_calls == 0 {
                self.preflight.clone()
            } else {
                self.read_back.clone()
            };
            self.readonly_calls += 1;
            ChannelSettlement::Acknowledged(lines)
        }

        fn run_mutation(&mut self, _commands: &[String]) -> ChannelSettlement {
            ChannelSettlement::Acknowledged(self.mutation.clone())
        }
    }

    /// A scratch directory removed on drop.
    ///
    /// The live fixture's checks are `assert_eq!`, which panics rather
    /// than returning through the `Result`, so a trailing `remove_dir_all`
    /// is skipped by unwinding on exactly the runs that matter — a failing
    /// assertion. `Drop` runs on both paths, so a failed run no longer
    /// leaves a rewritten `.pm` behind in the temp directory.
    struct ScratchDir(std::path::PathBuf);

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Acquire the live-Perl instrument, or report NOT_PROVEN.
    ///
    /// Shared by every live fixture on purpose. A missing instrument is
    /// NOT_PROVEN, and NOT_PROVEN must not look like a pass — but Rust has
    /// no runtime "skipped" state, so each test that open-coded this was
    /// one opportunity to silently return `Ok(())`. Centralising it means
    /// a new live test cannot forget: it gets the loud stderr line and the
    /// `PERL_LSP_REQUIRE_LIVE_PERL=1` enforcement for free.
    ///
    /// Returns `Ok(Some(env))` when the instrument is present,
    /// `Ok(None)` when it is absent and the skip is permitted, and `Err`
    /// when it is absent and the caller demanded the proof.
    fn live_perl_or_not_proven(
        test_name: &str,
    ) -> Result<Option<perl_lsp_rs_core::config::PerlOracleEnv>, Box<dyn std::error::Error>> {
        let require_live =
            std::env::var("PERL_LSP_REQUIRE_LIVE_PERL").map(|value| value == "1").unwrap_or(false);
        let refuse = |reason: &str| -> Result<Option<_>, Box<dyn std::error::Error>> {
            eprintln!(
                "NOT_PROVEN {test_name}: {reason}. The generated command plan was not \
                 executed against a real debuggee. Set PERL_LSP_REQUIRE_LIVE_PERL=1 to \
                 make this a failure."
            );
            if require_live {
                return Err(format!("live perl proof required but unavailable: {reason}").into());
            }
            Ok(None)
        };
        let Some(oracle) = perl_lsp_rs_core::config::PerlOracleEnv::for_dap_test_fixture() else {
            return refuse("perl is not on PATH");
        };
        if !perl_debugger_available(&oracle) {
            return refuse("perl is present but `perl -d` is unusable");
        }
        Ok(Some(oracle))
    }

    /// Whether a real `perl -d` is usable here. A missing interpreter or a
    /// missing debugger is an instrument skip, never a pass.
    fn perl_debugger_available(oracle: &perl_lsp_rs_core::config::PerlOracleEnv) -> bool {
        oracle
            .clone()
            .into_command()
            .arg("-d")
            .arg("-e")
            .arg("1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    /// Drive a real `perl -d` debuggee through the given command stream.
    fn drive_live_debuggee(
        oracle: perl_lsp_rs_core::config::PerlOracleEnv,
        scratch: &std::path::Path,
        program: &std::path::Path,
        commands: &[String],
    ) -> Result<LiveRun, String> {
        use std::io::Write as _;

        let mut command = oracle.into_command();
        command
            .arg("-d")
            .arg("-I")
            .arg(scratch)
            .arg("--")
            .arg(program)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().map_err(|error| format!("spawn perl -d: {error}"))?;

        // Write the whole command stream, then close stdin so perl5db
        // reaches EOF and exits even if `q` is swallowed.
        //
        // Every early return here kills the child first: `Child::drop`
        // neither kills nor reaps, so a mid-stream write failure (an
        // `EPIPE` from a debuggee that already died, say) would otherwise
        // leave the process unreaped.
        {
            let Some(mut stdin) = child.stdin.take() else {
                let _ = child.kill();
                let _ = child.wait();
                return Err("perl -d has no stdin".to_string());
            };
            let mut write_all = || -> Result<(), String> {
                for line in commands {
                    stdin
                        .write_all(format!("{line}\n").as_bytes())
                        .map_err(|error| format!("write debugger command: {error}"))?;
                }
                stdin.write_all(b"q\n").map_err(|error| format!("write quit: {error}"))?;
                stdin.flush().map_err(|error| format!("flush: {error}"))
            };
            if let Err(error) = write_all() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }

        // perl5db writes its prompt and `p` output to STDERR whenever
        // STDIN is not a terminal, so both streams are captured and
        // merged. Bounded reads: a hung debugger is killed rather than
        // hanging the suite, mirroring the measurement harness.
        let read_capped = |pipe: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buffer = Vec::new();
                let Some(mut pipe) = pipe else { return buffer };
                let mut chunk = [0u8; 8192];
                loop {
                    if buffer.len() >= 256 * 1024 {
                        break;
                    }
                    match std::io::Read::read(&mut pipe, &mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                    }
                }
                buffer
            })
        };
        let out_reader = read_capped(
            child.stdout.take().map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
        );
        let err_reader = read_capped(
            child.stderr.take().map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        // Kill *and* reap: `Child::drop` does neither, so a
                        // timed-out run would otherwise leave a zombie, and
                        // repeated timeouts would accumulate them. Joining
                        // the readers also releases the pipe ends.
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = out_reader.join();
                        let _ = err_reader.join();
                        return Err("perl -d exceeded the 20s deadline".to_string());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_reader.join();
                    let _ = err_reader.join();
                    return Err(format!("wait perl -d: {error}"));
                }
            }
        }
        let mut merged = out_reader.join().unwrap_or_default();
        merged.extend_from_slice(&err_reader.join().unwrap_or_default());
        // Assertions below read fields out of the merged stream, so the
        // same production sanitizer the parser uses is applied per line.
        let merged = String::from_utf8_lossy(&merged).into_owned();
        let cleaned = merged.lines().map(sanitize_frame_line).collect::<Vec<String>>().join("\n");
        Ok(LiveRun { stdout: cleaned })
    }

    /// Read one marker's trailing field out of live debuggee output.
    /// Takes the **last** match, mirroring the production parsers.
    ///
    /// This matters: the fixture's replacement module deliberately prints
    /// forged marker lines during `require`, so a first-match helper would
    /// read the module's text instead of the transaction's answer — and
    /// would report a passing executor as broken.
    fn live_field(run: &LiveRun, marker: &str) -> Option<String> {
        run.stdout
            .lines()
            .filter_map(|line| {
                // Marker plus separator, so `..._ALIAS` is not found
                // inside `..._ALIAS_AFTER`.
                let needle = format!("{marker} ");
                let index = line.rfind(&needle)?;
                let rest = line.get(index + needle.len()..)?;
                rest.split_whitespace().next().map(|field| field.to_string())
            })
            .next_back()
    }

    /// The generated command plan, executed against a real `perl -d`,
    /// actually replaces the running module — and the debuggee's own
    /// output drives the executor to `Reloaded`.
    ///
    /// Non-vacuity is explicit: the same subroutine returns 41 before the
    /// transaction and 42 after it. If `delete $INC` + `require` did
    /// nothing, the "after" value would still be 41 and this test fails.
    #[test]
    fn live_perl_debuggee_reload_replaces_running_code() -> TestResult {
        // A missing instrument is NOT_PROVEN, and NOT_PROVEN must not look
        // like a pass. Rust has no runtime "skipped" state, so the skip is
        // made loud on stderr and made failable: any host that guarantees
        // perl — CI in particular — sets PERL_LSP_REQUIRE_LIVE_PERL=1 and
        // an unavailable instrument becomes a failure instead of a silent
        // green. Without it, a developer machine without perl still skips
        // rather than reporting a proof it did not run.
        let Some(oracle) =
            live_perl_or_not_proven("live_perl_debuggee_reload_replaces_running_code")?
        else {
            return Ok(());
        };

        let scratch = ScratchDir(std::env::temp_dir().join(format!(
            "perl-reload-live-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0),
        )));
        let scratch = &scratch.0;
        let module_dir = scratch.join("App");
        std::fs::create_dir_all(&module_dir)?;
        let module_path = module_dir.join("Core.pm");
        std::fs::write(&module_path, "package App::Core;\nsub answer { 41 }\n1;\n")?;
        let program_path = scratch.join("main.pl");
        std::fs::write(
            &program_path,
            "use App::Core;\nmy $x = App::Core::answer();\nprint \"RAN $x\\n\";\n",
        )?;

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            // The subject names the real runtime path perl will resolve.
            let resolved = module_path.to_string_lossy().into_owned();
            let subject = SubjectCandidate {
                inc_key: KEY.to_string(),
                resolved_runtime_path: resolved.clone(),
                ..candidate()
            }
            .bind()
            .map_err(|_| "live subject must bind")?;
            let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
                .map_err(|_| "live subject must plan")?;

            // The harness rewrites the module on disk from inside the
            // debuggee, so the rewrite is serialized with the transaction
            // exactly the way an editor save would be.
            // The replacement module deliberately forges the transaction's
            // own markers before defining anything. `require` runs this
            // body inside the mutation exchange, so both lines land in the
            // same frame as the real acknowledgement — a first-match
            // parser would read the module's `0` and report a successful
            // reload as indeterminate. The executor must still recognise
            // its own answer.
            let forged_stem = format!("{MUTATION_MARKER} 0");
            let forged_exact = format!("{} 0", mark(MUTATION_MARKER));
            let replacement = format!(
                "print STDERR \"{forged_stem}\\n\"; \
                 print STDERR \"{forged_exact}\\n\"; \
                 package App::Core; sub answer {{ 42 }} 1;\n"
            );
            // Both the destination and the contents travel as hex, so this
            // harness has no quoting problem of its own to get wrong.
            let rewrite = format!(
                "p do {{ my $f = pack(q(H*),q({})); my $c = pack(q(H*),q({})); \
                 open(my $fh, q(>), $f) or die; print $fh $c; close $fh; \
                 \"PERLLSP_TEST_REWROTE ok\" }}",
                perl_hex(&resolved),
                perl_hex(&replacement)
            );
            // Register the absolute path as a second %INC alias, the way a
            // program that required both spellings would leave it. The
            // transaction must not cost that alias its idempotence.
            let alias = format!(
                "p do {{ my $p = pack(q(H*),q({})); $INC{{$p}} = $p; \"PERLLSP_TEST_ALIAS ok\" }}",
                perl_hex(&resolved)
            );
            let mut stream = vec![
                "p \"PERLLSP_TEST_BEFORE \" . App::Core::answer()".to_string(),
                alias,
                rewrite,
            ];
            stream.extend(commands.preflight().iter().cloned());
            stream.extend(commands.mutation().iter().cloned());
            stream.extend(commands.read_back().iter().cloned());
            stream.push("p \"PERLLSP_TEST_AFTER \" . App::Core::answer()".to_string());
            // Did the pre-existing absolute alias survive the reload?
            stream.push(format!(
                "p do {{ my $p = pack(q(H*),q({})); \"PERLLSP_TEST_ALIAS_AFTER \"                  . (exists $INC{{$p}} ? q(present) : q(absent)) }}",
                perl_hex(&resolved)
            ));

            let run = drive_live_debuggee(oracle, scratch, &program_path, &stream)?;
            let context = || format!("debuggee stdout was:\n{}", run.stdout);

            // The harness itself worked.
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_BEFORE").as_deref(),
                Some("41"),
                "module must start at 41; {}",
                context()
            );
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_REWROTE").as_deref(),
                Some("ok"),
                "harness must rewrite the module; {}",
                context()
            );

            // The generated commands are valid Perl and produced markers.
            assert_eq!(
                live_field(&run, &mark(PREFLIGHT_MARKER)).as_deref(),
                Some("present"),
                "preflight must observe the loaded module; {}",
                context()
            );
            assert_eq!(
                live_field(&run, &mark(MUTATION_MARKER)).as_deref(),
                Some("1"),
                "mutation must report a successful require; {}",
                context()
            );
            assert_eq!(
                live_field(&run, &mark(READBACK_MARKER)).as_deref(),
                Some("present"),
                "read-back must observe the refreshed registration; {}",
                context()
            );

            // The forgery really did reach the stream, so the marker
            // binding and last-match parsing are load-bearing here rather
            // than incidentally satisfied.
            assert!(
                run.stdout.contains(&forged_stem),
                "the fixture must actually forge the bare marker; {}",
                context()
            );
            assert!(
                run.stdout.contains(&forged_exact),
                "the fixture must actually forge the operation-bound marker; {}",
                context()
            );

            // The discriminating assertion: the running code changed.
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_AFTER").as_deref(),
                Some("42"),
                "the reload must replace the running sub; {}",
                context()
            );

            assert_eq!(
                live_field(&run, "PERLLSP_TEST_ALIAS").as_deref(),
                Some("ok"),
                "the fixture must register the absolute alias; {}",
                context()
            );
            // A pre-existing absolute registration keeps its idempotence.
            assert_eq!(
                live_field(&run, "PERLLSP_TEST_ALIAS_AFTER").as_deref(),
                Some("present"),
                "a pre-existing absolute %INC alias must survive the reload; {}",
                context()
            );

            // Close the loop: the debuggee's own lines, parsed by the
            // production parser, drive the executor to `Reloaded`.
            let mut channel = ReplayChannel {
                preflight: run.lines_with(&mark(PREFLIGHT_MARKER)),
                mutation: run.lines_with(&mark(MUTATION_MARKER)),
                read_back: run.lines_with(&mark(READBACK_MARKER)),
                readonly_calls: 0,
            };
            let plan = plan_reload(&subject, &admitted_observation())
                .map_err(|_| "live subject must admit")?;
            let mut clock = RuntimeModuleGenerationClock::new();
            let execution = execute_reload(
                &plan,
                ReloadMechanism::IncDeletionAndRequire,
                &mut channel,
                &mut clock,
            );
            assert_eq!(
                execution.outcome,
                LoadedModuleReloadOutcome::Reloaded,
                "real debuggee output must drive the executor to Reloaded; {}",
                context()
            );
            assert!(execution.mutation_issued);
            assert!(execution.generation.advanced());
            Ok(())
        })();

        // Cleanup is the `ScratchDir` guard's job, so it also runs when an
        // assertion above panics.
        result
    }

    /// A failed reload does not leave a poisoned `%INC` entry behind.
    ///
    /// Perl marks a failed `require` by leaving `$INC{$p}` present but
    /// undefined, and every later `require` of that path then dies with
    /// `Attempt to reload ... aborted` for the life of the process. That
    /// is the realistic case — someone edits a loaded module, introduces
    /// a bug, and reloads — so the transaction must not turn it into a
    /// latent fatal in unrelated code that requires the same path.
    ///
    /// Drives real `perl -d` end to end: registers the absolute alias,
    /// breaks the source, runs the generated mutation, and proves the
    /// entry is gone and a later `require` of it does not die.
    #[test]
    fn live_perl_failed_reload_does_not_poison_inc() -> TestResult {
        let Some(oracle) = live_perl_or_not_proven("live_perl_failed_reload_does_not_poison_inc")?
        else {
            return Ok(());
        };
        let scratch = ScratchDir(std::env::temp_dir().join(format!(
            "perl-reload-fail-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_nanos())
                .unwrap_or(0),
        )));
        let scratch = &scratch.0;
        let module_dir = scratch.join("App");
        std::fs::create_dir_all(&module_dir)?;
        let module_path = module_dir.join("Core.pm");
        std::fs::write(&module_path, "package App::Core;\nsub answer { 41 }\n1;\n")?;
        let program_path = scratch.join("main.pl");
        std::fs::write(&program_path, "use App::Core;\nmy $x = App::Core::answer();\n")?;

        let resolved = module_path.to_string_lossy().into_owned();
        let subject = SubjectCandidate {
            inc_key: KEY.to_string(),
            resolved_runtime_path: resolved.clone(),
            ..candidate()
        }
        .bind()
        .map_err(|_| "subject must bind")?;
        let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
            .map_err(|_| "must plan")?;
        let path_hex = perl_hex(&resolved);

        // Register the absolute alias, then break the source so `require`
        // fails inside the mutation.
        let broken = "package App::Core; sub answer { syntax ( error ;\n";
        let mut stream = vec![
            format!(
                "p do {{ my $p = pack(q(H*),q({path_hex})); $INC{{$p}} = $p; \"PERLLSP_TEST_ALIAS ok\" }}"
            ),
            format!(
                "p do {{ my $f = pack(q(H*),q({path_hex})); my $c = pack(q(H*),q({})); \
                 open(my $fh, q(>), $f) or die; print $fh $c; close $fh; \
                 \"PERLLSP_TEST_BROKE ok\" }}",
                perl_hex(broken)
            ),
        ];
        stream.extend(commands.mutation().iter().cloned());
        // Is the entry gone, and does a later require survive?
        stream.push(format!(
            "p do {{ my $p = pack(q(H*),q({path_hex})); \"PERLLSP_TEST_POISON \" \
             . (exists $INC{{$p}} ? q(present) : q(absent)) }}"
        ));
        // Repair the source, then require again. This is the
        // discriminating step: a poisoned entry makes `require` die with
        // `Attempt to reload ... aborted` regardless of file contents, so
        // recovery would be impossible for the life of the process.
        let repaired = "package App::Core; sub answer { 42 } 1;\n";
        stream.push(format!(
            "p do {{ my $f = pack(q(H*),q({path_hex})); my $c = pack(q(H*),q({})); \
             open(my $fh, q(>), $f) or die; print $fh $c; close $fh; \
             \"PERLLSP_TEST_REPAIRED ok\" }}",
            perl_hex(repaired)
        ));
        stream.push(format!(
            "p do {{ my $p = pack(q(H*),q({path_hex})); \
             my $r = eval {{ require $p; 1 }} ? q(ok) : q(died); \"PERLLSP_TEST_RECOVERS $r\" }}"
        ));

        let run = drive_live_debuggee(oracle, scratch, &program_path, &stream)?;
        let context = || format!("debuggee stdout was:\n{}", run.stdout);

        assert_eq!(live_field(&run, "PERLLSP_TEST_ALIAS").as_deref(), Some("ok"), "{}", context());
        assert_eq!(live_field(&run, "PERLLSP_TEST_BROKE").as_deref(), Some("ok"), "{}", context());
        // The require really did fail — otherwise this proves nothing.
        assert_eq!(
            live_field(&run, &mark(MUTATION_MARKER)).as_deref(),
            Some("0"),
            "the mutation must fail for this test to mean anything; {}",
            context()
        );
        // No poisoned entry survives the failure.
        assert_eq!(
            live_field(&run, "PERLLSP_TEST_POISON").as_deref(),
            Some("absent"),
            "a failed reload must not leave a poisoned %INC entry; {}",
            context()
        );
        assert_eq!(
            live_field(&run, "PERLLSP_TEST_REPAIRED").as_deref(),
            Some("ok"),
            "{}",
            context()
        );
        // Recovery is possible: with a poisoned entry this dies with
        // `Attempt to reload ... aborted` no matter how good the source is.
        assert_eq!(
            live_field(&run, "PERLLSP_TEST_RECOVERS").as_deref(),
            Some("ok"),
            "a repaired module must be requirable again after a failed reload; {}",
            context()
        );
        Ok(())
    }

    /// Marker parsing refuses partial and malformed frames instead of
    /// reading them as "absent".
    #[test]
    fn registration_parsing_refuses_malformed_frames() {
        assert_eq!(parse_registration(&[], &mark(PREFLIGHT_MARKER)), None);
        assert_eq!(
            parse_registration(
                &[format!("{} maybe /p", mark(PREFLIGHT_MARKER))],
                &mark(PREFLIGHT_MARKER)
            ),
            None
        );
        assert_eq!(
            parse_registration(&[PREFLIGHT_MARKER.to_string()], &mark(PREFLIGHT_MARKER)),
            None
        );
        let present = parse_registration(
            &[format!("  DB<2> {} present {}", mark(PREFLIGHT_MARKER), perl_hex(PATH))],
            &mark(PREFLIGHT_MARKER),
        );
        assert_eq!(
            present,
            Some(RegistrationObservation {
                present: true,
                path: "/ws/lib/App/Core.pm".to_string()
            })
        );
        assert_eq!(parse_mutation_ack(&[], &mark(MUTATION_MARKER)), None);
        assert_eq!(
            parse_mutation_ack(&[format!("{} 2", mark(MUTATION_MARKER))], &mark(MUTATION_MARKER)),
            None
        );
        assert_eq!(
            parse_mutation_ack(&[format!("{} 1", mark(MUTATION_MARKER))], &mark(MUTATION_MARKER)),
            Some(true)
        );
        assert_eq!(
            parse_mutation_ack(&[format!("{} 0", mark(MUTATION_MARKER))], &mark(MUTATION_MARKER)),
            Some(false)
        );
    }

    /// A resolved path containing spaces round-trips intact.
    ///
    /// Taking the next whitespace token as the path truncates
    /// `/ws/my lib/App/Core.pm` to `/ws/my`, which mismatches the bound
    /// subject and refuses a current subject as AmbiguousRuntimeMapping.
    #[test]
    fn paths_with_spaces_are_not_truncated() -> TestResult {
        let spaced = "/ws/my lib/App/Core.pm";
        assert_eq!(
            parse_registration(
                &[format!("{} present {}", mark(READBACK_MARKER), perl_hex(spaced))],
                &mark(READBACK_MARKER)
            ),
            Some(RegistrationObservation { present: true, path: spaced.to_string() })
        );

        // End to end: a subject at a spaced path still reloads.
        let subject = SubjectCandidate { resolved_runtime_path: spaced.to_string(), ..candidate() }
            .bind()
            .map_err(|_| "spaced subject must bind")?;
        let plan = plan_reload(&subject, &admitted_observation())
            .map_err(|_| "spaced subject must admit")?;
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, spaced),
                ScriptedChannel::ok(&mark(READBACK_MARKER), true, spaced),
            ],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let mut clock = RuntimeModuleGenerationClock::new();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Reloaded,
            "a spaced resolved path must not be read as a different subject"
        );

        // A genuinely different path still refuses, so the fix did not
        // turn the mapping check into a rubber stamp.
        let mut moved = ScriptedChannel::new(
            vec![ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, "/ws/other lib/App/Core.pm")],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let mut clock = RuntimeModuleGenerationClock::new();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut moved, &mut clock);
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::AmbiguousRuntimeMapping
            }
        );
        assert!(moved.issued_mutations.is_empty());
        Ok(())
    }

    /// A save landing between admission and the mutation refuses, and
    /// nothing is written to the debuggee.
    ///
    /// The mutation's `require` reads the file from disk, so admitting on a
    /// digest observed before preflight and then mutating would execute
    /// bytes the transaction never admitted. The executor re-reads the live
    /// view immediately before the boundary; this pins that it does.
    #[test]
    fn source_saved_during_preflight_refuses_before_mutating() -> TestResult {
        let mut channel = ScriptedChannel::happy();
        // Admission sees the bound digest; the check before the boundary
        // sees the file as it is after an editor save.
        channel.later_view = Some(Some(SubjectCurrentnessView {
            saved_content_digest: "sha256:saved-during-preflight".to_string(),
            ..current_view()
        }));
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert!(!execution.mutation_issued);
        assert!(!execution.generation.advanced());
        assert!(
            channel.issued_mutations.is_empty(),
            "unadmitted source must never reach the debuggee"
        );
        // The preflight observation did happen, so the refusal is the
        // second check rather than the first.
        assert_eq!(channel.readonly_calls, 1);
        assert_eq!(channel.view_calls, 2);

        // A debuggee that stops being command-ready in that same window
        // also refuses rather than mutating.
        let mut resumed = ScriptedChannel::happy();
        resumed.later_view = Some(None);
        let execution = run(&mut resumed)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::NotStoppedOrNotCommandReady
            }
        );
        assert!(resumed.issued_mutations.is_empty());

        // Control: an unchanged view still reaches Reloaded, so the new
        // check is not refusing everything.
        let mut unchanged = ScriptedChannel::happy();
        let execution = run(&mut unchanged)?;
        assert_eq!(execution.outcome, LoadedModuleReloadOutcome::Reloaded);
        Ok(())
    }

    /// An exhausted generation clock refuses before mutating.
    ///
    /// `RuntimeModuleGenerationClock::apply` saturates at `u64::MAX` and
    /// still reports `Advanced`, while `reference_is_stale` compares
    /// strictly — so mutating at the ceiling would claim an advance that
    /// did not happen and leave references minted there current across the
    /// reload. The executor refuses instead.
    #[test]
    fn exhausted_generation_clock_refuses_before_mutating() -> TestResult {
        let plan = admitted_plan()?;
        let mut channel = ScriptedChannel::happy();
        // Positioned at the ceiling through the test seam: advancing there
        // one `apply` at a time would take `u64::MAX` iterations.
        let mut clock =
            RuntimeModuleGenerationClock::at_generation(RuntimeModuleGeneration::new(u64::MAX));
        assert!(clock.current().is_exhausted(), "clock must start at the ceiling");

        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::Admission);
        assert!(!execution.mutation_issued);
        assert!(
            channel.issued_mutations.is_empty(),
            "an exhausted clock must not mutate the runtime"
        );
        assert_eq!(channel.view_calls, 0, "refusal precedes any observation");
        Ok(())
    }

    /// Subject values reach Perl as hex, so no path or key character can
    /// have syntactic meaning in the command text.
    ///
    /// This replaces an earlier allowlist that banned parens, backslashes
    /// and quotes. That guard made injection impossible by making
    /// `C:\\ws\\lib\\App\\Core.pm` and `/opt/Perl (local)/lib/App/Core.pm`
    /// permanently unreloadable — ordinary paths on ordinary systems.
    /// Encoding removes the dilemma instead of choosing a side of it.
    #[test]
    fn hostile_path_characters_are_encoded_not_executed() -> TestResult {
        let hostile = [
            "/ws/lib/App/Core.pm) ; system(q(id)); q(",
            "/ws/lib/App/Core.pm\nq foo",
            "/ws/lib/App/Core.pm\\",
            "/ws/lib/App/Core.pm'",
            "/ws/lib/App/Core.pm\"",
            "/opt/Perl (local)/lib/App/Core.pm",
            "C:\\Users\\dev\\ws\\lib\\App\\Core.pm",
        ];
        for path in hostile {
            let subject =
                SubjectCandidate { resolved_runtime_path: path.to_string(), ..candidate() }
                    .bind()
                    .map_err(|_| "subject must bind")?;
            let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
                .map_err(|_| "encoded path must plan")?;
            let mutation = &commands.mutation()[0];
            // Characters this module never writes itself, so their
            // presence could only come from the subject. (`"` is excluded
            // deliberately: the command's own marker string is
            // double-quoted Perl written here, not subject data.)
            for bad in ['\n', '\\', '\''] {
                assert!(
                    !mutation.contains(bad),
                    "{path:?} leaked {bad:?} into the command: {mutation}"
                );
            }
            // Parens appear only as the `q(...)` delimiters this module
            // writes, never from the subject: every hex payload is
            // strictly [0-9a-f].
            assert!(
                mutation.contains(&format!("q({})", perl_hex(path))),
                "{path:?} must be carried as hex: {mutation}"
            );
            assert!(!mutation.contains(path), "{path:?} must not appear literally: {mutation}");
        }
        Ok(())
    }

    /// Only a genuinely unusable path is refused.
    #[test]
    fn unusable_runtime_paths_are_refused() -> TestResult {
        for path in ["", "   ", "/ws/lib/App/\u{0}Core.pm"] {
            assert!(!runtime_path_is_safe(path), "{path:?} must be refused");
        }
        assert!(!runtime_path_is_safe(&"x".repeat(5000)));
        for path in [
            PATH,
            "/ws/my lib/App/Core.pm",
            "C:\\Users\\dev\\ws\\lib\\App\\Core.pm",
            "/opt/Perl (local)/lib/App/Core.pm",
            "/ws/proyectos/lib/App/Core.pm",
        ] {
            assert!(runtime_path_is_safe(path), "{path:?} must be accepted");
        }

        // A NUL-bearing path refuses the whole transaction untouched.
        let unusable = SubjectCandidate {
            resolved_runtime_path: "/ws/lib/App/\u{0}Core.pm".to_string(),
            ..candidate()
        }
        .bind()
        .map_err(|_| "candidate must bind")?;
        let plan = plan_reload(&unusable, &admitted_observation())
            .map_err(|_| "admission is observation-driven")?;
        let mut clock = RuntimeModuleGenerationClock::new();
        let mut channel = ScriptedChannel::happy();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::Refused {
                disposition: LoadedModuleReloadEligibility::SourceNotExactOrStale
            }
        );
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::Admission);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// A relative bound path is refused: `require` would search `@INC`.
    ///
    /// This is the whole reason the mutation names a path rather than the
    /// key. Verified against real Perl while writing this:
    /// `perl -e 'use lib "b"; require "a/App/Core.pm"'` reports
    /// `Can't locate a/App/Core.pm in @INC`, so a relative path is
    /// searched exactly like a bare key and could execute a same-named
    /// module from elsewhere on `@INC`.
    #[test]
    fn relative_runtime_paths_are_refused() -> TestResult {
        for relative in [
            "App/Core.pm",
            "lib/App/Core.pm",
            "./lib/App/Core.pm",
            "../lib/App/Core.pm",
            "C:lib/App/Core.pm", // drive-relative, not drive-rooted
        ] {
            assert!(!runtime_path_is_safe(relative), "{relative:?} must be refused");
            assert!(!path_is_absolute(relative));
        }
        for absolute in [
            "/ws/lib/App/Core.pm",
            "C:\\ws\\lib\\App\\Core.pm",
            "C:/ws/lib/App/Core.pm",
            "\\\\server\\share\\App\\Core.pm",
        ] {
            assert!(path_is_absolute(absolute), "{absolute:?} must be absolute");
            assert!(runtime_path_is_safe(absolute), "{absolute:?} must be accepted");
        }

        // End to end: a relative subject refuses without touching the
        // debuggee, rather than reaching a `require` that would search.
        let relative = SubjectCandidate {
            resolved_runtime_path: "lib/App/Core.pm".to_string(),
            ..candidate()
        }
        .bind()
        .map_err(|_| "candidate must bind")?;
        assert_eq!(
            plan_commands(&relative, ReloadMechanism::IncDeletionAndRequire),
            Err(CommandPlanError::UnsafeRuntimePath)
        );
        let plan = plan_reload(&relative, &admitted_observation())
            .map_err(|_| "admission is observation-driven")?;
        let mut clock = RuntimeModuleGenerationClock::new();
        let mut channel = ScriptedChannel::happy();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(execution.phase_reached, ReloadTransactionPhase::Admission);
        assert!(channel.issued_mutations.is_empty());
        Ok(())
    }

    /// A path with surrounding whitespace round-trips exactly.
    ///
    /// The observation carries the path hex-encoded precisely so this
    /// works: a whitespace-delimited frame would have the parser trim the
    /// value, mismatch the bound subject, and refuse every reload of such
    /// a module.
    #[test]
    fn whitespace_bearing_paths_round_trip() -> TestResult {
        let odd = "/ws/ odd lib /App/Core.pm ";
        assert!(runtime_path_is_safe(odd));
        assert_eq!(
            parse_registration(
                &[format!("{} present {}", mark(READBACK_MARKER), perl_hex(odd))],
                &mark(READBACK_MARKER)
            ),
            Some(RegistrationObservation { present: true, path: odd.to_string() }),
            "the trailing space must survive the frame"
        );

        let subject = SubjectCandidate { resolved_runtime_path: odd.to_string(), ..candidate() }
            .bind()
            .map_err(|_| "subject must bind")?;
        let plan = plan_reload(&subject, &admitted_observation()).map_err(|_| "must admit")?;
        let mut channel = ScriptedChannel::new(
            vec![
                ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, odd),
                ScriptedChannel::ok(&mark(READBACK_MARKER), true, odd),
            ],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let mut clock = RuntimeModuleGenerationClock::new();
        let execution =
            execute_reload(&plan, ReloadMechanism::IncDeletionAndRequire, &mut channel, &mut clock);
        assert_eq!(execution.outcome, LoadedModuleReloadOutcome::Reloaded);
        Ok(())
    }

    /// An unreadable hex payload is not an observation.
    #[test]
    fn malformed_hex_payloads_are_refused() {
        let marker = mark(READBACK_MARKER);
        for payload in ["zz", "abc", "", "6g"] {
            assert_eq!(
                parse_registration(&[format!("{marker} present {payload}")], &marker),
                None,
                "{payload:?} must not decode"
            );
        }
        assert_eq!(decode_perl_hex(&perl_hex("/ws/x.pm")).as_deref(), Some("/ws/x.pm"));
    }

    /// A built command plan is read-only.
    ///
    /// `plan_commands` owns command derivation, which is what makes the
    /// `%INC` allowlist meaningful. If a consumer could swap the vectors
    /// after construction, the guard would be decorative.
    #[test]
    fn command_plans_are_not_mutable_after_construction() -> TestResult {
        let subject = candidate().bind().map_err(|_| "bind")?;
        let commands = plan_commands(&subject, ReloadMechanism::IncDeletionAndRequire)
            .map_err(|_| "safe key must plan")?;
        // Accessors hand out shared slices; there is no `&mut` path and no
        // public field to assign through.
        let mutation: &[String] = commands.mutation();
        assert_eq!(mutation.len(), 1);
        assert!(commands.preflight().first().is_some_and(|c| c.contains(PREFLIGHT_MARKER)));
        assert!(commands.read_back().first().is_some_and(|c| c.contains(READBACK_MARKER)));
        Ok(())
    }

    /// A channel that runs out of scripted settlements yields the
    /// conservative answer, not a silent success.
    ///
    /// The sweep and the race tests each supply exactly as many
    /// settlements as the executor consumes, so this fallback is never
    /// reached there. It is a real safety net for future scenarios, so it
    /// is pinned directly rather than left as an assumption.
    #[test]
    fn exhausted_script_settles_conservatively() -> TestResult {
        let mut channel = ScriptedChannel::new(
            // Preflight consumes the only queued settlement; the read-back
            // call past the end falls back.
            vec![ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH)],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        assert_eq!(
            channel.run_readonly(&[]),
            ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH)
        );
        assert_eq!(
            channel.run_readonly(&[]),
            ChannelSettlement::Unsettled(UnsettledKind::Timeout),
            "an exhausted script must fall back to the conservative settlement"
        );

        // And through the executor: an exhausted read-back after a
        // successful mutation is possibly applied, never Reloaded.
        let mut channel = ScriptedChannel::new(
            vec![ScriptedChannel::ok(&mark(PREFLIGHT_MARKER), true, PATH)],
            ChannelSettlement::Acknowledged(vec![format!("{} 1", mark(MUTATION_MARKER))]),
        );
        let execution = run(&mut channel)?;
        assert_eq!(
            execution.outcome,
            LoadedModuleReloadOutcome::IndeterminatePossiblyApplied {
                phase: ReloadTransactionPhase::RuntimeAcknowledgementReadBack,
                cause: IndeterminateCause::TimeoutAfterMutationBegan,
            }
        );
        assert!(execution.mutation_issued);
        assert!(execution.generation.advanced());
        Ok(())
    }

    /// perl5db's decorated prompt does not corrupt marker fields.
    ///
    /// Real captured shape: an underline/bold CSI run and a shift-in
    /// control byte immediately precede the payload on the prompt line.
    #[test]
    fn ansi_decorated_frames_parse_cleanly() {
        let decorated = format!(
            "\u{1b}[4m  DB<2> \u{1b}[24m\u{1b}[1m\u{1b}[m\u{0f}{} present {}",
            mark(READBACK_MARKER),
            perl_hex(PATH)
        );
        assert_eq!(
            parse_registration(&[decorated], &mark(READBACK_MARKER)),
            Some(RegistrationObservation { present: true, path: PATH.to_string() })
        );
        let decorated_ack = format!("\u{1b}[4m  DB<3> \u{1b}[24m\u{0f}{} 1", mark(MUTATION_MARKER));
        assert_eq!(parse_mutation_ack(&[decorated_ack], &mark(MUTATION_MARKER)), Some(true));
        // Sanitizing must not invent a marker where there is none.
        assert_eq!(
            parse_mutation_ack(
                &["\u{1b}[4m  DB<4> \u{1b}[24m".to_string()],
                &mark(MUTATION_MARKER)
            ),
            None
        );
    }
}
