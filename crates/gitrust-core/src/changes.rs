//! UI-independent workspace intents and display data for changes and commits.

use std::{path::PathBuf, sync::Arc};

/// Which copy of a file is being inspected.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ChangeSide {
    /// HEAD to index.
    Staged,
    /// Index to working tree.
    Unstaged,
    /// File outside the index.
    Untracked,
    /// Git reports unresolved index entries.
    Conflict,
}

/// Stable identity within one repository snapshot.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChangeKey {
    /// Native repository-relative path.
    pub path: PathBuf,
    /// Comparison layer.
    pub side: ChangeSide,
}

/// A row in the real changes workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedPath {
    /// File and comparison layer.
    pub key: ChangeKey,
    /// Git status letter.
    pub status: char,
    /// Previous path for a rename or copy.
    pub from: Option<PathBuf>,
    /// Permission or file type changed.
    pub mode_changed: bool,
}

/// A changed line or an entire hunk in the selected preview.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PatchSelection {
    /// Zero-based hunk position.
    pub hunk: usize,
    /// Zero-based line within the hunk. None selects the hunk.
    pub line: Option<usize>,
}

/// Display form of a numbered diff line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchLine {
    /// Base line number if present.
    pub old: Option<u32>,
    /// Target line number if present.
    pub new: Option<u32>,
    /// Display text decoded from the original bytes.
    pub text: String,
    /// Whether the content had a newline.
    pub newline: bool,
}

/// Display form of a hunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PatchHunk {
    /// Unified range and section heading.
    pub header: String,
    /// Numbered lines.
    pub lines: Vec<PatchLine>,
}

/// An indexed display row referencing the preview's text and hunks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatchRow {
    /// A mode, rename, binary, or combined diff header.
    Metadata(usize),
    /// Start of a hunk.
    Hunk(usize),
    /// Old and new line positions within the same hunk.
    Lines(usize, [Option<usize>; 2]),
    /// A missing newline marker for the specified sides.
    NoNewline([bool; 2]),
}

/// A worker-prepared preview for the selected change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangePreview {
    /// Selected file.
    pub key: ChangeKey,
    /// Raw diff decoded as text for the clipboard.
    pub text: String,
    /// Binary content.
    pub binary: bool,
    /// Textual hunks.
    pub hunks: Vec<PatchHunk>,
    /// Textual headers outside regular hunks.
    pub metadata: Vec<String>,
    /// The indexed unified projection.
    pub unified: Vec<PatchRow>,
    /// The indexed side-by-side projection.
    pub split: Vec<PatchRow>,
    /// Maximum approximate monospaced width, in columns.
    pub columns: usize,
    /// The engine can safely apply selected hunks or lines.
    pub partial: bool,
}

/// Explicit author information. It is scoped to a commit request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitAuthor {
    /// Display name.
    pub name: String,
    /// Email address.
    pub email: String,
}

impl CommitAuthor {
    /// Check whether the identity can be passed to Git for a commit.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let valid = |text: &str| {
            !text.is_empty()
                && text.len() <= 4096
                && !text
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '<' | '>'))
        };
        let email = self.email.trim();
        valid(self.name.trim())
            && valid(email)
            && email.contains('@')
            && !email.chars().any(char::is_whitespace)
    }
}

/// Explicit signing approval for one commit.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum CommitSigning {
    /// Create an unsigned commit.
    #[default]
    None,
    /// Use the trusted system GPG binary with a selected fingerprint.
    OpenPgp(String),
    /// Use the trusted system OpenSSH binary with an explicit key path.
    Ssh(PathBuf),
}

/// Commit text and explicit operation preferences.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct CommitDraft {
    /// One-line summary.
    pub title: String,
    /// Optional longer text.
    pub description: String,
    /// Explicitly selected committer identity.
    pub author: CommitAuthor,
    /// Signer and key selection.
    pub signing: CommitSigning,
    /// Rewrite the current commit after confirmation.
    pub amend: bool,
}

impl std::fmt::Debug for CommitDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommitDraft")
            .field("amend", &self.amend)
            .finish_non_exhaustive()
    }
}

/// Worker-prepared list of the content covered by discard confirmation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscardReview {
    /// Request identity within the workspace.
    pub id: u64,
    /// Every affected path and the corresponding diff.
    pub files: Vec<ChangePreview>,
}

/// A workspace action tied to the displayed version.
#[derive(Clone)]
pub struct ChangesIntent {
    /// Snapshot version.
    pub version: u64,
    /// Action to perform.
    pub command: ChangesCommand,
}

/// Changes workspace commands.
#[derive(Clone)]
pub enum ChangesCommand {
    /// Load a file preview.
    Preview(ChangeKey),
    /// Stage selected paths.
    Stage(Vec<ChangeKey>),
    /// Unstage selected paths.
    Unstage(Vec<ChangeKey>),
    /// Apply a subset of bytes from the selected preview.
    Partial(ChangeKey, Vec<PatchSelection>),
    /// Read all affected files for confirmation.
    ReviewDiscard(Vec<ChangeKey>),
    /// Confirm a specific destructive review.
    Discard(u64),
    /// Close a destructive review.
    DismissDiscard,
    /// Read the current commit for amend.
    LoadAmend,
    /// Open a new-commit composer and load local author information.
    LoadAuthor,
    /// Save the specified commit draft.
    Commit(CommitDraft),
}

/// Non-sensitive terminal feedback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangesNotice {
    /// Index or worktree updated.
    Updated,
    /// Commit published.
    Committed,
    /// The operation used a stale snapshot.
    Stale,
    /// No valid author was supplied.
    AuthorRequired,
    /// Explicit signing did not succeed.
    SigningFailed,
    /// Git rejected an unsupported or unsafe operation.
    Unsupported,
    /// Operation did not complete; refresh before retrying.
    Failed,
}

/// Current active changes workspace projection.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangesView {
    /// Identity of this authorized repository workspace.
    pub identity: u64,
    /// Workspace snapshot ID.
    pub version: u64,
    /// Actual Git rows.
    pub rows: Arc<[ChangedPath]>,
    /// Worker-prepared patch.
    pub preview: Option<Arc<ChangePreview>>,
    /// Confirmation content for destructive work.
    pub discard: Option<Arc<DiscardReview>>,
    /// Draft prefill from local Git, if requested.
    pub draft: Option<Arc<CommitDraft>>,
    /// Current operation result.
    pub notice: Option<ChangesNotice>,
    /// A request is running for this workspace.
    pub busy: bool,
}

impl std::fmt::Debug for ChangesIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangesIntent")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ChangesCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChangesCommand")
    }
}
