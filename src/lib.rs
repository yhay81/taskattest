pub mod discover;
pub mod error;
pub mod execute;
pub mod git;
mod hex;
pub mod model;
pub mod schema;
pub mod source;
pub mod store;
pub mod verify;

pub const DISCOVERY_SCHEMA_VERSION: &str = "taskattest.discovery.v1";
pub const RECEIPT_SCHEMA_VERSION: &str = "taskattest.receipt.v1";
pub const PROGRESS_SCHEMA_VERSION: &str = "taskattest.progress.v1";
pub const VERIFICATION_SCHEMA_VERSION: &str = "taskattest.verification.v1";
pub const ERROR_SCHEMA_VERSION: &str = "taskattest.error.v1";
pub const RECEIPT_RECOVERY_SCHEMA_VERSION: &str = "taskattest.receipt-recovery.v1";
pub const CONFIG_VERSION: u32 = 1;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
