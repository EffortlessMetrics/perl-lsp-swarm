use serde_json::{Map, Value};
use std::error::Error;
use std::fmt::{Display, Formatter};

pub const SCHEMA_VERSION: &str = "actual_host_receipt.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RegistrationState {
    ManualClientRegistration,
    UpstreamSourceRegistration,
    UpstreamAcceptedUnreleased,
    UpstreamBuiltinReleased,
}

impl RegistrationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ManualClientRegistration => "manual_client_registration",
            Self::UpstreamSourceRegistration => "upstream_source_registration",
            Self::UpstreamAcceptedUnreleased => "upstream_accepted_unreleased",
            Self::UpstreamBuiltinReleased => "upstream_builtin_released",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "manual_client_registration" => Some(Self::ManualClientRegistration),
            "upstream_source_registration" => Some(Self::UpstreamSourceRegistration),
            "upstream_accepted_unreleased" => Some(Self::UpstreamAcceptedUnreleased),
            "upstream_builtin_released" => Some(Self::UpstreamBuiltinReleased),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ValidationPolicy {
    pub minimum_registration_state: Option<RegistrationState>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptValidationError(String);

impl ReceiptValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for ReceiptValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ReceiptValidationError {}

pub fn validate_receipt(receipt: &Value) -> Result<(), ReceiptValidationError> {
    validate_receipt_with_policy(receipt, ValidationPolicy::default())
}

pub fn validate_receipt_with_policy(
    receipt: &Value,
    policy: ValidationPolicy,
) -> Result<(), ReceiptValidationError> {
    let root = as_object(receipt, "receipt")?;

    require_exact_string(root, "schema_version", SCHEMA_VERSION, "receipt.schema_version")?;
    require_u64(root, "receipt_version", "receipt.receipt_version")?;
    require_nonempty_string(root, "run_id", "receipt.run_id")?;
    require_nonempty_string(root, "timestamp", "receipt.timestamp")?;

    validate_identity_object(root, "editor", &["family", "version", "source"])?;
    validate_identity_object(root, "client", &["family", "version", "source"])?;
    validate_identity_object(root, "server", &["path", "sha256", "version"])?;
    validate_identity_object(root, "platform", &["os", "arch"])?;
    validate_identity_object(root, "workspace", &["root", "identity"])?;
    validate_identity_object(root, "profile", &["identity", "source"])?;
    validate_identity_object(root, "artifacts", &["client_log", "server_stderr"])?;

    let registration = require_nonempty_string(
        root,
        "registration_state",
        "receipt.registration_state",
    )?;
    let registration = RegistrationState::parse(registration).ok_or_else(|| {
        ReceiptValidationError::new(format!(
            "receipt.registration_state: unsupported value `{registration}`"
        ))
    })?;
    if let Some(required) = policy.minimum_registration_state
        && registration < required
    {
        return Err(ReceiptValidationError::new(format!(
            "receipt.registration_state: `{}` cannot satisfy required `{}` evidence",
            registration.as_str(),
            required.as_str()
        )));
    }

    validate_features(root)?;
    validate_state_machine(root)?;
    validate_extensions(root)?;
    Ok(())
}

fn validate_identity_object(
    root: &Map<String, Value>,
    key: &str,
    fields: &[&str],
) -> Result<(), ReceiptValidationError> {
    let path = format!("receipt.{key}");
    let object = require_object(root, key, &path)?;
    for field in fields {
        require_nonempty_string(object, field, &format!("{path}.{field}"))?;
    }
    Ok(())
}

fn validate_features(root: &Map<String, Value>) -> Result<(), ReceiptValidationError> {
    let features = require_object(root, "features", "receipt.features")?;
    if features.is_empty() {
        return Err(ReceiptValidationError::new(
            "receipt.features: at least one feature outcome is required",
        ));
    }

    for (name, value) in features {
        let path = format!("receipt.features.{name}");
        let feature = as_object(value, &path)?;
        let advertised = require_bool(feature, "advertised", &format!("{path}.advertised"))?;
        let observed = require_bool(feature, "observed", &format!("{path}.observed"))?;
        let outcome = require_nonempty_string(feature, "outcome", &format!("{path}.outcome"))?;

        if !matches!(outcome, "passed" | "failed" | "skipped") {
            return Err(ReceiptValidationError::new(format!(
                "{path}.outcome: unsupported value `{outcome}`"
            )));
        }
        if !advertised && observed {
            return Err(ReceiptValidationError::new(format!(
                "{path}: observed=true contradicts advertised=false"
            )));
        }
        if !observed && outcome == "passed" {
            return Err(ReceiptValidationError::new(format!(
                "{path}: outcome=passed requires observed=true"
            )));
        }

        if outcome == "skipped" {
            let classification = require_nonempty_string(
                feature,
                "skip_classification",
                &format!("{path}.skip_classification"),
            )?;
            if !matches!(
                classification,
                "unsupported"
                    | "harness_limit"
                    | "blocked"
                    | "not_applicable"
                    | "infra_blocked"
            ) {
                return Err(ReceiptValidationError::new(format!(
                    "{path}.skip_classification: unsupported value `{classification}`"
                )));
            }
            require_nonempty_string(feature, "reason", &format!("{path}.reason"))?;
        }
    }
    Ok(())
}

fn validate_state_machine(root: &Map<String, Value>) -> Result<(), ReceiptValidationError> {
    let state = require_object(root, "state_machine", "receipt.state_machine")?;
    validate_terminal_event(state, "initialize")?;
    validate_terminal_event(state, "initialized")?;
    require_nonempty_string(
        state,
        "position_encoding",
        "receipt.state_machine.position_encoding",
    )?;
    require_nonempty_string(
        state,
        "diagnostics_mode",
        "receipt.state_machine.diagnostics_mode",
    )?;
    require_nonempty_string(
        state,
        "diagnostics_response_form",
        "receipt.state_machine.diagnostics_response_form",
    )?;
    validate_terminal_event(state, "workspace_configuration")?;
    validate_terminal_event(state, "register_capability")?;
    validate_terminal_event(state, "watcher_behavior")?;
    validate_terminal_event(state, "refresh")?;
    validate_terminal_event(state, "shutdown")?;
    validate_terminal_event(state, "exit")?;

    let orphan = require_nonempty_string(
        state,
        "orphan_result",
        "receipt.state_machine.orphan_result",
    )?;
    if !matches!(orphan, "none" | "orphan_detected") {
        return Err(ReceiptValidationError::new(format!(
            "receipt.state_machine.orphan_result: unsupported value `{orphan}`"
        )));
    }
    Ok(())
}

fn validate_terminal_event(
    state: &Map<String, Value>,
    key: &str,
) -> Result<(), ReceiptValidationError> {
    let path = format!("receipt.state_machine.{key}");
    let event = require_object(state, key, &path)?;
    let outcome = require_nonempty_string(event, "outcome", &format!("{path}.outcome"))?;
    if !matches!(
        outcome,
        "ok" | "unsupported" | "not_applicable" | "skipped" | "failed"
    ) {
        return Err(ReceiptValidationError::new(format!(
            "{path}.outcome: unsupported value `{outcome}`"
        )));
    }
    if outcome != "ok" {
        require_nonempty_string(event, "reason", &format!("{path}.reason"))?;
    }
    Ok(())
}

fn validate_extensions(root: &Map<String, Value>) -> Result<(), ReceiptValidationError> {
    let Some(value) = root.get("extensions") else {
        return Ok(());
    };
    let extensions = as_object(value, "receipt.extensions")?;
    for key in extensions.keys() {
        if !key.contains('.') || key.starts_with('.') || key.ends_with('.') {
            return Err(ReceiptValidationError::new(format!(
                "receipt.extensions: key `{key}` must be namespaced"
            )));
        }
    }
    Ok(())
}

fn as_object<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a Map<String, Value>, ReceiptValidationError> {
    value
        .as_object()
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: expected object")))
}

fn require_object<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Map<String, Value>, ReceiptValidationError> {
    let value = parent
        .get(key)
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: missing required field")))?;
    as_object(value, path)
}

fn require_nonempty_string<'a>(
    parent: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a str, ReceiptValidationError> {
    let value = parent
        .get(key)
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: missing required field")))?;
    let value = value
        .as_str()
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: expected string")))?;
    if value.trim().is_empty() {
        return Err(ReceiptValidationError::new(format!(
            "{path}: must not be empty"
        )));
    }
    Ok(value)
}

fn require_exact_string(
    parent: &Map<String, Value>,
    key: &str,
    expected: &str,
    path: &str,
) -> Result<(), ReceiptValidationError> {
    let actual = require_nonempty_string(parent, key, path)?;
    if actual != expected {
        return Err(ReceiptValidationError::new(format!(
            "{path}: expected `{expected}`, found `{actual}`"
        )));
    }
    Ok(())
}

fn require_bool(
    parent: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<bool, ReceiptValidationError> {
    parent
        .get(key)
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: missing required field")))?
        .as_bool()
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: expected boolean")))
}

fn require_u64(
    parent: &Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<u64, ReceiptValidationError> {
    parent
        .get(key)
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: missing required field")))?
        .as_u64()
        .ok_or_else(|| ReceiptValidationError::new(format!("{path}: expected unsigned integer")))
}
