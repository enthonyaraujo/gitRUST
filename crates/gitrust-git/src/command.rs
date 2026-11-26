use std::{
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
};

use gitrust_core::changes::ChangeKey;

use crate::{CloneRepository, CreateRepository, GitError, Mutation, ObjectId, TrustedRepository};

/// Future mutation classes are reserved for later stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationSafetyClass {
    /// Read-only inspection.
    ReadOnly,
    /// A future operation requiring exclusive repository access.
    SerializedWrite,
    /// A future operation requiring exclusive access and explicit confirmation.
    DestructiveWrite,
}

/// A literal, repository-relative path filter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepoPath(PathBuf);

impl RepoPath {
    /// Construct a literal path without decoding it.
    ///
    /// # Errors
    /// Rejects empty paths, roots, prefixes, parent traversals and NUL bytes.
    pub fn new(path: PathBuf) -> Result<Self, GitError> {
        if path.as_os_str().is_empty()
            || has_nul(path.as_os_str())
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(GitError::InvalidArgument("repository-relative path"));
        }
        Ok(Self(path))
    }

    /// The path in native encoding.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

pub(crate) fn has_nul(text: &OsStr) -> bool {
    text.as_encoded_bytes().contains(&0)
}

/// The two sides of a diff.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffTarget {
    /// Working tree against the index.
    Worktree,
    /// Index against HEAD, including an unborn branch.
    Index,
    /// Two complete commit IDs.
    Between {
        /// Base commit.
        old: ObjectId,
        /// Target commit.
        new: ObjectId,
    },
}

/// An offset and bounded page size for one snapshotted HEAD history walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryPage {
    pub(crate) skip: u32,
    pub(crate) limit: u16,
}

impl HistoryPage {
    /// Bound a page to 1 through 500 commits.
    ///
    /// # Errors
    /// Rejects a zero or excessive page size and overflowing offset.
    pub fn new(skip: u32, limit: u16) -> Result<Self, GitError> {
        if !(1..=500).contains(&limit) || skip.checked_add(u32::from(limit) + 1).is_none() {
            return Err(GitError::InvalidArgument("history page"));
        }
        Ok(Self { skip, limit })
    }
}

/// The complete public allowlist of repository operations.
#[derive(Clone, Debug)]
pub enum Inspection {
    /// Current HEAD, canonical paths and object format.
    Info,
    /// Repository information and basic working tree status, when supported.
    Workspace,
    /// Porcelain v2 status.
    Status {
        /// Include ignored entries.
        include_ignored: bool,
    },
    /// File metadata and textual changes.
    Diff {
        /// The sides being compared.
        target: DiffTarget,
        /// Optional literal path filter.
        path: Option<RepoPath>,
    },
    /// Exact patch bytes and typed hunks for one workspace file.
    Change(ChangeKey),
    /// Capture all paths and content for a discard review.
    DiscardReview(Vec<ChangeKey>),
    /// Local author and, for amend, the current message.
    CommitDraft { amend: bool },
    /// A bounded page of commits rooted at HEAD.
    History(HistoryPage),
    /// Local and remote-tracking references.
    Branches,
}

/// One task accepted by the engine.
#[derive(Clone, Debug)]
pub enum Request {
    /// Locate candidate metadata without launching Git or granting trust.
    Identify(PathBuf),
    /// Run an allowlisted operation on an explicitly trusted candidate.
    Inspect(TrustedRepository, Inspection),
    /// Initialize a newly requested workspace with no custom templates.
    Create(CreateRepository),
    /// Clone explicitly from a public HTTPS source.
    Clone(CloneRepository),
    /// Apply a typed operation to a revalidated workspace.
    Mutate(TrustedRepository, Box<Mutation>),
}

impl Request {
    /// The operation determines its own access class.
    #[must_use]
    pub const fn safety_class(&self) -> OperationSafetyClass {
        match self {
            Self::Identify(_) | Self::Inspect(..) => OperationSafetyClass::ReadOnly,
            Self::Create(_) | Self::Clone(_) => OperationSafetyClass::SerializedWrite,
            Self::Mutate(_, operation) => operation.safety_class(),
        }
    }
}

pub(crate) enum Builtin {
    Add,
    Apply,
    CommitTree,
    Restore,
    Rm,
    UpdateRef,
    WriteTree,
    CatFile,
    Clone,
    Init,
    ReadTree,
    Config,
    Diff,
    ForEachRef,
    RevList,
    RevParse,
    ShowRef,
    Status,
}

impl Builtin {
    const fn name(self) -> &'static str {
        match self {
            Self::CatFile => "cat-file",
            Self::Add => "add",
            Self::Apply => "apply",
            Self::CommitTree => "commit-tree",
            Self::Restore => "restore",
            Self::Rm => "rm",
            Self::UpdateRef => "update-ref",
            Self::WriteTree => "write-tree",
            Self::Clone => "clone",
            Self::Init => "init",
            Self::ReadTree => "read-tree",
            Self::Config => "config",
            Self::Diff => "diff",
            Self::ForEachRef => "for-each-ref",
            Self::RevList => "rev-list",
            Self::RevParse => "rev-parse",
            Self::ShowRef => "show-ref",
            Self::Status => "status",
        }
    }
}

/// Raw argv construction is private to the trusted operation implementations.
pub(crate) struct GitCommandSpec {
    pub(crate) args: Vec<OsString>,
}

impl GitCommandSpec {
    pub(crate) fn new(subcommand: Builtin) -> Self {
        Self {
            args: vec![subcommand.name().into()],
        }
    }

    pub(crate) fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub(crate) fn args<const N: usize>(mut self, args: [&str; N]) -> Self {
        self.args.extend(args.map(OsString::from));
        self
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::ReferenceName;

    #[test]
    fn accepts_discrete_arguments_without_shell_interpretation() {
        let operand = RepoPath::new(PathBuf::from("file ; text")).expect("literal filename");
        let command = GitCommandSpec::new(Builtin::Diff)
            .args(["--no-ext-diff", "--"])
            .arg(operand.as_path().as_os_str());
        assert_eq!(command.args, ["diff", "--no-ext-diff", "--", "file ; text"]);
        assert_eq!(
            Request::Identify(PathBuf::from("fixture")).safety_class(),
            OperationSafetyClass::ReadOnly
        );
    }

    #[test]
    fn rejects_leading_dash_option_injection_and_malformed_subcommands() {
        // The old strings cannot select a builtin. Dynamic revision operands
        // enter through the complete object ID or full reference types.
        for invalid in [
            "-c",
            "--patch",
            "status ; text",
            "../git",
            "Status",
            "status --short",
        ] {
            assert!(ObjectId::new(invalid).is_err());
            assert!(ReferenceName::new(invalid).is_err());
        }
        let dash = RepoPath::new("-c".into()).expect("dash is allowed after the path separator");
        assert_eq!(
            GitCommandSpec::new(Builtin::Diff)
                .arg("--")
                .arg(dash.as_path().as_os_str())
                .args,
            ["diff", "--", "-c"]
        );
    }

    #[test]
    fn rejects_interior_nul_bytes_and_empty_inputs() {
        assert!(RepoPath::new(PathBuf::new()).is_err());
        assert!(RepoPath::new("bad\0name".into()).is_err());
        assert!(ObjectId::new("").is_err());
        assert!(ReferenceName::new("refs/heads/bad\0name").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_unix_paths_and_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let raw = OsString::from_vec(b"file-\xff".to_vec());
        let operand = RepoPath::new(raw.clone().into()).expect("native path");
        assert_eq!(
            GitCommandSpec::new(Builtin::Diff)
                .arg("--")
                .arg(operand.as_path().as_os_str())
                .args[2],
            raw
        );
    }
}
