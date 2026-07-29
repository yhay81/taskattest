use std::fmt::{Display, Formatter};
use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;

use crate::{ERROR_SCHEMA_VERSION, RECEIPT_RECOVERY_SCHEMA_VERSION};

#[derive(Debug)]
pub struct TaskError {
    pub code: &'static str,
    pub message: String,
    pub exit_code: u8,
    pub recovery: Option<Box<ReceiptRecoveryDocument>>,
}

impl TaskError {
    pub fn new(code: &'static str, message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code,
            recovery: None,
        }
    }

    pub fn io(action: &str, path: &Path, error: std::io::Error) -> Self {
        Self::new(
            "io_failed",
            format!("{action} {}: {error}", path.display()),
            6,
        )
    }

    pub fn git(message: impl Into<String>) -> Self {
        Self::new("git_failed", message, 3)
    }

    pub fn discovery(message: impl Into<String>) -> Self {
        Self::new("discovery_failed", message, 3)
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new("configuration_invalid", message, 3)
    }

    pub fn execution(message: impl Into<String>) -> Self {
        Self::new("execution_failed", message, 4)
    }

    pub fn receipt(message: impl Into<String>) -> Self {
        Self::new("receipt_invalid", message, 5)
    }

    pub fn post_execution_receipt_publication(
        receipt_id: &str,
        stored_receipt: &Path,
        requested_receipt: &Path,
        cause: &Self,
    ) -> Self {
        let code = "receipt_publication_failed_after_execution";
        let message = format!(
            "checks completed and receipt {receipt_id} is stored at {}, but standalone publication to {} failed: {}; do not retry taskattest run",
            stored_receipt.display(),
            requested_receipt.display(),
            cause.message
        );
        Self {
            code,
            message: message.clone(),
            exit_code: 7,
            recovery: Some(Box::new(ReceiptRecoveryDocument {
                schema_version: RECEIPT_RECOVERY_SCHEMA_VERSION.to_owned(),
                error_code: code.to_owned(),
                message,
                exit_code: 7,
                action: ReceiptRecoveryAction::DoNotRetryRun,
                command_state: ReceiptCommandState::ChecksCompleted,
                receipt_id: receipt_id.to_owned(),
                receipt_persisted: true,
                stored_receipt: stored_receipt.display().to_string(),
                requested_receipt: requested_receipt.display().to_string(),
                publication_error_code: cause.code.to_owned(),
            })),
        }
    }
}

impl Display for TaskError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TaskError {}

impl From<serde_json::Error> for TaskError {
    fn from(error: serde_json::Error) -> Self {
        Self::new("json_invalid", error.to_string(), 5)
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
pub struct ErrorDocument {
    pub schema_version: String,
    pub error_code: String,
    pub message: String,
    pub exit_code: u8,
}

impl From<&TaskError> for ErrorDocument {
    fn from(error: &TaskError) -> Self {
        Self {
            schema_version: ERROR_SCHEMA_VERSION.to_owned(),
            error_code: error.code.to_owned(),
            message: error.message.clone(),
            exit_code: error.exit_code,
        }
    }
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptRecoveryAction {
    DoNotRetryRun,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptCommandState {
    ChecksCompleted,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(deny_unknown_fields)]
pub struct ReceiptRecoveryDocument {
    pub schema_version: String,
    pub error_code: String,
    pub message: String,
    pub exit_code: u8,
    pub action: ReceiptRecoveryAction,
    pub command_state: ReceiptCommandState,
    pub receipt_id: String,
    pub receipt_persisted: bool,
    pub stored_receipt: String,
    pub requested_receipt: String,
    pub publication_error_code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_execution_publication_error_is_machine_actionable() {
        let cause = TaskError::new("output_already_exists", "occupied", 4);
        let error = TaskError::post_execution_receipt_publication(
            "rcpt_0123456789abcdef0123456789abcdef",
            Path::new("/state/receipts/receipt.json"),
            Path::new("/requested/receipt.json"),
            &cause,
        );
        let recovery = error.recovery.expect("recovery document");
        assert_eq!(error.code, "receipt_publication_failed_after_execution");
        assert_eq!(error.exit_code, 7);
        assert!(matches!(
            recovery.action,
            ReceiptRecoveryAction::DoNotRetryRun
        ));
        assert!(recovery.receipt_persisted);
        assert_eq!(recovery.publication_error_code, "output_already_exists");
    }

    #[test]
    fn normal_error_v1_shape_is_unchanged() {
        let error = TaskError::execution("failed");
        let document =
            serde_json::to_value(ErrorDocument::from(&error)).expect("serialize error document");
        assert_eq!(document["schema_version"], "taskattest.error.v1");
        assert_eq!(
            document
                .as_object()
                .expect("error document object")
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            ["error_code", "exit_code", "message", "schema_version"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }
}
