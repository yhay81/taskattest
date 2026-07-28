use schemars::JsonSchema;
use serde_json::{Value, json};

use crate::error::ErrorDocument;
use crate::model::{DiscoveryReport, ProgressEvent, Receipt, VerificationReport};
use crate::{
    DISCOVERY_SCHEMA_VERSION, ERROR_SCHEMA_VERSION, PROGRESS_SCHEMA_VERSION,
    RECEIPT_SCHEMA_VERSION, VERIFICATION_SCHEMA_VERSION, VERSION,
};

pub fn brief_schema() -> Value {
    json!({
        "schema_version": "taskattest.brief.v1",
        "taskattest_version": VERSION,
        "commands": {
            "schema": {
                "purpose": "emit brief capability metadata or a full JSON Schema",
                "documents": ["brief", "discovery", "receipt", "progress", "verification", "error"]
            },
            "discover": {
                "purpose": "read repository evidence and explain selected or omitted checks",
                "mutates_workspace": false
            },
            "run": {
                "purpose": "execute selected checks without a shell and persist evidence by digest",
                "defaults": {
                    "max_runtime_ms_per_check": 900000,
                    "max_log_bytes_per_check": 67108864,
                    "environment": "allowlist",
                    "network_sandbox": false
                }
            },
            "receipt_show": {
                "purpose": "load a stored receipt by id or path",
                "mutates_workspace": false
            },
            "verify": {
                "purpose": "verify canonical receipt integrity and every referenced local log blob",
                "reruns_checks": false
            }
        },
        "exit_codes": {
            "0": "requested operation succeeded; run receipts have passed outcome",
            "1": "checks failed, verification was incomplete, or a receipt did not verify",
            "2": "command-line usage error",
            "3": "Git discovery or configuration error",
            "4": "execution or persistence error",
            "5": "receipt parsing or integrity error",
            "6": "filesystem I/O error"
        },
        "document_versions": {
            "discovery": DISCOVERY_SCHEMA_VERSION,
            "receipt": RECEIPT_SCHEMA_VERSION,
            "progress": PROGRESS_SCHEMA_VERSION,
            "verification": VERIFICATION_SCHEMA_VERSION,
            "error": ERROR_SCHEMA_VERSION
        },
        "safety": {
            "shell_interpolation": false,
            "default_upload": false,
            "full_logs": "content-addressed local blobs marked potentially_sensitive",
            "environment_values_recorded": false,
            "claim": "evidence captured, not correctness proven"
        }
    })
}

pub fn document_schema(document: SchemaDocument) -> Value {
    match document {
        SchemaDocument::Brief => brief_schema(),
        SchemaDocument::Discovery => schema_for::<DiscoveryReport>(),
        SchemaDocument::Receipt => schema_for::<Receipt>(),
        SchemaDocument::Progress => schema_for::<ProgressEvent>(),
        SchemaDocument::Verification => schema_for::<VerificationReport>(),
        SchemaDocument::Error => schema_for::<ErrorDocument>(),
    }
}

fn schema_for<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("JSON Schema is serializable")
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum SchemaDocument {
    Brief,
    Discovery,
    Receipt,
    Progress,
    Verification,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_document_schema_serializes() {
        for document in [
            SchemaDocument::Brief,
            SchemaDocument::Discovery,
            SchemaDocument::Receipt,
            SchemaDocument::Progress,
            SchemaDocument::Verification,
            SchemaDocument::Error,
        ] {
            let schema = document_schema(document);
            assert!(schema.is_object());
        }
    }
}
