use std::{fmt, io, process::ExitStatus, time::Duration};

pub(crate) const MAX_RECORDS: usize = 250_000;

/// A complete SHA-1 or SHA-256 object ID.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObjectId(String);

impl ObjectId {
    /// Accept a complete hexadecimal ID and normalize it to lowercase.
    ///
    /// # Errors
    /// Returns an invalid reference error for an incomplete or malformed ID.
    pub fn new(value: &str) -> Result<Self, GitError> {
        if !matches!(value.len(), 40 | 64) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(GitError::InvalidReference);
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    pub(crate) fn parse(value: &[u8]) -> Result<Self, GitError> {
        std::str::from_utf8(value)
            .ok()
            .and_then(|s| Self::new(s).ok())
            .ok_or(GitError::MalformedOutput("object ID"))
    }

    /// Complete ID suitable for passing back to Git.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Fixed-length display label. It is not a unique revision selector.
    #[must_use]
    pub fn abbreviated(&self) -> &str {
        &self.0[..12]
    }
}

/// A Git filename as reported by a byte-oriented Git format.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GitPath(Vec<u8>);

impl GitPath {
    pub(crate) fn parse(value: &[u8]) -> Result<Self, GitError> {
        // Porcelain status may append a slash to a directory entry.
        let relative = value.strip_suffix(b"/").unwrap_or(value);
        if value.contains(&0)
            || relative
                .split(|&byte| byte == b'/')
                .any(|part| matches!(part, b"" | b"." | b".."))
        {
            return Err(GitError::MalformedOutput(
                "invalid repository-relative Git path",
            ));
        }
        Ok(Self(value.to_vec()))
    }

    /// Original Git bytes, without display decoding.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convert on Unix without losing bytes. On Windows, Git for Windows
    /// names must be valid UTF-8 and may not contain native separator or
    /// drive/stream syntax within a Git path component.
    ///
    /// # Errors
    /// Returns a malformed-output error for paths unrepresentable on the host.
    pub fn to_path_buf(&self) -> Result<std::path::PathBuf, GitError> {
        #[cfg(windows)]
        if self.0.iter().any(|byte| matches!(byte, b'\\' | b':')) {
            return Err(GitError::MalformedOutput(
                "Git path has Windows-specific syntax",
            ));
        }
        path_from_bytes(&self.0)
    }
}

pub(crate) fn path_from_bytes(bytes: &[u8]) -> Result<std::path::PathBuf, GitError> {
    if bytes.contains(&0) {
        return Err(GitError::MalformedOutput("NUL-containing path"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
    }
    #[cfg(not(unix))]
    {
        std::str::from_utf8(bytes)
            .map(std::path::PathBuf::from)
            .map_err(|_| GitError::MalformedOutput("path encoding"))
    }
}

/// A validated Git reference name, with its original byte encoding.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ReferenceName(Vec<u8>);

impl ReferenceName {
    /// Validate a full Git reference such as `refs/heads/main`.
    ///
    /// # Errors
    /// Rejects revision expressions, option-like strings and invalid ref syntax.
    pub fn new(bytes: impl AsRef<[u8]>) -> Result<Self, GitError> {
        let bytes = bytes.as_ref();
        if !bytes.starts_with(b"refs/") || !valid_ref_tail(&bytes[5..]) {
            return Err(GitError::InvalidReference);
        }
        Ok(Self(bytes.to_vec()))
    }

    pub(crate) fn short(bytes: &[u8]) -> Result<Self, GitError> {
        if !valid_ref_tail(bytes) {
            return Err(GitError::MalformedOutput("reference name"));
        }
        Ok(Self(bytes.to_vec()))
    }

    /// Reference bytes, without display decoding.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn full(bytes: &[u8]) -> Result<Self, GitError> {
        Self::new(bytes).map_err(|_| GitError::MalformedOutput("full reference"))
    }
}

fn valid_ref_tail(bytes: &[u8]) -> bool {
    !bytes.is_empty()
        && bytes != b"@"
        && !bytes.ends_with(b".")
        && !bytes.windows(2).any(|pair| pair == b".." || pair == b"@{")
        && !bytes
            .iter()
            .any(|b| *b <= b' ' || *b == 127 || b"~^:?*[\\".contains(b))
        && bytes
            .split(|&b| b == b'/')
            .all(|part| !part.is_empty() && !part.starts_with(b".") && !part.ends_with(b".lock"))
}

/// The current repository checkout state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeadState {
    /// HEAD points to a branch with a commit.
    Attached(ReferenceName, ObjectId),
    /// HEAD is a commit rather than a branch.
    Detached(ObjectId),
    /// HEAD names a branch whose first commit has not been created.
    Unborn(ReferenceName),
}

/// One side of an index, worktree or diff change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileChange {
    /// No change.
    Unchanged,
    /// New entry.
    Added,
    /// Content or mode changed.
    Modified,
    /// Entry removed.
    Deleted,
    /// Entry moved to a different location.
    Renamed,
    /// Entry copied.
    Copied,
    /// File type changed.
    TypeChanged,
    /// Merge conflict.
    Unmerged,
    /// Git reported an unknown change.
    Unknown,
    /// Pairing was broken.
    Broken,
}

impl FileChange {
    pub(crate) fn parse(byte: u8) -> Result<Self, GitError> {
        match byte {
            b'.' => Ok(Self::Unchanged),
            b'A' => Ok(Self::Added),
            b'M' => Ok(Self::Modified),
            b'D' => Ok(Self::Deleted),
            b'R' => Ok(Self::Renamed),
            b'C' => Ok(Self::Copied),
            b'T' => Ok(Self::TypeChanged),
            b'U' => Ok(Self::Unmerged),
            b'X' => Ok(Self::Unknown),
            b'B' => Ok(Self::Broken),
            _ => Err(GitError::MalformedOutput("change status")),
        }
    }
}

/// One of the two independently bounded output streams.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputStream {
    /// Normal output.
    Stdout,
    /// Diagnostics from Git.
    Stderr,
}

/// Raw bytes and timing for one authorized Git process. Debug formatting
/// reports stream sizes only. Callers should not log these byte buffers.
pub struct ProcessOutput {
    /// Exit information, including platforms where no numeric exit code exists.
    pub status: ExitStatus,
    /// Time from before spawning to exit and output drainage.
    pub duration: Duration,
    /// Unmodified output bytes.
    pub stdout: Vec<u8>,
    /// Unmodified diagnostics.
    pub stderr: Vec<u8>,
}

impl fmt::Debug for ProcessOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProcessOutput")
            .field("status", &self.status)
            .field("duration", &self.duration)
            .field("stdout_len", &self.stdout.len())
            .field("stderr_len", &self.stderr.len())
            .finish()
    }
}

/// Structured errors returned by the Git engine.
#[derive(Debug)]
pub enum GitError {
    /// No candidate found in absolute PATH entries.
    GitNotFound,
    /// The selected file is not an eligible Git executable.
    InvalidExecutable,
    /// Git did not meet the supported version floor.
    UnsupportedGitVersion(crate::GitVersion),
    /// The requested directory does not identify a repository.
    InvalidRepository,
    /// A new repository destination is already occupied.
    DestinationExists,
    /// Its metadata was not authorized for inspection or Git rejected ownership.
    RepositoryNotTrusted,
    /// Access was denied by the operating system.
    PermissionDenied,
    /// Invalid reference name or object ID.
    InvalidReference,
    /// Invalid public argument or executor option.
    InvalidArgument(&'static str),
    /// A Git command returned a non-success status. Bytes are available for
    /// private diagnostics and the display representation omits them.
    CommandFailed(Box<ProcessOutput>),
    /// Git or one of the executor's workers could not start.
    SpawnFailed(io::Error),
    /// The task exceeded its active deadline.
    Timeout,
    /// The task was cancelled.
    Cancelled,
    /// Git exceeded a stream's output budget.
    OutputLimitExceeded(OutputStream),
    /// A decoded model exceeded its item budget.
    ParsedLimitExceeded(&'static str),
    /// A Git response did not match its documented grammar.
    MalformedOutput(&'static str),
    /// A repository lock was present. The engine did not remove it.
    RepositoryLocked,
    /// The repository uses a feature outside the Stage 02 policy.
    UnsupportedRepositoryFeature(&'static str),
    /// The metadata violates the local-only inspection policy.
    SecurityPolicyViolation(&'static str),
    /// The bounded queue cannot accept another task.
    QueueFull,
    /// The executor has shut down.
    ExecutorClosed,
    /// A filesystem or stream operation failed.
    Io(io::Error),
}

impl GitError {
    pub(crate) fn io(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::PermissionDenied {
            Self::PermissionDenied
        } else {
            Self::Io(error)
        }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GitNotFound => f.write_str("Git not found in absolute PATH directories"),
            Self::InvalidExecutable => f.write_str("invalid Git executable"),
            Self::UnsupportedGitVersion(version) => write!(f, "unsupported Git version {version}"),
            Self::InvalidRepository => f.write_str("invalid Git repository"),
            Self::DestinationExists => f.write_str("repository destination already exists"),
            Self::RepositoryNotTrusted => {
                f.write_str("Git repository ownership or trust was rejected")
            }
            Self::PermissionDenied => f.write_str("permission denied"),
            Self::InvalidReference => f.write_str("invalid Git reference or object ID"),
            Self::InvalidArgument(name) => write!(f, "invalid argument: {name}"),
            Self::CommandFailed(output) => write!(f, "Git command failed ({})", output.status),
            Self::SpawnFailed(error) => write!(f, "process or worker could not start: {error}"),
            Self::Timeout => f.write_str("Git operation timed out"),
            Self::Cancelled => f.write_str("Git operation cancelled"),
            Self::OutputLimitExceeded(stream) => write!(f, "Git exceeded its {stream:?} budget"),
            Self::ParsedLimitExceeded(items) => {
                write!(f, "Git exceeded the decoded {items} budget")
            }
            Self::MalformedOutput(kind) => write!(f, "malformed Git output: {kind}"),
            Self::RepositoryLocked => f.write_str("Git repository is locked"),
            Self::UnsupportedRepositoryFeature(feature) => {
                write!(f, "unsupported repository feature: {feature}")
            }
            Self::SecurityPolicyViolation(rule) => {
                write!(f, "repository blocked by security policy: {rule}")
            }
            Self::QueueFull => f.write_str("Git task queue is full"),
            Self::ExecutorClosed => f.write_str("Git executor is closed"),
            Self::Io(error) => write!(f, "Git input or output failed: {error}"),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) | Self::SpawnFailed(error) => Some(error),
            _ => None,
        }
    }
}

pub(crate) fn decimal<T: std::str::FromStr>(
    value: &[u8],
    context: &'static str,
) -> Result<T, GitError> {
    let digits = value.strip_prefix(b"-").unwrap_or(value);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(GitError::MalformedOutput(context));
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or(GitError::MalformedOutput(context))
}

pub(crate) fn mode(value: &[u8]) -> Result<u32, GitError> {
    if value.len() != 6 || !value.iter().all(|byte| (b'0'..=b'7').contains(byte)) {
        return Err(GitError::MalformedOutput("file mode"));
    }
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| u32::from_str_radix(value, 8).ok())
        .ok_or(GitError::MalformedOutput("file mode"))
}

pub(crate) fn strip_line_end(bytes: &[u8]) -> Result<&[u8], GitError> {
    let line = bytes
        .strip_suffix(b"\n")
        .ok_or(GitError::MalformedOutput("unterminated line"))?;
    #[cfg(windows)]
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    Ok(line)
}

pub(crate) fn records(bytes: &[u8]) -> Result<impl Iterator<Item = &[u8]>, GitError> {
    let take = if bytes.is_empty() { 0 } else { usize::MAX };
    let bytes = if bytes.is_empty() {
        bytes
    } else {
        bytes
            .strip_suffix(&[0])
            .ok_or(GitError::MalformedOutput("unterminated record"))?
    };
    Ok(bytes.split(|&byte| byte == 0).take(take))
}

pub(crate) fn split_once(bytes: &[u8], separator: u8) -> Option<(&[u8], &[u8])> {
    let at = bytes.iter().position(|&byte| byte == separator)?;
    Some((&bytes[..at], &bytes[at + 1..]))
}

pub(crate) fn allowed_path(path: &std::path::Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        match path.components().next() {
            Some(Component::Prefix(prefix)) => {
                matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
            }
            _ => true,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        true
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn git_paths_are_relative_and_allow_directory_markers() {
        for bad in [
            b"/absolute".as_slice(),
            b"../parent",
            b"dir/../outside",
            b"dir/./file",
            b"dir//file",
        ] {
            assert!(matches!(
                GitPath::parse(bad),
                Err(GitError::MalformedOutput(_))
            ));
        }
        let dir = GitPath::parse(b"ignored folder/").expect("Git directory marker");
        assert_eq!(dir.as_bytes(), b"ignored folder/");
    }

    #[cfg(windows)]
    #[test]
    fn windows_conversion_keeps_git_names_from_becoming_native_path_syntax() {
        for raw in [b"dir\\child".as_slice(), b"disk:stream", b"C:/drive-path"] {
            let path = GitPath::parse(raw).expect("raw Git name");
            assert!(matches!(
                path.to_path_buf(),
                Err(GitError::MalformedOutput(_))
            ));
        }
    }
}
