use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::TaskError;
use crate::git::GitContext;
use crate::model::Receipt;
use crate::source::{sha256_path, write_json_atomic};

const MAX_RECEIPT_BYTES: u64 = 4 * 1024 * 1024;
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct StateStore {
    root: PathBuf,
}

impl StateStore {
    pub fn create(git: &GitContext, override_path: Option<&Path>) -> Result<Self, TaskError> {
        let root = override_path
            .map(Path::to_path_buf)
            .unwrap_or_else(|| git.git_dir.join("taskattest"));
        std::fs::create_dir_all(root.join("receipts"))
            .map_err(|error| TaskError::io("create receipt store", &root, error))?;
        std::fs::create_dir_all(root.join("blobs").join("sha256"))
            .map_err(|error| TaskError::io("create blob store", &root, error))?;
        std::fs::create_dir_all(root.join("tmp"))
            .map_err(|error| TaskError::io("create temporary store", &root, error))?;
        let root = root
            .canonicalize()
            .map_err(|error| TaskError::io("resolve state store", &root, error))?;
        Ok(Self { root })
    }

    pub fn open_existing(path: &Path) -> Result<Self, TaskError> {
        let root = path
            .canonicalize()
            .map_err(|error| TaskError::io("resolve state store", path, error))?;
        if !root.join("receipts").is_dir() || !root.join("blobs").join("sha256").is_dir() {
            return Err(TaskError::receipt(format!(
                "not a TaskAttest state store: {}",
                root.display()
            )));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn receipt_path(&self, receipt_id: &str) -> Result<PathBuf, TaskError> {
        validate_receipt_id(receipt_id)?;
        Ok(self
            .root
            .join("receipts")
            .join(format!("{receipt_id}.json")))
    }

    pub fn write_receipt(&self, receipt: &Receipt) -> Result<PathBuf, TaskError> {
        let path = self.receipt_path(&receipt.receipt_id)?;
        let serialized_bytes = serde_json::to_vec_pretty(receipt)?.len().saturating_add(1);
        if u64::try_from(serialized_bytes).unwrap_or(u64::MAX) > MAX_RECEIPT_BYTES {
            return Err(TaskError::receipt(
                "receipt exceeds the 4 MiB persistence safety bound",
            ));
        }
        if path.exists() {
            let existing = self.read_receipt_path(&path)?;
            if existing.canonical_digest == receipt.canonical_digest {
                return Ok(path);
            }
            return Err(TaskError::receipt(format!(
                "receipt id collision at {}",
                path.display()
            )));
        }
        write_json_atomic(&path, receipt)?;
        Ok(path)
    }

    pub fn read_receipt(&self, id_or_path: &str) -> Result<(Receipt, PathBuf), TaskError> {
        let candidate = Path::new(id_or_path);
        let path = if candidate.components().count() > 1
            || candidate
                .extension()
                .is_some_and(|extension| extension == "json")
        {
            candidate.to_path_buf()
        } else {
            self.receipt_path(id_or_path)?
        };
        let receipt = self.read_receipt_path(&path)?;
        Ok((receipt, path))
    }

    pub fn read_receipt_path(&self, path: &Path) -> Result<Receipt, TaskError> {
        Self::read_receipt_file(path)
    }

    pub fn read_receipt_file(path: &Path) -> Result<Receipt, TaskError> {
        let metadata = std::fs::metadata(path)
            .map_err(|error| TaskError::io("inspect receipt", path, error))?;
        if metadata.len() > MAX_RECEIPT_BYTES {
            return Err(TaskError::receipt(format!(
                "receipt exceeds the 4 MiB safety bound: {}",
                path.display()
            )));
        }
        let file = File::open(path).map_err(|error| TaskError::io("open receipt", path, error))?;
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        file.take(MAX_RECEIPT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| TaskError::io("read receipt", path, error))?;
        let document: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(TaskError::from)?;
        let receipt: Receipt = serde_json::from_value(document.clone()).map_err(TaskError::from)?;
        let normalized = serde_json::to_value(&receipt)?;
        if document != normalized {
            return Err(TaskError::receipt(
                "receipt contains unknown or omitted fields",
            ));
        }
        Ok(receipt)
    }

    pub fn create_capture_file(&self, stream: &str) -> Result<(PathBuf, File), TaskError> {
        for _ in 0..100 {
            let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = self.root.join("tmp").join(format!(
                "{}-{}-{}.log",
                std::process::id(),
                sequence,
                stream
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((path, file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(TaskError::io("create temporary log", &path, error));
                }
            }
        }
        Err(TaskError::execution(
            "could not allocate a unique temporary log file",
        ))
    }

    pub fn store_blob(
        &self,
        temporary: &Path,
        digest: &str,
        expected_bytes: u64,
    ) -> Result<PathBuf, TaskError> {
        validate_sha256(digest)?;
        let target = self.root.join("blobs").join("sha256").join(digest);
        if target.exists() {
            verify_existing_blob(&target, digest, expected_bytes)?;
            std::fs::remove_file(temporary).map_err(|error| {
                TaskError::io("remove duplicate temporary log", temporary, error)
            })?;
            return Ok(target);
        }
        match std::fs::hard_link(temporary, &target) {
            Ok(()) => {
                std::fs::remove_file(temporary).map_err(|error| {
                    TaskError::io("remove published temporary log", temporary, error)
                })?;
                Ok(target)
            }
            Err(_error) if target.exists() => {
                verify_existing_blob(&target, digest, expected_bytes)?;
                std::fs::remove_file(temporary).map_err(|remove_error| {
                    TaskError::io("remove raced temporary log", temporary, remove_error)
                })?;
                Ok(target)
            }
            Err(error) => Err(TaskError::io(
                "publish content-addressed log without overwriting",
                &target,
                error,
            )),
        }
    }

    pub fn blob_path(&self, digest: &str) -> Result<PathBuf, TaskError> {
        validate_sha256(digest)?;
        Ok(self.root.join("blobs").join("sha256").join(digest))
    }
}

fn verify_existing_blob(path: &Path, digest: &str, expected_bytes: u64) -> Result<(), TaskError> {
    let metadata =
        std::fs::metadata(path).map_err(|error| TaskError::io("inspect blob", path, error))?;
    if metadata.len() != expected_bytes || sha256_path(path)? != digest {
        return Err(TaskError::receipt(format!(
            "content-addressed blob does not match its handle: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn validate_receipt_id(value: &str) -> Result<(), TaskError> {
    let digest = value
        .strip_prefix("rcpt_")
        .ok_or_else(|| TaskError::receipt("receipt id must start with rcpt_"))?;
    if digest.len() != 32 || !digest.bytes().all(is_lower_hex_byte) {
        return Err(TaskError::receipt(
            "receipt id must end with 32 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

pub fn validate_sha256(value: &str) -> Result<(), TaskError> {
    if value.len() != 64 || !value.bytes().all(is_lower_hex_byte) {
        return Err(TaskError::receipt(
            "SHA-256 digest must contain 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn is_lower_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}
