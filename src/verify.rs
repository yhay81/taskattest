use std::collections::BTreeSet;

use crate::RECEIPT_SCHEMA_VERSION;
use crate::VERIFICATION_SCHEMA_VERSION;
use crate::error::TaskError;
use crate::model::{
    BlobVerification, CheckOutcome, LogReference, Receipt, ReceiptOutcome, VerificationReport,
};
use crate::source::{sha256_bytes, sha256_path};
use crate::store::{StateStore, validate_receipt_id, validate_sha256};

pub fn verify_receipt(
    receipt: &Receipt,
    store: &StateStore,
) -> Result<VerificationReport, TaskError> {
    let mut problems = Vec::new();
    let schema_supported = receipt.payload.schema_version == RECEIPT_SCHEMA_VERSION;
    if !schema_supported {
        problems.push(format!(
            "unsupported receipt schema {}; expected {}",
            receipt.payload.schema_version, RECEIPT_SCHEMA_VERSION
        ));
    }

    let canonical_digest = sha256_bytes(&serde_json::to_vec(&receipt.payload)?);
    let canonical_digest_matches = canonical_digest == receipt.canonical_digest;
    if !canonical_digest_matches {
        problems.push("canonical receipt digest does not match the payload".to_owned());
    }
    let expected_id = format!("rcpt_{}", &canonical_digest[..32]);
    let receipt_id_matches =
        validate_receipt_id(&receipt.receipt_id).is_ok() && receipt.receipt_id == expected_id;
    if !receipt_id_matches {
        problems.push("receipt id is not derived from the canonical payload digest".to_owned());
    }
    if validate_sha256(&receipt.canonical_digest).is_err() {
        problems.push("canonical digest is not lowercase SHA-256".to_owned());
    }

    verify_semantics(receipt, &mut problems)?;
    let mut blobs = Vec::new();
    for reference in log_references(receipt) {
        let verification = verify_blob(reference, store)?;
        if !verification.found {
            problems.push(format!(
                "referenced log blob is missing: {}",
                reference.handle
            ));
        } else if !verification.digest_matches {
            problems.push(format!(
                "referenced log blob does not match: {}",
                reference.handle
            ));
        }
        blobs.push(verification);
    }

    let valid =
        schema_supported && canonical_digest_matches && receipt_id_matches && problems.is_empty();
    Ok(VerificationReport {
        schema_version: VERIFICATION_SCHEMA_VERSION.to_owned(),
        receipt_id: receipt.receipt_id.clone(),
        valid,
        schema_supported,
        canonical_digest_matches,
        receipt_id_matches,
        blobs,
        problems,
    })
}

fn verify_semantics(receipt: &Receipt, problems: &mut Vec<String>) -> Result<(), TaskError> {
    if serde_json::to_value(&receipt.payload.source)?
        != serde_json::to_value(&receipt.payload.discovery.source)?
    {
        problems.push("receipt source and discovery source differ".to_owned());
    }
    if receipt.payload.source_unchanged != (receipt.payload.source == receipt.payload.source_after)
    {
        problems.push("source_unchanged is inconsistent with the source identities".to_owned());
    }
    if receipt.payload.completed_unix_ms < receipt.payload.started_unix_ms {
        problems.push("receipt completion precedes its start".to_owned());
    }
    let selected: BTreeSet<_> = receipt
        .payload
        .discovery
        .selection
        .iter()
        .filter(|selection| selection.selected)
        .map(|selection| selection.check_id.as_str())
        .collect();
    let executed: BTreeSet<_> = receipt
        .payload
        .checks
        .iter()
        .map(|execution| execution.check.id.as_str())
        .collect();
    if selected != executed {
        problems.push("executed checks do not equal the selected discovery set".to_owned());
    }
    if executed.len() != receipt.payload.checks.len() {
        problems.push("receipt contains duplicate executed check ids".to_owned());
    }
    for execution in &receipt.payload.checks {
        if execution.completed_unix_ms < execution.started_unix_ms {
            problems.push(format!(
                "check {} completes before it starts",
                execution.check.id
            ));
        }
        for reference in [&execution.stdout, &execution.stderr].into_iter().flatten() {
            if reference.algorithm != "sha256"
                || reference.handle != format!("sha256:{}", reference.digest)
                || validate_sha256(&reference.digest).is_err()
            {
                problems.push(format!(
                    "check {} has an invalid log reference",
                    execution.check.id
                ));
            }
        }
    }

    let any_cancelled = receipt
        .payload
        .checks
        .iter()
        .any(|check| matches!(check.outcome, CheckOutcome::Cancelled));
    let any_failed = !receipt.payload.source_unchanged
        || receipt
            .payload
            .checks
            .iter()
            .any(|check| !matches!(check.outcome, CheckOutcome::Passed));
    let incomplete = receipt.payload.checks.is_empty() || !receipt.payload.coverage_gaps.is_empty();
    let expected = if any_cancelled {
        ReceiptOutcome::Cancelled
    } else if any_failed {
        ReceiptOutcome::Failed
    } else if incomplete {
        ReceiptOutcome::Incomplete
    } else {
        ReceiptOutcome::Passed
    };
    if std::mem::discriminant(&receipt.payload.outcome) != std::mem::discriminant(&expected) {
        problems.push("receipt outcome is inconsistent with check results and coverage".to_owned());
    }
    Ok(())
}

fn log_references(receipt: &Receipt) -> Vec<&LogReference> {
    receipt
        .payload
        .checks
        .iter()
        .flat_map(|execution| [&execution.stdout, &execution.stderr])
        .flatten()
        .collect()
}

fn verify_blob(
    reference: &LogReference,
    store: &StateStore,
) -> Result<BlobVerification, TaskError> {
    let path = store.blob_path(&reference.digest)?;
    if !path.is_file() {
        return Ok(BlobVerification {
            handle: reference.handle.clone(),
            found: false,
            digest_matches: false,
            expected_bytes: reference.bytes,
            actual_bytes: None,
        });
    }
    let metadata = std::fs::metadata(&path)
        .map_err(|error| TaskError::io("inspect log blob", &path, error))?;
    let actual_bytes = metadata.len();
    let digest_matches = actual_bytes == reference.bytes && sha256_path(&path)? == reference.digest;
    Ok(BlobVerification {
        handle: reference.handle.clone(),
        found: true,
        digest_matches,
        expected_bytes: reference.bytes,
        actual_bytes: Some(actual_bytes),
    })
}
