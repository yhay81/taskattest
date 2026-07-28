use std::fmt::{Display, Formatter};
use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;

use crate::ERROR_SCHEMA_VERSION;

#[derive(Debug)]
pub struct TaskError {
    pub code: &'static str,
    pub message: String,
    pub exit_code: u8,
}

impl TaskError {
    pub fn new(code: &'static str, message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code,
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
