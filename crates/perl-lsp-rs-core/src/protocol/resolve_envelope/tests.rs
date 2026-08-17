use super::*;
use std::collections::HashMap;
use std::error::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SyntheticSubject {
    identity: String,
    facts: HashMap<String, String>,
}

impl ResolveEnvelopeSubject for SyntheticSubject {
    const FAMILY: ResolveFamily = ResolveFamily::Synthetic;
    const VERSION: u16 = 1;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SyntheticSubjectV2 {
    identity: String,
    facts: HashMap<String, String>,
}

impl ResolveEnvelopeSubject for SyntheticSubjectV2 {
    const FAMILY: ResolveFamily = ResolveFamily::Synthetic;
    const VERSION: u16 = 2;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WorkspaceSymbolSubject {
    identity: String,
    facts: HashMap<String, String>,
}

impl ResolveEnvelopeSubject for WorkspaceSymbolSubject {
    const FAMILY: ResolveFamily = ResolveFamily::WorkspaceSymbol;
    const VERSION: u16 = 1;
}

struct TestAuthenticator {
    key: u8,
    fail: bool,
}

impl TestAuthenticator {
    const fn new(key: u8) -> Self {
        Self { key, fail: false }
    }

    const fn failing() -> Self {
        Self { key: 0, fail: true }
    }
}

impl ResolveEnvelopeAuthenticator for TestAuthenticator {
    fn authenticate(
        &self,
        canonical_unsigned: &[u8],
    ) -> Result<ResolveAuthTag, ResolveAuthenticatorFailure> {
        if self.fail {
            return Err(ResolveAuthenticatorFailure::Unavailable);
        }

        let mut tag = [0_u8; RESOLVE_AUTH_TAG_BYTES];
        for (index, byte) in canonical_unsigned.iter().copied().enumerate() {
            let slot = index % RESOLVE_AUTH_TAG_BYTES;
            tag[slot] =
                tag[slot].wrapping_add(byte.rotate_left((index % 8) as u32)).wrapping_add(self.key);
        }
        tag[0] ^= (canonical_unsigned.len() & 0xff) as u8;
        tag[1] ^= ((canonical_unsigned.len() >> 8) & 0xff) as u8;
        Ok(ResolveAuthTag::from_bytes(tag))
    }
}

fn identity(value: &str) -> Result<ResolveIdentityRef, ResolveEnvelopeIssueError> {
    ResolveIdentityRef::new(value)
}

fn header<T: ResolveEnvelopeSubject>(
    session: &str,
) -> Result<ResolveEnvelopeHeaderV1, ResolveEnvelopeIssueError> {
    ResolveEnvelopeHeaderV1::for_subject::<T>(
        identity(session)?,
        identity("operation:17")?,
        identity("result:23")?,
        identity("profile:utf16-tooltip")?,
        vec![
            ResolveCurrentnessRef::new(ResolveCurrentnessKind::Document, identity("document:4")?),
            ResolveCurrentnessRef::new(
                ResolveCurrentnessKind::Configuration,
                identity("config:9")?,
            ),
        ],
        ResolveReplayDisposition::CurrentSubjectBound,
        1,
    )
}

fn subject(first: (&str, &str), second: (&str, &str)) -> SyntheticSubject {
    let mut facts = HashMap::new();
    facts.insert(first.0.to_string(), first.1.to_string());
    facts.insert(second.0.to_string(), second.1.to_string());
    SyntheticSubject { identity: "entity:42".to_string(), facts }
}

#[test]
fn typed_token_round_trips_through_the_issuing_session() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(7);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("b", "2"), ("a", "1")),
        &authenticator,
    )?;

    let validated = codec.validate::<SyntheticSubject, _>(
        &token,
        &identity("session:alpha")?,
        &authenticator,
    )?;

    assert_eq!(validated.header().family(), ResolveFamily::Synthetic);
    assert_eq!(validated.subject().identity, "entity:42");
    assert_eq!(validated.subject().facts.get("a").map(String::as_str), Some("1"));
    Ok(())
}

#[test]
fn map_insertion_order_does_not_change_the_token() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(9);

    let first = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &authenticator,
    )?;
    let second = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("b", "2"), ("a", "1")),
        &authenticator,
    )?;

    assert_eq!(first, second);
    Ok(())
}

#[test]
fn altered_authentication_tag_fails_integrity() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(11);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &authenticator,
    )?;

    let encoded =
        token.as_str().strip_prefix(RESOLVE_ENVELOPE_TOKEN_PREFIX).ok_or("missing token prefix")?;
    let decoded = decode_hex_bounded(encoded, DEFAULT_MAX_DECODED_BYTES)?;
    let mut value: Value = serde_json::from_slice(&decoded)?;
    let current_tag = value.get("tag").and_then(Value::as_str).ok_or("missing tag")?;
    let replacement = if current_tag.starts_with('0') { '1' } else { '0' };
    let mut changed_tag = current_tag.to_string();
    changed_tag.replace_range(0..1, &replacement.to_string());
    value["tag"] = Value::String(changed_tag);
    let changed_bytes = canonical_json_bytes(&value)?;
    let changed_token = ResolveEnvelopeToken(format!(
        "{RESOLVE_ENVELOPE_TOKEN_PREFIX}{}",
        encode_hex(&changed_bytes)
    ));

    assert_eq!(
        codec.validate::<SyntheticSubject, _>(
            &changed_token,
            &identity("session:alpha")?,
            &authenticator,
        ),
        Err(ResolveEnvelopeRejection::IntegrityFailure)
    );
    Ok(())
}

#[test]
fn noncanonical_internal_bytes_are_rejected() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(13);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &authenticator,
    )?;

    let encoded =
        token.as_str().strip_prefix(RESOLVE_ENVELOPE_TOKEN_PREFIX).ok_or("missing token prefix")?;
    let decoded = decode_hex_bounded(encoded, DEFAULT_MAX_DECODED_BYTES)?;
    let mut text = String::from_utf8(decoded)?;
    text.insert(1, ' ');
    let noncanonical = ResolveEnvelopeToken(format!(
        "{RESOLVE_ENVELOPE_TOKEN_PREFIX}{}",
        encode_hex(text.as_bytes())
    ));

    assert_eq!(
        codec.validate::<SyntheticSubject, _>(
            &noncanonical,
            &identity("session:alpha")?,
            &authenticator,
        ),
        Err(ResolveEnvelopeRejection::NonCanonical)
    );
    Ok(())
}

#[test]
fn another_session_is_distinct_from_integrity_failure() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(15);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &authenticator,
    )?;

    assert_eq!(
        codec.validate::<SyntheticSubject, _>(&token, &identity("session:beta")?, &authenticator,),
        Err(ResolveEnvelopeRejection::ForeignSession)
    );
    Ok(())
}

#[test]
fn one_family_cannot_decode_as_another() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(17);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &authenticator,
    )?;

    assert_eq!(
        codec.validate::<WorkspaceSymbolSubject, _>(
            &token,
            &identity("session:alpha")?,
            &authenticator,
        ),
        Err(ResolveEnvelopeRejection::WrongMethodOrFamily)
    );
    Ok(())
}

#[test]
fn unknown_subject_version_remains_explicit() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(19);
    let v2 = SyntheticSubjectV2 {
        identity: "entity:42".to_string(),
        facts: subject(("a", "1"), ("b", "2")).facts,
    };
    let token = codec.issue(header::<SyntheticSubjectV2>("session:alpha")?, v2, &authenticator)?;

    assert_eq!(
        codec.validate::<SyntheticSubject, _>(&token, &identity("session:alpha")?, &authenticator,),
        Err(ResolveEnvelopeRejection::UnknownSubjectVersion(2))
    );
    Ok(())
}

#[test]
fn unknown_envelope_version_remains_explicit() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(20);
    let subject = subject(("a", "1"), ("b", "2"));
    let mut header = header::<SyntheticSubject>("session:alpha")?;
    header.envelope_version = 2;

    let unsigned = UnsignedResolveEnvelopeRef { header: &header, subject: &subject };
    let unsigned_bytes = canonical_json_bytes(&unsigned)?;
    let tag = authenticator.authenticate(&unsigned_bytes)?;
    let signed = SignedResolveEnvelope { header, subject, tag };
    let signed_bytes = canonical_json_bytes(&signed)?;
    let token = ResolveEnvelopeToken(format!(
        "{RESOLVE_ENVELOPE_TOKEN_PREFIX}{}",
        encode_hex(&signed_bytes)
    ));

    assert_eq!(
        codec.validate::<SyntheticSubject, _>(&token, &identity("session:alpha")?, &authenticator,),
        Err(ResolveEnvelopeRejection::UnknownEnvelopeVersion(2))
    );
    Ok(())
}

#[test]
fn path_uri_and_prose_identity_references_are_rejected() {
    for invalid in
        ["file:///tmp/source.pl", "/tmp/source.pl", "C:\\source.pl", "contains whitespace", ""]
    {
        assert!(ResolveIdentityRef::new(invalid).is_err(), "{invalid:?} must be rejected");
    }
}

#[test]
fn duplicate_and_excess_currentness_references_fail_issue() -> Result<(), Box<dyn Error>> {
    let duplicate = ResolveEnvelopeHeaderV1::for_subject::<SyntheticSubject>(
        identity("session:alpha")?,
        identity("operation:17")?,
        identity("result:23")?,
        identity("profile:utf16-tooltip")?,
        vec![
            ResolveCurrentnessRef::new(ResolveCurrentnessKind::Document, identity("document:4")?),
            ResolveCurrentnessRef::new(ResolveCurrentnessKind::Document, identity("document:5")?),
        ],
        ResolveReplayDisposition::CurrentSubjectBound,
        1,
    );
    assert!(matches!(duplicate, Err(ResolveEnvelopeIssueError::DuplicateCurrentnessKind)));

    let mut excessive = Vec::new();
    for index in 0..=DEFAULT_MAX_CURRENTNESS_REFS {
        let kind = match index % 5 {
            0 => ResolveCurrentnessKind::Document,
            1 => ResolveCurrentnessKind::Source,
            2 => ResolveCurrentnessKind::Root,
            3 => ResolveCurrentnessKind::Workspace,
            _ => ResolveCurrentnessKind::Configuration,
        };
        excessive.push(ResolveCurrentnessRef::new(kind, identity(&format!("current:{index}"))?));
    }
    let excessive_header = ResolveEnvelopeHeaderV1::for_subject::<SyntheticSubject>(
        identity("session:alpha")?,
        identity("operation:17")?,
        identity("result:23")?,
        identity("profile:utf16-tooltip")?,
        excessive,
        ResolveReplayDisposition::CurrentSubjectBound,
        1,
    );
    assert!(matches!(
        excessive_header,
        Err(ResolveEnvelopeIssueError::TooManyCurrentnessReferences)
    ));
    Ok(())
}

#[test]
fn oversized_subject_is_rejected_before_authentication() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(21);
    let oversized = SyntheticSubject {
        identity: "entity:large".to_string(),
        facts: HashMap::from([("payload".to_string(), "x".repeat(DEFAULT_MAX_SUBJECT_BYTES + 1))]),
    };

    assert!(matches!(
        codec.issue(header::<SyntheticSubject>("session:alpha")?, oversized, &authenticator,),
        Err(ResolveEnvelopeIssueError::OversizedOrResourceBound)
    ));
    Ok(())
}

#[test]
fn authenticator_failure_is_not_an_integrity_mismatch() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let good = TestAuthenticator::new(23);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &good,
    )?;

    assert_eq!(
        codec.validate::<SyntheticSubject, _>(
            &token,
            &identity("session:alpha")?,
            &TestAuthenticator::failing(),
        ),
        Err(ResolveEnvelopeRejection::InstrumentFailure)
    );
    Ok(())
}

#[test]
fn uppercase_outer_hex_is_not_an_alternate_wire_form() -> Result<(), Box<dyn Error>> {
    let codec = ResolveEnvelopeCodec::default();
    let authenticator = TestAuthenticator::new(25);
    let token = codec.issue(
        header::<SyntheticSubject>("session:alpha")?,
        subject(("a", "1"), ("b", "2")),
        &authenticator,
    )?;
    let uppercase = token.as_str().to_ascii_uppercase();

    assert_eq!(ResolveEnvelopeToken::parse(uppercase), Err(ResolveEnvelopeRejection::Malformed));
    Ok(())
}
