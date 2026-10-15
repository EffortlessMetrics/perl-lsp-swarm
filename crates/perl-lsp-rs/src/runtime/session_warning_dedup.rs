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
    /// Rejected client `includePaths` entries (subject: the entry plus the
    /// bounded reason kind, fingerprinted). The same rejected entry is
    /// re-validated on several channels (`didChangeConfiguration`,
    /// `initializationOptions` replay, folder effective-config rebuild), so
    /// without a family the user would see one popup per channel per
    /// notification (#17164).
    ClientIncludePath,
    /// AI backend warnings (authentication failures).
    AiBackend,
    /// A workspace `.perl-lsp.toml` could not be loaded or applied
    /// (subject: the offending config path, fingerprinted).
    ///
    /// The remedy is "fix the file and reload the window", so a broken profile
    /// is a persistent condition for the whole session: re-emitting the same
    /// warning on every `didOpen` trains the user to dismiss the one message
    /// that matters (#16548).
    #[cfg(not(target_arch = "wasm32"))]
    ProjectConfig,
}

/// Stable internal reason/category for a session warning.
///
/// The code plus the bounded subject below form the complete dedup identity;
/// warning wording and remediation stay owned by the domain call sites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SessionWarningCode {
    /// Invalid enum value in an editor-provided setting
    /// (subject: setting tag + value type + normalized value fingerprint).
    ClientSettingInvalidValue,
    /// A client-supplied `includePaths` entry was rejected
    /// (subject: the entry plus the bounded reason kind, fingerprinted).
    ClientIncludePathRejected,
    /// AI inline-completion backend authentication failed
    /// (no variable subject).
    AiBackendAuthFailure,
    /// A workspace `.perl-lsp.toml` failed to load or apply
    /// (subject: the offending config path fingerprint).
    ///
    /// The message body is deliberately **not** part of the identity: the
    /// warning wording stays owned by the domain call site, and two different
    /// parse errors in the same file are the same condition for suppression
    /// purposes - the user fixes the file, not the sentence.
    #[cfg(not(target_arch = "wasm32"))]
    ProjectConfigInvalid,
    /// A loaded `.perl-lsp.toml` carried an unusable `[perl].version`
    /// (subject: the offending config path fingerprint).
    ///
    /// A separate kind from [`SessionWarningCode::ProjectConfigInvalid`] so a
    /// load-failure warning and an invalid-version warning for the same file
    /// remain distinct: the user can fix one and then trip the other, and
    /// each names a different remedy (PR #16566 review).
    #[cfg(not(target_arch = "wasm32"))]
    ProjectConfigVersionInvalid,
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

/// Session-scoped warning-dedup store for the governed families.
///
/// Presentation/operational state only (#9769): classification
/// `provider_or_presentation`, semantic authority none, persistence never.
#[derive(Default)]
pub(crate) struct SessionWarningDedupStore {
    #[cfg(not(target_arch = "wasm32"))]
    critic: FamilyStore,
    client_setting: FamilyStore,
    client_include_path: FamilyStore,
    ai_backend: FamilyStore,
    #[cfg(not(target_arch = "wasm32"))]
    project_config: FamilyStore,
}

impl SessionWarningDedupStore {
    fn family(&self, family: SessionWarningFamily) -> &FamilyStore {
        match family {
            #[cfg(not(target_arch = "wasm32"))]
            SessionWarningFamily::Critic => &self.critic,
            SessionWarningFamily::ClientSetting => &self.client_setting,
            SessionWarningFamily::ClientIncludePath => &self.client_include_path,
            SessionWarningFamily::AiBackend => &self.ai_backend,
            #[cfg(not(target_arch = "wasm32"))]
            SessionWarningFamily::ProjectConfig => &self.project_config,
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

    /// Decide, emit, and roll back on failed delivery for one rejected
    /// client `includePaths` entry warning (#17164).
    ///
    /// The same rejected entry is re-validated on every channel that applies
    /// client settings (`didChangeConfiguration`, `initializationOptions`
    /// replay, folder effective-config rebuild), so suppression is keyed on
    /// the **entry plus the bounded reason kind** (`reason_key`, from
    /// [`perl_lsp_rs_core::config::RejectedClientIncludePathReason::dedup_key`]),
    /// fingerprinted: one rejected entry warns once per session across all
    /// channels, while a different entry — or the same entry rejected for a
    /// different reason kind — still warns. Detail payloads (workspace-escape
    /// details, unauthorized source labels) are excluded from the identity by
    /// the caller's key, and only the deterministic fingerprint of the subject
    /// is retained.
    ///
    /// Delivery failures roll the retention back via [`Self::emit_once_with`]:
    /// a warning the client never received must be eligible to re-fire on the
    /// next occurrence.
    pub(crate) fn emit_client_include_path_warning(
        &self,
        entry: &str,
        reason_key: &str,
        emit: impl FnOnce() -> bool,
    ) -> SessionWarningDecision {
        let mut subject = String::with_capacity(entry.len() + reason_key.len() + 1);
        subject.push_str(entry);
        subject.push('\u{0}');
        subject.push_str(reason_key);
        self.emit_once_with(
            SessionWarningFamily::ClientIncludePath,
            SessionWarningIdentity::fingerprinted(
                SessionWarningCode::ClientIncludePathRejected,
                SessionWarningSubjectTag::None,
                &subject,
            ),
            emit,
        )
    }

    /// Decide, emit, and roll back on failed delivery for one
    /// `.perl-lsp.toml` warning.
    ///
    /// Suppression is keyed on the **config path** alone, fingerprinted: the
    /// same file warns once per session however many times it is re-read, and
    /// a second broken file still warns because its path differs. The error
    /// body is not part of the identity, so a user who fixes one error and
    /// trips another in the same file is not spammed a second time — the
    /// remedy (fix the file, reload the window) is identical either way.
    ///
    /// The caller supplies the warning kind (`ProjectConfigInvalid` for a
    /// load/apply failure, `ProjectConfigVersionInvalid` for an unusable
    /// `[perl].version`) and the **selected** config path the warning is
    /// about — the file discovery actually chose, not the search root or a
    /// display name, so two different files never collide into one identity
    /// and one file never splits into two (PR #16566 review).
    ///
    /// Delivery failures roll the retention back via [`Self::emit_once_with`]:
    /// a warning the client never received must be eligible to re-fire on the
    /// next occurrence instead of being suppressed forever.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn emit_project_config_warning(
        &self,
        code: SessionWarningCode,
        config_path: &str,
        emit: impl FnOnce() -> bool,
    ) -> SessionWarningDecision {
        self.emit_once_with(
            SessionWarningFamily::ProjectConfig,
            SessionWarningIdentity::fingerprinted(
                code,
                SessionWarningSubjectTag::None,
                config_path,
            ),
            emit,
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
    /// Rejected client include-path entry family counters.
    pub client_include_path: SessionWarningFamilyCounters,
    /// AI-backend family counters.
    pub ai_backend: SessionWarningFamilyCounters,
    /// Project-config family counters (absent on WASM targets, where the
    /// project-config loaders do not exist).
    #[cfg(not(target_arch = "wasm32"))]
    pub project_config: SessionWarningFamilyCounters,
}

#[cfg(any(test, feature = "expose_lsp_test_api"))]
impl SessionWarningDedupStore {
    /// Test/pressure observation of every family's counters.
    pub(crate) fn snapshot(&self) -> SessionWarningDedupSnapshot {
        SessionWarningDedupSnapshot {
            #[cfg(not(target_arch = "wasm32"))]
            critic: self.critic.counters(),
            client_setting: self.client_setting.counters(),
            client_include_path: self.client_include_path.counters(),
            ai_backend: self.ai_backend.counters(),
            #[cfg(not(target_arch = "wasm32"))]
            project_config: self.project_config.counters(),
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
