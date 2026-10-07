use super::MAX_HARNESS_JSON_BODY_BYTES;
use crate::harness_context_owner_wire::ContextOwnerOperationV2;
use serde::Deserialize;
use zeroize::Zeroize;

const MANAGEMENT_SCHEMA_V1: &str = "ascension.management/v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SanitizedManagementClass {
    InvalidInput,
    Capability,
    Conflict,
    Forbidden,
    Unresolved,
    Unavailable,
    Budget,
    Store,
    Replay,
    Authentication,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagementErrorEnvelope {
    schema_version: String,
    error: ManagementErrorBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagementErrorBody {
    class: SanitizedManagementClass,
    code: SecretText,
    message: SecretText,
}

#[derive(Deserialize)]
#[serde(transparent)]
struct SecretText(String);

impl SecretText {
    fn as_str(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub(super) fn management_error(
    status: u16,
    body: &[u8],
    operation: &ContextOwnerOperationV2,
) -> HarnessTransportError {
    if body.len() > MAX_HARNESS_JSON_BODY_BYTES {
        return HarnessTransportError::InvalidManagementError;
    }
    let Ok(envelope) = serde_json::from_slice::<ManagementErrorEnvelope>(body) else {
        return HarnessTransportError::InvalidManagementError;
    };
    if envelope.schema_version != MANAGEMENT_SCHEMA_V1
        || !safe_error_code(envelope.error.code.as_str())
        || !status_matches_class(status, envelope.error.class, envelope.error.code.as_str())
    {
        return HarnessTransportError::InvalidManagementError;
    }
    if status == 404
        && envelope.error.code.as_str() == "context_control_receipt_not_recorded"
        && matches!(operation, ContextOwnerOperationV2::LookupControl { .. })
    {
        return HarnessTransportError::ControlReceiptNotRecorded;
    }
    HarnessTransportError::RemoteManagement {
        status,
        class: envelope.error.class,
    }
}

fn safe_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 128
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn status_matches_class(status: u16, class: SanitizedManagementClass, code: &str) -> bool {
    match status {
        400 => class == SanitizedManagementClass::InvalidInput,
        401 => class == SanitizedManagementClass::Authentication,
        403 => class == SanitizedManagementClass::Forbidden,
        404 => {
            class == SanitizedManagementClass::InvalidInput
                && matches!(
                    code,
                    "draft_not_found"
                        | "context_owner_draft_not_found"
                        | "context_owner_revision_not_found"
                        | "context_owner_preview_not_found"
                        | "definition_not_found"
                        | "context_binding_not_recorded"
                        | "context_control_receipt_not_recorded"
                )
        }
        409 => matches!(
            class,
            SanitizedManagementClass::Capability
                | SanitizedManagementClass::Conflict
                | SanitizedManagementClass::Unresolved
                | SanitizedManagementClass::Budget
        ),
        422 => class == SanitizedManagementClass::Replay,
        503 => matches!(
            class,
            SanitizedManagementClass::Unavailable | SanitizedManagementClass::Store
        ),
        _ => false,
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum HarnessTransportError {
    InvalidConfiguration,
    InvalidInvocation,
    UnsupportedOperation,
    CredentialUnavailable,
    CredentialDenied,
    ClockUnavailable,
    Deadline,
    IoOutcomeUnknown,
    InvalidFraming,
    ResponseTooLarge,
    InvalidManagementError,
    InvalidOwnerResponse,
    ControlReceiptNotRecorded,
    RemoteManagement {
        status: u16,
        class: SanitizedManagementClass,
    },
    Store(crate::owner_invocation_store::StoreError),
}

impl From<super::framing::FrameError> for HarnessTransportError {
    fn from(error: super::framing::FrameError) -> Self {
        match error {
            super::framing::FrameError::Deadline => Self::Deadline,
            super::framing::FrameError::Io => Self::IoOutcomeUnknown,
            super::framing::FrameError::InvalidFraming => Self::InvalidFraming,
            super::framing::FrameError::ResponseTooLarge => Self::ResponseTooLarge,
        }
    }
}

impl std::fmt::Display for HarnessTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                formatter.write_str("Harness transport configuration is invalid")
            }
            Self::InvalidInvocation => {
                formatter.write_str("Harness invocation failed transport validation")
            }
            Self::UnsupportedOperation => {
                formatter.write_str("Harness operation is not enabled for this transport")
            }
            Self::CredentialUnavailable => {
                formatter.write_str("protected Harness credential is unavailable")
            }
            Self::CredentialDenied => {
                formatter.write_str("protected Harness credential is not authorized")
            }
            Self::ClockUnavailable => formatter.write_str("trusted system clock is unavailable"),
            Self::Deadline => {
                formatter.write_str("Harness request deadline expired; outcome is unknown")
            }
            Self::IoOutcomeUnknown => formatter.write_str("Harness I/O failed; outcome is unknown"),
            Self::InvalidFraming => {
                formatter.write_str("Harness HTTP response framing is invalid; outcome is unknown")
            }
            Self::ResponseTooLarge => {
                formatter.write_str("Harness HTTP response exceeds its bound; outcome is unknown")
            }
            Self::InvalidManagementError => {
                formatter.write_str("Harness returned an invalid typed management error")
            }
            Self::InvalidOwnerResponse => {
                formatter.write_str("Harness response did not match the typed invocation")
            }
            Self::ControlReceiptNotRecorded => {
                formatter.write_str("Harness has not recorded the exact control receipt")
            }
            Self::RemoteManagement { status, class } => {
                write!(formatter, "Harness returned HTTP {status} ({class:?})")
            }
            Self::Store(error) => write!(formatter, "owner invocation record failed: {error}"),
        }
    }
}

impl std::fmt::Debug for HarnessTransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for HarnessTransportError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CredentialRedemptionError {
    Unavailable,
    Denied,
}
