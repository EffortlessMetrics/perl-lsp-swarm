//! Content hygiene for handoff envelopes.
//!
//! A handoff crosses trust boundaries: it is produced in one workspace and
//! read in another. Two classes of content must never make that crossing
//! silently — credentials, and envelope-relative names that could escape the
//! envelope on the reading side.
//!
//! Credential-bearing content is *refused*, not redacted, when it is part of
//! an immutable Git object. Rewriting a commit message to hide a token would
//! change the candidate; refusing tells the operator the truth instead.

/// A credential or secret detected in retained content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretFinding {
    /// Field the finding was located in.
    pub field: String,
    /// Stable classification of what was matched, never the matched bytes.
    pub kind: &'static str,
}

/// Markers that identify a credential wherever they appear in retained text.
const CREDENTIAL_MARKERS: &[(&str, &str)] = &[
    ("ghp_", "github_personal_access_token"),
    ("gho_", "github_oauth_token"),
    ("ghu_", "github_user_token"),
    ("ghs_", "github_server_token"),
    ("ghr_", "github_refresh_token"),
    ("github_pat_", "github_fine_grained_token"),
    ("xoxb-", "slack_bot_token"),
    ("-----BEGIN OPENSSH PRIVATE KEY-----", "private_key"),
    ("-----BEGIN RSA PRIVATE KEY-----", "private_key"),
    ("-----BEGIN PRIVATE KEY-----", "private_key"),
    // Assignment shapes for secrets that have no distinctive token prefix.
    ("AWS_SECRET_ACCESS_KEY=", "aws_secret_access_key"),
    ("_authToken=", "npm_auth_token"),
];

/// Scan one retained string for credential material.
///
/// Returns every distinct classification found so a receipt can name the
/// class without ever echoing the secret itself.
#[must_use]
pub fn scan_secrets(field: &str, value: &str) -> Vec<SecretFinding> {
    let mut findings: Vec<SecretFinding> = Vec::new();
    let record = |kind: &'static str, findings: &mut Vec<SecretFinding>| {
        if !findings.iter().any(|finding| finding.kind == kind) {
            findings.push(SecretFinding { field: field.to_string(), kind });
        }
    };

    for (marker, kind) in CREDENTIAL_MARKERS {
        if value.contains(marker) {
            record(kind, &mut findings);
        }
    }
    if contains_aws_access_key_id(value) {
        record("aws_access_key_id", &mut findings);
    }
    if contains_netrc_password(value) {
        record("netrc_password", &mut findings);
    }
    if url_carries_credentials(value) {
        record("url_embedded_credentials", &mut findings);
    }
    findings
}

/// Whether `value` contains a full AWS access-key id, `AKIA` plus 16 uppercase
/// alphanumerics.
///
/// Matching the bare `AKIA` prefix refused ordinary prose — an identifier or a
/// module name containing those four letters — and a producer has no override,
/// so an over-broad marker makes legitimate candidates unexportable.
#[must_use]
pub fn contains_aws_access_key_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(20).any(|window| {
        window.starts_with(b"AKIA")
            && window[4..].iter().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    })
}

/// Whether `value` looks like a `.netrc` entry carrying a password.
#[must_use]
pub fn contains_netrc_password(value: &str) -> bool {
    value.lines().any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.windows(2).any(|pair| pair[0] == "password" && !pair[1].is_empty())
            && fields.contains(&"machine")
    })
}

/// Whether `value` contains a URL with a userinfo section.
///
/// Any userinfo is treated as credential-bearing, not only `user:password`.
/// The bare-token form `https://<token>@github.com/owner/name` is the ordinary
/// way a PAT is embedded in a remote and carries no colon at all, and a
/// percent-encoded `%3A` hides one; requiring a literal colon classified both
/// as clean and let the manifest record such a remote as an observed, and
/// therefore trustworthy, identity source.
///
/// `git@github.com:owner/name` has no `://` and is unaffected, so the ordinary
/// SSH remote is not misread as credential-bearing.
#[must_use]
pub fn url_carries_credentials(value: &str) -> bool {
    let Some(scheme_end) = value.find("://") else {
        return false;
    };
    let after_scheme = &value[scheme_end + 3..];
    // Only a `@` before the first path separator is a userinfo section; a `@`
    // later in the URL belongs to the path or query.
    let authority = after_scheme.split('/').next().unwrap_or_default();
    let Some(at_index) = authority.find('@') else {
        return false;
    };
    !authority[..at_index].is_empty()
}

/// Whether an envelope-relative name is safe to join onto a reader's root.
///
/// Absolute paths, drive letters, parent traversal, and backslash separators
/// are all refused so a manifest cannot direct a reader outside the envelope.
#[must_use]
pub fn is_safe_envelope_name(value: &str) -> bool {
    if value.is_empty() || value.len() > 255 {
        return false;
    }
    if value.starts_with('/') || value.starts_with('\\') || value.contains('\\') {
        return false;
    }
    if value.contains(':') {
        return false;
    }
    if value.split('/').any(|segment| segment.is_empty() || segment == "." || segment == "..") {
        return false;
    }
    !value.chars().any(char::is_control)
}

/// Whether a proof identifier is a stable lowercase token.
#[must_use]
pub fn is_proof_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !bytes[0].is_ascii_alphanumeric() {
        return false;
    }
    bytes.iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

/// Whether `value` is a lowercase `owner/name` repository identity.
///
/// Deliberately structural: an identity that cannot be expressed as
/// `owner/name` is not accepted in redacted or partial form, so no remote URL
/// bytes — and therefore no embedded credentials — can survive into a
/// manifest through this field.
#[must_use]
pub fn is_repository_identity(value: &str) -> bool {
    let mut parts = value.split('/');
    let (Some(owner), Some(name)) = (parts.next(), parts.next()) else {
        return false;
    };
    if parts.next().is_some() || owner.is_empty() || name.is_empty() {
        return false;
    }
    if value != value.to_lowercase() {
        return false;
    }
    let acceptable = |segment: &str| {
        segment.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    acceptable(owner) && acceptable(name)
}

/// Why a remote URL could not be used as an identity source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteIdentityError {
    /// The URL embedded credentials, so none of its bytes may be retained.
    CredentialsPresent,
}

impl std::fmt::Display for RemoteIdentityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CredentialsPresent => {
                write!(formatter, "the configured remote URL embedded credentials")
            }
        }
    }
}

impl std::error::Error for RemoteIdentityError {}

/// Extract `owner/name` from a Git remote URL, refusing credential-bearing URLs.
///
/// A credential-bearing URL is an error rather than a redacted value, so the
/// caller records the refusal as a limitation and no URL bytes are retained.
pub fn repository_identity_from_remote(url: &str) -> Result<Option<String>, RemoteIdentityError> {
    let trimmed = url.trim();
    if url_carries_credentials(trimmed) {
        return Err(RemoteIdentityError::CredentialsPresent);
    }
    // Strip scheme-and-host for URL forms, or the `host:` prefix for SCP form.
    let path = if let Some(scheme_end) = trimmed.find("://") {
        let after_scheme = &trimmed[scheme_end + 3..];
        match after_scheme.find('/') {
            Some(index) => &after_scheme[index + 1..],
            None => return Ok(None),
        }
    } else {
        match trimmed.split_once(':') {
            Some((_, path)) => path,
            None => return Ok(None),
        }
    };

    let path = path.trim_start_matches('/').trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let segments: Vec<&str> = path.split('/').filter(|segment| !segment.is_empty()).collect();
    // Take the final two segments so nested hosting prefixes do not confuse
    // the owner/name pair.
    let [.., owner, name] = segments.as_slice() else {
        return Ok(None);
    };
    let candidate = format!("{}/{}", owner.to_lowercase(), name.to_lowercase());
    Ok(is_repository_identity(&candidate).then_some(candidate))
}
