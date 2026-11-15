//! Connection/client-session owner for one LSP connection (#8386, "C01").
//!
//! Before this component, lifecycle flags, negotiated capabilities, trace
//! level, progress tokens, cancellation markers, and the live reverse-request
//! pending map lived as sibling fields on `LspServer`. Shutdown cleared a
//! subset of them from two call sites (`dispatch/lifecycle.rs` and
//! `dispatch/routing.rs`) and left progress tokens and capability identity
//! retained until process exit.
//!
//! `ClientSession` is the single owner of that connection-scoped state. It
//! exposes two reset points:
//!
//! - [`ClientSession::begin_shutdown`]: first-caller-wins drain of every
//!   session-owned handle, plus capability/session identity invalidation.
//!   The same connection remains terminal (`initialize` flags stay set).
//! - [`ClientSession::replace_connection`]: full replacement-connection
//!   reset, including lifecycle flags and a new session generation. Production
//!   serving does not reconnect on one `LspServer` instance today; this is
//!   the named reset point the negative controls exercise.
//!
//! ## Hard boundary
//!
//! This component owns connection/client state only. It does **not**:
//!
//! - create a second reverse-request identity registry (#7007's
//!   `ServerRequestRegistry` remains the correlation type; inbound wiring is
//!   #7010);
//! - own document, workspace, analysis, or application-worker state;
//! - ratchet handlers off `&LspServer` (#8389);
//! - change framework/application dispatch (#7388).
//!
//! Resolve-session authenticator teardown stays on the ClientTransport
//! `#8342` path and is invoked by the shutdown dispatcher after this
//! component drains session-owned handles.

use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use parking_lot::{Mutex, MutexGuard};
use perl_lsp_rs_core::governance::FeatureProfile;
use serde_json::Value;

use super::lifecycle::position_encoding::PositionEncodingSessionContext;
use super::lifecycle::root_input::InitialRootInput;
use super::lifecycle::session_contract::AcceptedTextSyncSession;
use super::session_warning_dedup::SessionWarningDedupStore;
use super::types::{PendingWorkspaceConfigurationRequest, ServerRequestId};
use crate::protocol::JsonRpcId;
use crate::protocol::capabilities::AdvertisedFeatures;
use crate::state::{ClientCapabilities, ServerConfig};

/// Outcome of [`ClientSession::begin_shutdown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShutdownAdmission {
    /// This caller won the first-shutdown race and drained session state.
    First,
    /// Shutdown had already been admitted; no second drain ran.
    AlreadyShutdown,
}

/// Outcome of installing a session-owned progress token.
///
/// Token installation is serialized with [`ClientSession::begin_shutdown`]
/// through the `progress_tokens` mutex: drain holds `outbound_enqueue`, sets
/// `shutdown_received`, then clears under `progress_tokens`, so a late
/// producer either refuses or inserts before drain and is then cleared. An
/// independent atomic precheck before outbound I/O is not the enqueue gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgressTokenInstall {
    Installed,
    Shutdown,
    AlreadyExists,
}

/// Connection-scoped client/session state for one LSP connection.
pub(crate) struct ClientSession {
    /// Monotonic session identity. Starts at 1; bumps on shutdown and
    /// replacement so a capability/progress view captured earlier cannot
    /// authorize later work.
    generation: AtomicU64,
    /// Advertised-feature defaults restored on [`Self::replace_connection`].
    default_advertised_features: AdvertisedFeatures,
    default_advertised_feature_ids: Vec<&'static str>,
    pub(crate) initialize_requested: AtomicBool,
    pub(crate) initialized: AtomicBool,
    pub(crate) shutdown_received: Arc<AtomicBool>,
    pub(crate) pending_startup_log: Arc<Mutex<Option<String>>>,
    pub(crate) config: Arc<Mutex<ServerConfig>>,
    pub(crate) client_capabilities: Mutex<ClientCapabilities>,
    pub(crate) initial_root_input: Mutex<Option<InitialRootInput>>,
    pub(crate) cancelled: Arc<Mutex<HashSet<JsonRpcId>>>,
    pub(crate) pending_request_ids: Arc<Mutex<HashSet<JsonRpcId>>>,
    pub(crate) advertised_features: Mutex<AdvertisedFeatures>,
    pub(crate) advertised_feature_ids: Mutex<Vec<&'static str>>,
    pub(crate) text_sync_session: Mutex<Option<AcceptedTextSyncSession>>,
    pub(crate) position_encoding_session_context: Mutex<Option<PositionEncodingSessionContext>>,
    pub(crate) client_supports_pull_diags: Arc<AtomicBool>,
    pub(crate) initialization_options_perl_settings: Arc<Mutex<Option<Value>>>,
    pub(crate) next_request_id: Arc<AtomicI32>,
    pub(crate) pending_workspace_configuration_requests:
        Arc<Mutex<HashMap<ServerRequestId, PendingWorkspaceConfigurationRequest>>>,
    pub(crate) progress_tokens: Arc<Mutex<HashSet<String>>>,
    pub(crate) progress_token_to_request: Arc<Mutex<HashMap<String, JsonRpcId>>>,
    /// Serializes reverse-request and `$/progress` enqueue with shutdown.
    /// Held across the live-session recheck and the sink send so a producer
    /// that already passed `shutdown_received` still cannot emit after drain.
    /// This is not `progress_tokens`; do not hold that map across I/O.
    outbound_enqueue: Mutex<()>,
    pub(crate) trace_level: Arc<Mutex<String>>,
    pub(crate) root_undetected_shown: Arc<AtomicBool>,
    /// Once-per-session core-module goto-definition notice (#16551).
    pub(crate) core_module_notice_shown: Arc<AtomicBool>,
    pub(crate) session_warning_dedup: SessionWarningDedupStore,
}

impl ClientSession {
    pub(crate) fn new(feature_profile: FeatureProfile) -> Self {
        let default_advertised_features = feature_profile.advertised_features();
        let default_advertised_feature_ids = feature_profile.build_flags().to_feature_ids();
        Self {
            generation: AtomicU64::new(1),
            default_advertised_features: default_advertised_features.clone(),
            default_advertised_feature_ids: default_advertised_feature_ids.clone(),
            initialize_requested: AtomicBool::new(false),
            initialized: AtomicBool::new(false),
            shutdown_received: Arc::new(AtomicBool::new(false)),
            pending_startup_log: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(ServerConfig::default())),
            client_capabilities: Mutex::new(ClientCapabilities::default()),
            initial_root_input: Mutex::new(None),
            cancelled: Arc::new(Mutex::new(HashSet::new())),
            pending_request_ids: Arc::new(Mutex::new(HashSet::new())),
            advertised_features: Mutex::new(default_advertised_features),
            advertised_feature_ids: Mutex::new(default_advertised_feature_ids),
            text_sync_session: Mutex::new(None),
            position_encoding_session_context: Mutex::new(None),
            client_supports_pull_diags: Arc::new(AtomicBool::new(false)),
            initialization_options_perl_settings: Arc::new(Mutex::new(None)),
            next_request_id: Arc::new(AtomicI32::new(1)),
            pending_workspace_configuration_requests: Arc::new(Mutex::new(HashMap::new())),
            progress_tokens: Arc::new(Mutex::new(HashSet::new())),
            progress_token_to_request: Arc::new(Mutex::new(HashMap::new())),
            outbound_enqueue: Mutex::new(()),
            trace_level: Arc::new(Mutex::new("off".to_string())),
            root_undetected_shown: Arc::new(AtomicBool::new(false)),
            core_module_notice_shown: Arc::new(AtomicBool::new(false)),
            session_warning_dedup: SessionWarningDedupStore::default(),
        }
    }

    /// Current session generation. A view captured at generation `n` is
    /// unauthorized once generation advances.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// True only while `observed` still names this live session.
    #[cfg(test)]
    pub(crate) fn authorize_generation(&self, observed: u64) -> bool {
        observed != 0 && observed == self.generation()
    }

    /// Admit shutdown exactly once and drain every session-owned handle.
    ///
    /// A second caller observes [`ShutdownAdmission::AlreadyShutdown`] and
    /// must not re-run drain. Capability identity is invalidated by bumping
    /// generation and resetting negotiated client facts; lifecycle flags stay
    /// terminal for this connection.
    pub(crate) fn begin_shutdown(&self) -> ShutdownAdmission {
        // Take the enqueue fence before flipping shutdown so an in-flight
        // send that already holds this lock settles as pre-shutdown, and a
        // producer paused after the atomic precheck still observes the flag
        // at enqueue.
        let _enqueue = self.outbound_enqueue.lock();
        if self.shutdown_received.swap(true, Ordering::AcqRel) {
            return ShutdownAdmission::AlreadyShutdown;
        }
        self.invalidate_identity_and_drain();
        ShutdownAdmission::First
    }

    /// Full replacement-connection reset: drain, restore constructor defaults,
    /// and mint a new generation so prior identity cannot authorize.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "policy:#8386: replacement-connection reset is the session lifecycle point; production reconnect is not yet a serving path"
        )
    )]
    pub(crate) fn replace_connection(&self) {
        let _enqueue = self.outbound_enqueue.lock();
        let _ = self.shutdown_received.swap(true, Ordering::AcqRel);
        self.invalidate_identity_and_drain();
        self.initialize_requested.store(false, Ordering::Release);
        self.initialized.store(false, Ordering::Release);
        self.shutdown_received.store(false, Ordering::Release);
        *self.advertised_features.lock() = self.default_advertised_features.clone();
        *self.advertised_feature_ids.lock() = self.default_advertised_feature_ids.clone();
        *self.config.lock() = ServerConfig::default();
        *self.trace_level.lock() = "off".to_string();
        self.next_request_id.store(1, Ordering::Release);
        self.root_undetected_shown.store(false, Ordering::Release);
        self.core_module_notice_shown.store(false, Ordering::Release);
        self.client_supports_pull_diags.store(false, Ordering::Release);
    }

    /// Admit one reverse-request or `$/progress` enqueue if this session is
    /// still live. Hold the returned guard across the sink send. Shutdown
    /// takes the same lock before draining, so this is the settlement fence;
    /// `progress_tokens` stays out of that I/O.
    pub(crate) fn admit_outbound_enqueue(&self, method: &str) -> io::Result<MutexGuard<'_, ()>> {
        let guard = self.outbound_enqueue.lock();
        if self.shutdown_received.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                format!("`{method}` refused: session is shut down"),
            ));
        }
        Ok(guard)
    }

    /// Install a progress token if this session is still live.
    ///
    /// Holds `progress_tokens` only for admission and insertion — not across
    /// outbound I/O. Production producers must use this (or
    /// [`install_progress_token_with`]) rather than inserting through the
    /// raw mutex.
    pub(crate) fn install_progress_token(
        &self,
        token: String,
        request_id: Option<JsonRpcId>,
    ) -> ProgressTokenInstall {
        install_progress_token_with(
            &self.shutdown_received,
            &self.progress_tokens,
            &self.progress_token_to_request,
            token,
            request_id,
        )
    }

    fn invalidate_identity_and_drain(&self) {
        let _ = self.generation.fetch_add(1, Ordering::AcqRel);
        self.cancelled.lock().clear();
        self.pending_request_ids.lock().clear();
        self.progress_tokens.lock().clear();
        self.progress_token_to_request.lock().clear();
        self.pending_workspace_configuration_requests.lock().clear();
        *self.pending_startup_log.lock() = None;
        *self.text_sync_session.lock() = None;
        *self.position_encoding_session_context.lock() = None;
        *self.initial_root_input.lock() = None;
        *self.client_capabilities.lock() = ClientCapabilities::default();
        *self.initialization_options_perl_settings.lock() = None;
        // Do not flip `client_supports_pull_diags`: false means "push is
        // allowed". Shutdown keeps the negotiated transport frozen on this
        // terminal connection; [`Self::replace_connection`] resets it.
        self.session_warning_dedup.clear_all_families();
    }
}

/// Shared progress-token installer for `ClientSession` and cloned worker
/// handles. Callers must not hold `progress_tokens` across outbound I/O.
pub(crate) fn install_progress_token_with(
    shutdown_received: &AtomicBool,
    progress_tokens: &Mutex<HashSet<String>>,
    progress_token_to_request: &Mutex<HashMap<String, JsonRpcId>>,
    token: String,
    request_id: Option<JsonRpcId>,
) -> ProgressTokenInstall {
    let mut tokens = progress_tokens.lock();
    if shutdown_received.load(Ordering::Acquire) {
        return ProgressTokenInstall::Shutdown;
    }
    if !tokens.insert(token.clone()) {
        return ProgressTokenInstall::AlreadyExists;
    }
    if let Some(id) = request_id {
        progress_token_to_request.lock().insert(token, id);
    }
    ProgressTokenInstall::Installed
}
