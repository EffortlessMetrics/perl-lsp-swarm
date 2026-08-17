//! Session-local keyed authenticator for opaque resolve envelopes.
//!
//! The concrete LSP connection owner is wired by #8342. This type contains no
//! per-item registry and has no durable or cross-process compatibility contract.

use super::{
    RESOLVE_AUTH_TAG_BYTES, ResolveAuthTag, ResolveAuthenticatorFailure,
    ResolveEnvelopeAuthenticator, ResolveIdentityRef,
};
use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

const AUTHENTICATOR_DOMAIN: &[u8] = b"perl-lsp.resolve.session-auth.v1";
const AUTHENTICATOR_LANES: usize = RESOLVE_AUTH_TAG_BYTES / std::mem::size_of::<u64>();
const FIRST_ISSUE_SEQUENCE: u64 = 1;

/// Bounded session-local authenticator used by one LSP connection.
///
/// Four independently randomized [`RandomState`] lanes produce the fixed
/// 32-byte tag. The result is valid only inside one server session; it is not a
/// durable signature, credential, password KDF, or cross-process identity.
pub struct SessionResolveAuthenticator {
    session_identity: ResolveIdentityRef,
    hash_lanes: [RandomState; AUTHENTICATOR_LANES],
    next_issue_sequence: AtomicU64,
}

impl SessionResolveAuthenticator {
    /// Construct an authenticator for one validated server-session identity.
    #[must_use]
    pub fn new(session_identity: ResolveIdentityRef) -> Self {
        Self {
            session_identity,
            hash_lanes: std::array::from_fn(|_| RandomState::new()),
            next_issue_sequence: AtomicU64::new(FIRST_ISSUE_SEQUENCE),
        }
    }

    /// Public session identity authenticated into every token.
    #[must_use]
    pub const fn session_identity(&self) -> &ResolveIdentityRef {
        &self.session_identity
    }

    /// Reserve the next nonzero, nonwrapping token issue sequence.
    pub fn next_issue_sequence(&self) -> Result<u64, ResolveAuthenticatorFailure> {
        self.next_issue_sequence
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| ResolveAuthenticatorFailure::Internal)
    }

    #[cfg(test)]
    fn with_next_sequence_for_test(
        session_identity: ResolveIdentityRef,
        next_issue_sequence: u64,
    ) -> Self {
        Self {
            session_identity,
            hash_lanes: std::array::from_fn(|_| RandomState::new()),
            next_issue_sequence: AtomicU64::new(next_issue_sequence),
        }
    }
}

impl ResolveEnvelopeAuthenticator for SessionResolveAuthenticator {
    fn authenticate(
        &self,
        canonical_unsigned: &[u8],
    ) -> Result<ResolveAuthTag, ResolveAuthenticatorFailure> {
        let message_len = u64::try_from(canonical_unsigned.len())
            .map_err(|_| ResolveAuthenticatorFailure::Internal)?;
        let mut tag = [0_u8; RESOLVE_AUTH_TAG_BYTES];

        for (lane_index, lane) in self.hash_lanes.iter().enumerate() {
            let lane_id =
                u64::try_from(lane_index).map_err(|_| ResolveAuthenticatorFailure::Internal)?;
            let mut hasher = lane.build_hasher();
            hasher.write(AUTHENTICATOR_DOMAIN);
            hasher.write(&lane_id.to_be_bytes());
            hasher.write(self.session_identity.as_str().as_bytes());
            hasher.write(&message_len.to_be_bytes());
            hasher.write(canonical_unsigned);

            let start = lane_index * std::mem::size_of::<u64>();
            let end = start + std::mem::size_of::<u64>();
            tag[start..end].copy_from_slice(&hasher.finish().to_be_bytes());
        }

        Ok(ResolveAuthTag::from_bytes(tag))
    }
}

impl fmt::Debug for SessionResolveAuthenticator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SessionResolveAuthenticator")
            .field("session_identity", &self.session_identity)
            .field(
                "next_issue_sequence",
                &self.next_issue_sequence.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::resolve_envelope::{
        ResolveCurrentnessKind, ResolveCurrentnessRef, ResolveEnvelopeCodec,
        ResolveEnvelopeHeaderV1, ResolveEnvelopeRejection, ResolveEnvelopeSubject, ResolveFamily,
        ResolveReplayDisposition,
    };
    use serde::{Deserialize, Serialize};
    use std::error::Error;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestSubject {
        identity: String,
    }

    impl ResolveEnvelopeSubject for TestSubject {
        const FAMILY: ResolveFamily = ResolveFamily::WorkspaceSymbol;
        const VERSION: u16 = 1;
    }

    fn identity(value: &str) -> Result<ResolveIdentityRef, Box<dyn Error>> {
        Ok(ResolveIdentityRef::new(value)?)
    }

    fn header(
        authenticator: &SessionResolveAuthenticator,
    ) -> Result<ResolveEnvelopeHeaderV1, Box<dyn Error>> {
        Ok(ResolveEnvelopeHeaderV1::for_subject::<TestSubject>(
            authenticator.session_identity().clone(),
            identity("operation:31")?,
            identity("result:41")?,
            identity("profile:utf16")?,
            vec![ResolveCurrentnessRef::new(
                ResolveCurrentnessKind::Workspace,
                identity("workspace:17")?,
            )],
            ResolveReplayDisposition::CurrentSubjectBound,
            authenticator.next_issue_sequence()?,
        )?)
    }

    #[test]
    fn same_session_and_message_produce_the_same_tag() -> Result<(), Box<dyn Error>> {
        let authenticator = SessionResolveAuthenticator::new(identity("session:alpha")?);
        let first = authenticator.authenticate(b"canonical-message")?;
        let second = authenticator.authenticate(b"canonical-message")?;

        assert_eq!(first, second);
        Ok(())
    }

    #[test]
    fn another_message_or_secret_state_does_not_share_the_tag() -> Result<(), Box<dyn Error>> {
        let first = SessionResolveAuthenticator::new(identity("session:alpha")?);
        let second = SessionResolveAuthenticator::new(identity("session:alpha")?);

        assert_ne!(
            first.authenticate(b"canonical-message")?,
            first.authenticate(b"changed-message")?
        );
        assert_ne!(
            first.authenticate(b"canonical-message")?,
            second.authenticate(b"canonical-message")?
        );
        Ok(())
    }

    #[test]
    fn issue_sequence_is_nonzero_monotonic_and_nonwrapping() -> Result<(), Box<dyn Error>> {
        let authenticator = SessionResolveAuthenticator::new(identity("session:alpha")?);
        assert_eq!(authenticator.next_issue_sequence()?, 1);
        assert_eq!(authenticator.next_issue_sequence()?, 2);

        let exhausted = SessionResolveAuthenticator::with_next_sequence_for_test(
            identity("session:exhausted")?,
            u64::MAX,
        );
        assert_eq!(
            exhausted.next_issue_sequence(),
            Err(ResolveAuthenticatorFailure::Internal)
        );
        assert_eq!(
            exhausted.next_issue_sequence(),
            Err(ResolveAuthenticatorFailure::Internal)
        );
        Ok(())
    }

    #[test]
    fn codec_rejects_another_session_and_another_key() -> Result<(), Box<dyn Error>> {
        let codec = ResolveEnvelopeCodec::default();
        let first = SessionResolveAuthenticator::new(identity("session:first")?);
        let same_public_session_new_key =
            SessionResolveAuthenticator::new(identity("session:first")?);
        let other_session = SessionResolveAuthenticator::new(identity("session:second")?);

        let token = codec.issue(
            header(&first)?,
            TestSubject {
                identity: "symbol:7".to_string(),
            },
            &first,
        )?;

        assert!(
            codec
                .validate::<TestSubject, _>(&token, first.session_identity(), &first)
                .is_ok()
        );
        assert_eq!(
            codec.validate::<TestSubject, _>(
                &token,
                same_public_session_new_key.session_identity(),
                &same_public_session_new_key,
            ),
            Err(ResolveEnvelopeRejection::IntegrityFailure)
        );
        assert_eq!(
            codec.validate::<TestSubject, _>(
                &token,
                other_session.session_identity(),
                &other_session,
            ),
            Err(ResolveEnvelopeRejection::ForeignSession)
        );
        Ok(())
    }

    #[test]
    fn debug_output_contains_no_secret_or_tag_material() -> Result<(), Box<dyn Error>> {
        let authenticator = SessionResolveAuthenticator::new(identity("session:alpha")?);
        let tag = authenticator.authenticate(b"canonical-message")?;
        let debug = format!("{authenticator:?}");

        assert!(debug.contains("session:alpha"));
        assert!(!debug.contains(&tag.0));
        assert!(!debug.contains("hash_lanes"));
        Ok(())
    }

    #[test]
    fn authenticator_is_send_sync_and_fixed_size() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SessionResolveAuthenticator>();

        assert!(
            std::mem::size_of::<SessionResolveAuthenticator>() <= 512,
            "session authenticator state must remain fixed and bounded"
        );
    }
}
