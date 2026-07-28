use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::TaskError;

const MAX_GIT_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct GitContext {
    pub root: PathBuf,
    pub git_dir: PathBuf,
}

impl GitContext {
    pub fn discover(start: &Path) -> Result<Self, TaskError> {
        let root_text = git_text_at(start, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(root_text.trim())
            .canonicalize()
            .map_err(|error| TaskError::io("resolve Git workspace", start, error))?;
        let git_dir_text = git_text_at(&root, &["rev-parse", "--absolute-git-dir"])?;
        let git_dir = PathBuf::from(git_dir_text.trim());
        if !git_dir.is_absolute() {
            return Err(TaskError::git(format!(
                "Git returned a non-absolute metadata directory: {}",
                git_dir.display()
            )));
        }
        Ok(Self { root, git_dir })
    }

    pub fn bytes(&self, args: &[&str]) -> Result<Vec<u8>, TaskError> {
        git_bytes_at(&self.root, args)
    }

    pub fn optional_text(&self, args: &[&str]) -> Result<Option<String>, TaskError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|error| TaskError::git(format!("start git {}: {error}", args.join(" "))))?;
        if output.stdout.len() > MAX_GIT_OUTPUT_BYTES || output.stderr.len() > MAX_GIT_OUTPUT_BYTES
        {
            return Err(TaskError::git(
                "Git output exceeded the 64 MiB safety bound",
            ));
        }
        if output.status.success() {
            let text = String::from_utf8(output.stdout)
                .map_err(|_| TaskError::git("Git emitted non-UTF-8 text"))?;
            Ok(Some(text.trim().to_owned()))
        } else {
            Ok(None)
        }
    }

    pub fn workspace_files(&self) -> Result<Vec<String>, TaskError> {
        parse_nul_paths(self.bytes(&[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])?)
    }

    pub fn changed_paths(&self, has_head: bool) -> Result<Vec<String>, TaskError> {
        if !has_head {
            return self.workspace_files();
        }
        let mut paths = BTreeSet::new();
        for args in [
            &["diff", "--name-only", "-z", "HEAD"][..],
            &["diff", "--cached", "--name-only", "-z", "HEAD"][..],
            &["ls-files", "--others", "--exclude-standard", "-z"][..],
        ] {
            paths.extend(parse_nul_paths(self.bytes(args)?)?);
        }
        Ok(paths.into_iter().collect())
    }
}

fn git_text_at(directory: &Path, args: &[&str]) -> Result<String, TaskError> {
    String::from_utf8(git_bytes_at(directory, args)?)
        .map_err(|_| TaskError::git("Git emitted non-UTF-8 text"))
}

fn git_bytes_at(directory: &Path, args: &[&str]) -> Result<Vec<u8>, TaskError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .map_err(|error| TaskError::git(format!("start git {}: {error}", args.join(" "))))?;
    if output.stdout.len() > MAX_GIT_OUTPUT_BYTES || output.stderr.len() > MAX_GIT_OUTPUT_BYTES {
        return Err(TaskError::git(
            "Git output exceeded the 64 MiB safety bound",
        ));
    }
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        return Err(TaskError::git(format!(
            "git {} failed with {}: {}",
            args.join(" "),
            output.status,
            diagnostic.trim()
        )));
    }
    Ok(output.stdout)
}

fn parse_nul_paths(bytes: Vec<u8>) -> Result<Vec<String>, TaskError> {
    let mut paths = BTreeSet::new();
    for value in bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
    {
        let path = String::from_utf8(value.to_vec())
            .map_err(|_| TaskError::git("Git path is not valid UTF-8"))?;
        validate_git_path(&path)?;
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

fn validate_git_path(path: &str) -> Result<(), TaskError> {
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    {
        return Err(TaskError::git(format!(
            "Git returned an unsafe workspace path: {path}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_paths() {
        assert!(validate_git_path("../outside").is_err());
        assert!(validate_git_path("src/lib.rs").is_ok());
    }
}
