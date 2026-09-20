//! Connection-boundary lifecycle for the resolve-envelope session
//! authenticator (#8342).
//!
//! One [`SessionResolveAuthenticator`] is constructed per `LspServer`
//! instance — i.e. per LSP connection — with a fresh session identity and
//! fresh process-random lane keys. The `shutdown` request destroys it, so
//! every envelope issued before teardown becomes unverifiable and a restarted
//! server rejects old envelopes as foreign. No per-item registry, token
//! store, or key material leaves this boundary; provider leaves (#8295 /
//! #8299 / #8302 / #8304) adopt the substrate later without changing any
//! capability or payload here.

use super::LspServer;
use perl_lsp_rs_core::protocol::resolve_envelope::{
    ResolveIdentityRef, SessionResolveAuthenticator,
};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process sequence mixing into each generated session identity.
static SESSION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Construct the authenticator for one new server session.
///
/// The public session identity is `session:` followed by 16 hex characters
/// derived from process-random `RandomState` keys and a per-process sequence,
/// so two connections never share an identity in practice. Returns `None`
/// only if the core identity contract rejects the generated reference, which
/// leaves the server fail-closed (no envelope can be issued) rather than
/// panicking at the connection boundary.
pub(crate) fn new_session_authenticator() -> Option<SessionResolveAuthenticator> {
    let sequence = SESSION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(sequence);
    let reference = format!("session:{:016x}", hasher.finish());
    ResolveIdentityRef::new(reference).ok().map(SessionResolveAuthenticator::new)
}

impl LspServer {
    /// Destroy the session authenticator at connection teardown.
    ///
    /// Idempotent: a second call is a no-op. Dropping the authenticator
    /// discards the lane keys, so envelopes from this session can never be
    /// re-validated by this or any later session.
    pub(crate) fn teardown_resolve_session(&self) {
        self.resolve_session_authenticator.lock().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perl_lsp_rs_core::protocol::resolve_envelope::{
        ResolveCurrentnessKind, ResolveCurrentnessRef, ResolveEnvelopeCodec,
        ResolveEnvelopeHeaderV1, ResolveEnvelopeRejection, ResolveEnvelopeSubject,
        ResolveEnvelopeToken, ResolveFamily, ResolveReplayDisposition,
    };
    use serde::{Deserialize, Serialize};
    use std::error::Error;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct WiringSubject {
        marker: String,
    }

    impl ResolveEnvelopeSubject for WiringSubject {
        const FAMILY: ResolveFamily = ResolveFamily::WorkspaceSymbol;
        const VERSION: u16 = 1;
    }

    fn identity(value: &str) -> Result<ResolveIdentityRef, Box<dyn Error>> {
        Ok(ResolveIdentityRef::new(value)?)
    }

    fn issue_token(server: &LspServer) -> Result<ResolveEnvelopeToken, Box<dyn Error>> {
        let guard = server.resolve_session_authenticator.lock();
        let authenticator = guard.as_ref().ok_or("session authenticator must be present")?;
        let header = ResolveEnvelopeHeaderV1::for_subject::<WiringSubject>(
            authenticator.session_identity().clone(),
            identity("operation:1")?,
            identity("result:1")?,
            identity("profile:utf16")?,
            vec![ResolveCurrentnessRef::new(
                ResolveCurrentnessKind::Workspace,
                identity("workspace:1")?,
            )],
            ResolveReplayDisposition::CurrentSubjectBound,
            authenticator.next_issue_sequence()?,
        )?;
        let token = ResolveEnvelopeCodec::default().issue(
            header,
            WiringSubject { marker: "symbol:1".to_string() },
            authenticator,
        )?;
        Ok(token)
    }

    #[test]
    fn issued_envelope_validates_inside_the_issuing_session() -> Result<(), Box<dyn Error>> {
        let server = LspServer::new();
        let token = issue_token(&server)?;

        let guard = server.resolve_session_authenticator.lock();
        let authenticator = guard.as_ref().ok_or("session authenticator must be present")?;
        ResolveEnvelopeCodec::default()
            .validate::<WiringSubject, _>(&token, authenticator.session_identity(), authenticator)
            .map_err(|rejection| format!("same-session validation must succeed: {rejection}"))?;
        Ok(())
    }

    #[test]
    fn a_restarted_connection_rejects_old_envelopes_as_foreign() -> Result<(), Box<dyn Error>> {
        let first = LspServer::new();
        let second = LspServer::new();
        let token = issue_token(&first)?;

        let first_identity = {
            let guard = first.resolve_session_authenticator.lock();
            guard.as_ref().ok_or("first session must be present")?.session_identity().clone()
        };
        let second_guard = second.resolve_session_authenticator.lock();
        let second_authenticator = second_guard.as_ref().ok_or("second session must be present")?;

        assert_ne!(
            first_identity.as_str(),
            second_authenticator.session_identity().as_str(),
            "two connections must not share a session identity"
        );
        assert_eq!(
            ResolveEnvelopeCodec::default().validate::<WiringSubject, _>(
                &token,
                second_authenticator.session_identity(),
                second_authenticator,
            ),
            Err(ResolveEnvelopeRejection::ForeignSession),
            "an envelope from a previous session must be rejected before provider work"
        );
        Ok(())
    }

    #[test]
    fn shutdown_teardown_destroys_the_authenticator_and_is_idempotent() {
        let server = LspServer::new();
        assert!(server.resolve_session_authenticator.lock().is_some());

        server.teardown_resolve_session();
        assert!(
            server.resolve_session_authenticator.lock().is_none(),
            "teardown must destroy the session authenticator"
        );

        server.teardown_resolve_session();
        assert!(server.resolve_session_authenticator.lock().is_none());
    }

    #[test]
    fn session_state_is_fixed_size_and_carries_no_per_item_registry() {
        assert!(
            std::mem::size_of::<Option<SessionResolveAuthenticator>>() <= 512,
            "resolve session state must stay fixed-size regardless of issued envelope count"
        );
    }
}
