use std::{
    ffi::OsStr,
    path::{Component, Path, PathBuf},
    sync::mpsc,
};

use gitrust_core::workspaces::TransferProgress;
use url::Url;

use crate::{GitError, ReferenceName, command::has_nul, model::allowed_path};

/// An explicitly requested initialization in a new directory.
#[derive(Clone, Debug)]
pub struct CreateRepository {
    pub(crate) path: PathBuf,
    pub(crate) branch: String,
}

impl CreateRepository {
    /// Validate a destination and an initial local branch.
    ///
    /// # Errors
    /// Rejects malformed destinations and branch names.
    pub fn new(path: PathBuf, branch: String) -> Result<Self, GitError> {
        validate_destination(&path)?;
        if branch.starts_with('-') || matches!(branch.as_str(), "HEAD" | "@") || branch.len() > 255
        {
            return Err(GitError::InvalidArgument("initial branch"));
        }
        ReferenceName::new(format!("refs/heads/{branch}"))?;
        Ok(Self { path, branch })
    }
}

/// A public HTTPS repository URL with embedded credentials and redirects forbidden.
#[derive(Clone)]
pub struct CloneSource(Url);

impl std::fmt::Debug for CloneSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CloneSource(<private HTTPS URL>)")
    }
}

impl CloneSource {
    /// Validate a public HTTPS clone URL.
    ///
    /// # Errors
    /// Rejects unsupported protocols, credentials, query strings and fragments.
    pub fn new(input: &str) -> Result<Self, GitError> {
        let rest = input
            .strip_prefix("https://")
            .ok_or(GitError::InvalidArgument("HTTPS clone URL"))?;
        if input.len() > 4096
            || input
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace() || b == b'\\')
            || rest
                .split('/')
                .next()
                .is_none_or(|authority| authority.is_empty() || authority.contains('@'))
        {
            return Err(GitError::InvalidArgument("HTTPS clone URL"));
        }
        let parsed = Url::parse(input).map_err(|_| GitError::InvalidArgument("HTTPS clone URL"))?;
        if parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() == "/"
        {
            return Err(GitError::InvalidArgument("HTTPS clone URL"));
        }
        Ok(Self(parsed))
    }

    /// Suggest one safe directory name from the last path component.
    #[must_use]
    pub fn suggested_directory(&self) -> Option<&str> {
        let path = self.0.path().trim_end_matches('/');
        let name = path.rsplit('/').next()?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        (!name.is_empty()
            && !matches!(name, "." | "..")
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
        .then_some(name)
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// An explicitly requested public clone into a new directory.
#[derive(Clone, Debug)]
pub struct CloneRepository {
    pub(crate) path: PathBuf,
    pub(crate) source: CloneSource,
    pub(crate) progress: Option<mpsc::SyncSender<TransferProgress>>,
}

impl CloneRepository {
    /// Bind a validated HTTPS source to a new directory.
    ///
    /// # Errors
    /// Rejects an invalid destination.
    pub fn new(path: PathBuf, source: CloneSource) -> Result<Self, GitError> {
        validate_destination(&path)?;
        Ok(Self {
            path,
            source,
            progress: None,
        })
    }

    /// Supply a bounded observer for recognized Git progress. Slow observers
    /// may miss intermediate values and cannot block the subprocess.
    #[must_use]
    pub fn with_progress(mut self, progress: mpsc::SyncSender<TransferProgress>) -> Self {
        self.progress = Some(progress);
        self
    }
}

fn validate_destination(path: &Path) -> Result<(), GitError> {
    let name = path
        .file_name()
        .ok_or(GitError::InvalidArgument("new repository directory"))?;
    if !path.is_absolute()
        || !allowed_path(path)
        || has_nul(path.as_os_str())
        || path
            .components()
            .any(|component| component == Component::ParentDir)
        || !valid_name(name)
    {
        return Err(GitError::InvalidArgument("new repository directory"));
    }
    Ok(())
}

fn valid_name(name: &OsStr) -> bool {
    let bytes = name.as_encoded_bytes();
    #[cfg(windows)]
    {
        if bytes.last().is_some_and(|byte| matches!(byte, b'.' | b' '))
            || bytes
                .iter()
                .any(|byte| *byte < b' ' || b"<>:\"|?*".contains(byte))
        {
            return false;
        }
        let stem = bytes.split(|&byte| byte == b'.').next().unwrap_or_default();
        if [
            b"CON".as_slice(),
            b"PRN",
            b"AUX",
            b"NUL",
            b"CONIN$",
            b"CONOUT$",
        ]
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
            || stem.len() == 4
                && (stem[..3].eq_ignore_ascii_case(b"COM")
                    || stem[..3].eq_ignore_ascii_case(b"LPT"))
                && stem[3].is_ascii_digit()
        {
            return false;
        }
    }
    !bytes.eq_ignore_ascii_case(b".git")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn clone_source_accepts_only_an_explicit_public_https_url() {
        for value in [
            "http://example.org/repo",
            "ssh://example.org/repo",
            "file:///tmp/repo",
            "--quiet",
            "https://user@example.org/repo",
            "https://@example.org/repo",
            "https://example.org/repo?credential=hidden",
            "https://example.org/repo#section",
            "https://example.org/",
            "https:///example.org/repo",
            "https://example.org/\nrepo",
        ] {
            assert!(CloneSource::new(value).is_err(), "{value}");
        }
        let source = CloneSource::new("https://example.org/group/repo.git").expect("HTTPS source");
        assert_eq!(source.suggested_directory(), Some("repo"));
        assert!(!format!("{source:?}").contains("example"));
    }
}
