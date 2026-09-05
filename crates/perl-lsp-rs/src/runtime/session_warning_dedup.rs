//! Typed, bounded session-warning dedup state (#9769).
//!
//! The server occasionally suppresses a repeated user-facing
//! `window/showMessage` warning so a persistent condition (a missing
//! `perlcritic` binary, an invalid editor setting value, an AI authentication
//! failure) does not spam the editor on every diagnostic cycle. Before #9769
//! each family kept an unbounded `HashSet<String>` of raw key strings for the
//! whole server session, so a long-lived or adversarial client/workspace
//! could grow retained memory without limit even though only notification
//! suppression was at stake.
//!
//! This module replaces those sets with one typed store whose only authority
//! is *whether an already-emitted warning should be suppressed for the same
//! reviewed subject*. It never influences configuration, diagnostics,
//! provider semantics, readiness, trust, or repair decisions.
//!
//! Identity contract:
//!
//! - identities are a fixed-size `(code, subject tag, subject fingerprint)`
//!   triple -- no raw setting value, absolute path, error body, API key, or
//!   other secret-bearing payload is ever retained;
//! - the fingerprint is the repository's shared deterministic FNV-1a 64-bit
//!   hash ([`perl_lsp_rs_core::hashing::fnv1a64`]), so equality is
//!   deterministic and process-safe (no pointer or per-process random
//!   identity);
//! - a subject the store cannot represent through a bounded tag fails to
//!   [`SessionWarningDecision::EmitWithoutRetaining`] rather than storing an
//!   arbitrary string.
//!
//! Bound semantics (reviewed saturation): each family retains at most
//! [`PER_FAMILY_ENTRY_CAP`] identities of [`std::mem::size_of::<SessionWarningIdentity>`]
//! bytes each. When a family is saturated, a *new* identity is still emitted
//! -- it is simply not retained -- so a full table can never silently drop an
//! actionable warning, and retained growth stops at the hard bound. There is
//! deliberately no eviction: within the cap, suppression stays stable for the
//! whole session, which preserves the previous warn-once wording intent.
//!
//! Lifecycle: the critic family clears on critic configuration transitions
//! and the AI-backend family clears on configuration notifications, both at
//! their existing call sites. Correctness never depends on these clears
//! because the hard per-family cap remains load-bearing. The store owns no
//! global state, so server shutdown releases everything by drop.
//!
//! Registry (#9155/#7931): classification
//! `provider_or_presentation`/operational-session, semantic authority
//! `none`, lifecycle `server session + applicable root/config/backend
//! subject`, persistence `never`, hard bound `explicit`, privacy `no raw
//! secret/value/path payload`.

use std::collections::HashSet;

use parking_lot::Mutex;

/// Hard cap on retained identities per warning family.
///
/// Reviewed limit (#9769): a legitimate session presents only a handful of
/// distinct warning subjects per family (a few critic profile paths, a few
/// invalid setting values, one auth failure), so 32 leaves generous headroom
/// while bounding retained state to `32 * size_of::<SessionWarningIdentity>()`
/// bytes per family regardless of client behavior.
pub(crate) const PER_FAMILY_ENTRY_CAP: usize = 32;

/// One governed session-warning family (#9769).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionWarningFamily {
    /// Workspace-scoped Perl::Critic warnings (binary/profile/execution).
    #[cfg(not(target_arch = "wasm32"))]
    Critic,
    /// Invalid enum values from editor-provided settings.
    ClientSetting,
    /// AI backend warnings (authentication failures).
    AiBackend,
}

/// Stable internal reason/category for a session warning.
///
/// The code plus the bounded subject below form the complete dedup identity;
/// warning wording and remediation stay owned by the domain call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SessionWarningCode {
    /// `perlcritic` binary missing from PATH (no variable subject).
    #[cfg(not(target_arch = "wasm32"))]
    CriticMissingBinary,
    /// Configured Perl::Critic profile path does not exist
    /// (subject: configured profile string fingerprint).
    #[cfg(not(target_arch = "wasm32"))]
    CriticMissingProfile,
    /// Perl::Critic execution failed (subject: error text fingerprint).
    #[cfg(not(target_arch = "wasm32"))]
    CriticExecutionFailed,
    /// Invalid enum value in an editor-provided setting
    /// (subject: setting tag + value type + normalized value fingerprint).
    ClientSettingInvalidValue,
    /// AI inline-completion backend authentication failed
    /// (no variable subject).
    AiBackendAuthFailure,
}

/// Closed set of static dimensions that distinguish identities inside one
/// family. Editor-provided setting names come from a fixed configuration
/// surface; representing them as a bounded tag keeps an unknown name out of
/// retained state instead of retaining the raw string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SessionWarningSubjectTag {
    /// The code alone determines suppression (no variable subject).
    None,
    /// `critic.engine` client setting.
    ClientCriticEngine,
    /// `critic.profile` client setting.
    ClientCriticProfile,
    /// `formatting.engine` client setting.
    ClientFormattingEngine,
}

impl SessionWarningSubjectTag {
    /// Map an editor-provided setting name to its bounded tag.
    ///
    /// Returns `None` for any name outside the reviewed configuration
    /// surface; the caller then follows the bounded emit-without-retaining
    /// path instead of retaining an arbitrary string.
    pub(crate) fn from_client_setting(setting: &str) -> Option<Self> {
        match setting {
            "critic.engine" => Some(Self::ClientCriticEngine),
            "critic.profile" => Some(Self::ClientCriticProfile),
            "formatting.engine" => Some(Self::ClientFormattingEngine),
            _ => None,
        }
    }
}

/// Fixed-size dedup identity: code + bounded subject tag + subject
/// fingerprint. Retains no variable-length payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SessionWarningIdentity {
    code: SessionWarningCode,
    tag: SessionWarningSubjectTag,
    fingerprint: u64,
}

impl SessionWarningIdentity {
    /// Identity for a warning whose code alone determines suppression.
    pub(crate) const fn subjectless(code: SessionWarningCode) -> Self {
        Self { code, tag: SessionWarningSubjectTag::None, fingerprint: 0 }
    }

    /// Identity for a warning with one variable client/environment-controlled
    /// subject string. Only the deterministic 64-bit fingerprint is retained.
    pub(crate) fn fingerprinted(
        code: SessionWarningCode,
        tag: SessionWarningSubjectTag,
        subject: &str,
    ) -> Self {
        Self { code, tag, fingerprint: perl_lsp_rs_core::hashing::fnv1a64(subject.as_bytes()) }
    }
}

/// Exact outcome of consulting the dedup store for one warning emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionWarningDecision {
    /// First time this identity is seen in the family: emit and retain.
    EmitFirst,
    /// The identity was already retained: suppress the repeat.
    Suppress,
    /// The family is saturated: emit, but do not retain the identity.
    /// Saturation must never silently drop an actionable warning.
    EmitWithoutRetaining,
}

/// Per-family retained identities plus pressure counters.
#[derive(Default)]
struct FamilyState {
    seen: HashSet<SessionWarningIdentity>,
    inserted: u64,
    suppressed: u64,
    emitted_without_retaining: u64,
    cleared_by_lifecycle: u64,
    high_water_entries: usize,
}

impl FamilyState {
    fn note(&mut self, identity: SessionWarningIdentity) -> SessionWarningDecision {
        if self.seen.contains(&identity) {
            self.suppressed += 1;
            return SessionWarningDecision::Suppress;
        }
        if self.seen.len() >= PER_FAMILY_ENTRY_CAP {
            self.emitted_without_retaining += 1;
            return SessionWarningDecision::EmitWithoutRetaining;
        }
        self.seen.insert(identity);
        self.inserted += 1;
        self.high_water_entries = self.high_water_entries.max(self.seen.len());
        SessionWarningDecision::EmitFirst
    }

    fn forget(&mut self, identity: &SessionWarningIdentity) {
        self.seen.remove(identity);
    }

    fn clear_for_lifecycle(&mut self) {
        self.cleared_by_lifecycle += u64::try_from(self.seen.len()).unwrap_or(u64::MAX);
        self.seen.clear();
    }

    /// Record an emit-without-retaining event for an unrepresentable subject.
    fn note_unrepresentable(&mut self) -> SessionWarningDecision {
        self.emitted_without_retaining += 1;
        SessionWarningDecision::EmitWithoutRetaining
    }
}

/// One family's locked state.
#[derive(Default)]
struct FamilyStore {
    state: Mutex<FamilyState>,
}

impl FamilyStore {
    fn note(&self, identity: SessionWarningIdentity) -> SessionWarningDecision {
        self.state.lock().note(identity)
    }

    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    fn forget(&self, identity: &SessionWarningIdentity) {
        self.state.lock().forget(identity);
    }

    fn clear_for_lifecycle(&self) {
        self.state.lock().clear_for_lifecycle();
    }

    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    fn counters(&self) -> SessionWarningFamilyCounters {
        let state = self.state.lock();
        SessionWarningFamilyCounters {
            entries: state.seen.len(),
            high_water_entries: state.high_water_entries,
            inserted: state.inserted,
            suppressed: state.suppressed,
            emitted_without_retaining: state.emitted_without_retaining,
            cleared_by_lifecycle: state.cleared_by_lifecycle,
        }
    }
}

/// Session-scoped warning-dedup store for the three governed families.
///
/// Presentation/operational state only (#9769): classification
/// `provider_or_presentation`, semantic authority none, persistence never.
#[derive(Default)]
pub(crate) struct SessionWarningDedupStore {
    #[cfg(not(target_arch = "wasm32"))]
    critic: FamilyStore,
    client_setting: FamilyStore,
    ai_backend: FamilyStore,
}

impl SessionWarningDedupStore {
    fn family(&self, family: SessionWarningFamily) -> &FamilyStore {
        match family {
            #[cfg(not(target_arch = "wasm32"))]
            SessionWarningFamily::Critic => &self.critic,
            SessionWarningFamily::ClientSetting => &self.client_setting,
            SessionWarningFamily::AiBackend => &self.ai_backend,
        }
    }

    /// Consult the store about one warning emission.
    pub(crate) fn note(
        &self,
        family: SessionWarningFamily,
        identity: SessionWarningIdentity,
    ) -> SessionWarningDecision {
        self.family(family).note(identity)
    }

    /// Reverse a retention after the outbound send failed, so a warning the
    /// client never received can be retried on the next occurrence.
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub(crate) fn forget(&self, family: SessionWarningFamily, identity: &SessionWarningIdentity) {
        self.family(family).forget(identity);
    }

    /// Decide and emit under one family-lock hold, rolling the retention back
    /// when `emit` reports the warning was not delivered.
    ///
    /// This preserves the pre-#9769 `notify_ai_auth_failure` atomicity: a
    /// concurrent caller of the same family either observes the retained
    /// identity only after this send already succeeded, or is itself the one
    /// who emits. Without the lock held across send + rollback, caller A
    /// could retain the identity, caller B could suppress against it, and
    /// A's failed send could then release it -- leaving both calls with no
    /// delivered warning until the next occurrence.
    pub(crate) fn emit_once_with(
        &self,
        family: SessionWarningFamily,
        identity: SessionWarningIdentity,
        emit: impl FnOnce() -> bool,
    ) -> SessionWarningDecision {
        let mut state = self.family(family).state.lock();
        let decision = state.note(identity);
        if matches!(
            decision,
            SessionWarningDecision::EmitFirst | SessionWarningDecision::EmitWithoutRetaining
        ) && !emit()
        {
            state.forget(&identity);
        }
        decision
    }

    /// Drop every retained identity of one family at its lifecycle boundary.
    /// Identities of other families are untouched.
    pub(crate) fn clear_family(&self, family: SessionWarningFamily) {
        self.family(family).clear_for_lifecycle();
    }

    /// Consult the store for an invalid editor-provided setting value.
    ///
    /// Unknown setting names (no bounded tag) follow the bounded
    /// emit-without-retaining path: the warning is still emitted, but nothing
    /// is retained for it.
    pub(crate) fn note_client_setting(
        &self,
        setting: &str,
        value_type: &str,
        normalized_value: &str,
    ) -> SessionWarningDecision {
        let Some(tag) = SessionWarningSubjectTag::from_client_setting(setting) else {
            return self.client_setting.state.lock().note_unrepresentable();
        };
        let mut subject = String::with_capacity(value_type.len() + normalized_value.len() + 1);
        subject.push_str(value_type);
        subject.push('\u{0}');
        subject.push_str(normalized_value);
        self.note(
            SessionWarningFamily::ClientSetting,
            SessionWarningIdentity::fingerprinted(
                SessionWarningCode::ClientSettingInvalidValue,
                tag,
                &subject,
            ),
        )
    }
}

/// Pressure/bound counters for one warning family (#9183 pressure row).
///
/// Missing instrumentation is unknown, not zero: every counter starts at zero
/// and only moves when its event happens.
#[cfg(any(test, feature = "expose_lsp_test_api"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionWarningFamilyCounters {
    /// Identities currently retained by the family.
    pub entries: usize,
    /// Largest `entries` value reached so far (fixed-weight high-water).
    pub high_water_entries: usize,
    /// Identities retained for the first time.
    pub inserted: u64,
    /// Repeats suppressed because the identity was already retained.
    pub suppressed: u64,
    /// Emissions that could not retain their identity (saturation or an
    /// unrepresentable subject).
    pub emitted_without_retaining: u64,
    /// Identities dropped by an explicit lifecycle clear of this family.
    pub cleared_by_lifecycle: u64,
}

/// Point-in-time counters for the whole session-warning dedup store.
#[cfg(any(test, feature = "expose_lsp_test_api"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionWarningDedupSnapshot {
    /// Critic family counters (absent on WASM targets where the critic
    /// pipeline does not exist).
    #[cfg(not(target_arch = "wasm32"))]
    pub critic: SessionWarningFamilyCounters,
    /// Client-setting family counters.
    pub client_setting: SessionWarningFamilyCounters,
    /// AI-backend family counters.
    pub ai_backend: SessionWarningFamilyCounters,
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
impl SessionWarningDedupStore {
    /// Test/pressure observation of every family's counters.
    pub(crate) fn snapshot(&self) -> SessionWarningDedupSnapshot {
        SessionWarningDedupSnapshot {
            #[cfg(not(target_arch = "wasm32"))]
            critic: self.critic.counters(),
            client_setting: self.client_setting.counters(),
            ai_backend: self.ai_backend.counters(),
        }
    }
}

impl super::LspServer {
    /// Pressure/bound counters for the session-warning dedup store (#9183).
    #[cfg(any(test, feature = "expose_lsp_test_api"))]
    pub fn session_warning_dedup_snapshot(&self) -> SessionWarningDedupSnapshot {
        self.session_warning_dedup.snapshot()
    }
}
