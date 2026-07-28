use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::error::TaskError;
use crate::git::GitContext;
use crate::hex::encode_lower;
use crate::model::SourceIdentity;

const HASH_BUFFER_BYTES: usize = 64 * 1024;
static JSON_TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn identify_source(git: &GitContext) -> Result<SourceIdentity, TaskError> {
    let commit = git.optional_text(&["rev-parse", "--verify", "HEAD"])?;
    let git_ref = git.optional_text(&["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let status = git.bytes(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
    let dirty = !status.is_empty();
    let status_sha256 = sha256_bytes(&status);
    let changed_paths = git.changed_paths(commit.is_some())?;
    let files = git.workspace_files()?;
    let workspace_sha256 = hash_workspace(git, &files)?;

    Ok(SourceIdentity {
        git_commit: commit,
        git_ref,
        dirty,
        status_sha256,
        workspace_sha256,
        workspace_file_count: u64::try_from(files.len()).unwrap_or(u64::MAX),
        changed_paths,
    })
}

pub fn sha256_path(path: &Path) -> Result<String, TaskError> {
    let mut file =
        File::open(path).map_err(|error| TaskError::io("open file for hashing", path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| TaskError::io("read file for hashing", path, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(encode_lower(hasher.finalize()))
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    encode_lower(hasher.finalize())
}

fn hash_workspace(git: &GitContext, paths: &[String]) -> Result<String, TaskError> {
    let mut hasher = Sha256::new();
    hasher.update(b"taskattest-workspace-v1\0");
    for relative in paths {
        update_length_prefixed(&mut hasher, relative.as_bytes());
        let path = git.root.join(relative);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| TaskError::io("inspect workspace path", &path, error))?;
        if metadata.file_type().is_symlink() {
            hasher.update(b"symlink\0");
            let target = std::fs::read_link(&path)
                .map_err(|error| TaskError::io("read symbolic link", &path, error))?;
            let target = target.to_str().ok_or_else(|| {
                TaskError::git(format!(
                    "symbolic-link target is not valid UTF-8: {}",
                    path.display()
                ))
            })?;
            update_length_prefixed(&mut hasher, target.as_bytes());
        } else if metadata.is_file() {
            hasher.update(b"file\0");
            hash_file_into(&path, &mut hasher)?;
            hash_executable_bit(&metadata, &mut hasher);
        } else if metadata.is_dir() {
            hasher.update(b"submodule\0");
            hash_submodule(&path, &mut hasher)?;
        } else {
            return Err(TaskError::git(format!(
                "unsupported workspace file type: {}",
                path.display()
            )));
        }
    }
    Ok(encode_lower(hasher.finalize()))
}

fn hash_file_into(path: &Path, hasher: &mut Sha256) -> Result<(), TaskError> {
    let mut file =
        File::open(path).map_err(|error| TaskError::io("open workspace file", path, error))?;
    let mut buffer = [0_u8; HASH_BUFFER_BYTES];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| TaskError::io("read workspace file", path, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(())
}

#[cfg(unix)]
fn hash_executable_bit(metadata: &std::fs::Metadata, hasher: &mut Sha256) {
    use std::os::unix::fs::PermissionsExt;

    hasher.update([u8::from(metadata.permissions().mode() & 0o111 != 0)]);
}

#[cfg(not(unix))]
fn hash_executable_bit(_metadata: &std::fs::Metadata, hasher: &mut Sha256) {
    hasher.update([0]);
}

fn hash_submodule(path: &Path, hasher: &mut Sha256) -> Result<(), TaskError> {
    for args in [
        &["rev-parse", "--verify", "HEAD"][..],
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"][..],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .map_err(|error| {
                TaskError::git(format!("inspect submodule {}: {error}", path.display()))
            })?;
        if !output.status.success() {
            return Err(TaskError::git(format!(
                "workspace entry is a directory but not a readable submodule: {}",
                path.display()
            )));
        }
        update_length_prefixed(hasher, &output.stdout);
    }
    Ok(())
}

fn update_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
    hasher.update(bytes);
}

pub fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), TaskError> {
    let parent = path.parent().ok_or_else(|| {
        TaskError::execution(format!("output path has no parent: {}", path.display()))
    })?;
    std::fs::create_dir_all(parent)
        .map_err(|error| TaskError::io("create output directory", parent, error))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            TaskError::execution(format!(
                "output path has no UTF-8 file name: {}",
                path.display()
            ))
        })?;
    let mut allocated = None;
    for _ in 0..100 {
        let sequence = JSON_TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{sequence}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                allocated = Some((temporary, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(TaskError::io(
                    "create temporary JSON file",
                    &temporary,
                    error,
                ));
            }
        }
    }
    let (temporary, mut file) = allocated
        .ok_or_else(|| TaskError::execution("could not allocate a unique temporary JSON file"))?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, value)?;
        file.write_all(b"\n")
            .map_err(|error| TaskError::io("finish temporary JSON file", &temporary, error))?;
        file.sync_all()
            .map_err(|error| TaskError::io("sync temporary JSON file", &temporary, error))?;
        std::fs::hard_link(&temporary, path)
            .map_err(|error| TaskError::io("publish JSON file without overwriting", path, error))?;
        std::fs::remove_file(&temporary)
            .map_err(|error| TaskError::io("remove temporary JSON file", &temporary, error))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(directory: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn source_identity_changes_with_tracked_and_untracked_content() {
        let directory = tempfile::tempdir().expect("temporary repository");
        git(directory.path(), &["init", "--quiet"]);
        git(
            directory.path(),
            &["config", "user.email", "taskattest@example.invalid"],
        );
        git(
            directory.path(),
            &["config", "user.name", "TaskAttest Test"],
        );
        std::fs::write(directory.path().join("tracked.txt"), "one\n").expect("write tracked file");
        git(directory.path(), &["add", "."]);
        git(directory.path(), &["commit", "--quiet", "-m", "fixture"]);
        let context = GitContext::discover(directory.path()).expect("discover repository");
        let clean = identify_source(&context).expect("identify clean source");
        assert!(!clean.dirty);
        assert!(clean.changed_paths.is_empty());

        std::fs::write(directory.path().join("tracked.txt"), "two\n").expect("modify tracked file");
        std::fs::write(directory.path().join("untracked.txt"), "three\n")
            .expect("write untracked file");
        let dirty = identify_source(&context).expect("identify dirty source");
        assert!(dirty.dirty);
        assert_ne!(clean.status_sha256, dirty.status_sha256);
        assert_ne!(clean.workspace_sha256, dirty.workspace_sha256);
        assert_eq!(
            dirty.changed_paths,
            vec!["tracked.txt".to_owned(), "untracked.txt".to_owned()]
        );
    }

    #[test]
    fn atomic_json_publish_never_overwrites_an_existing_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("receipt.json");
        std::fs::write(&path, "original\n").expect("write existing file");
        assert!(write_json_atomic(&path, &serde_json::json!({"new": true})).is_err());
        assert_eq!(
            std::fs::read_to_string(path).expect("read existing file"),
            "original\n"
        );
    }
}
