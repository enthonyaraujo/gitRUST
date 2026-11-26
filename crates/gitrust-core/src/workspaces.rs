//! Repository management state shared by the native view and coordinator.

use std::path::{Path, PathBuf};

/// Maximum number of entries in the local repository catalog.
pub const MAX_RECENT_REPOSITORIES: usize = 64;

/// Saved appearance choice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Appearance {
    /// Use the platform appearance.
    #[default]
    System,
    /// Light palette.
    Light,
    /// Dark palette.
    Dark,
}

/// Saved interface language.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Locale {
    /// English.
    #[default]
    English,
    /// Brazilian Portuguese.
    Portuguese,
}

/// Local UI preferences.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Preferences {
    /// Appearance choice.
    pub appearance: Appearance,
    /// Interface language.
    pub locale: Locale,
    /// Interface scale, normally between 100 and 200.
    pub scale_percent: u16,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            locale: Locale::English,
            scale_percent: 100,
        }
    }
}

/// Most recently used workspaces, keyed by canonical worktree root or bare directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecentRepositories {
    /// Paths in most-recent-first order. Authorization is stored separately.
    pub paths: Vec<PathBuf>,
    /// Last selected path, if it still belongs to the catalog.
    pub last: Option<PathBuf>,
}

impl RecentRepositories {
    /// Move an opened workspace to the front and remember it.
    pub fn open(&mut self, path: PathBuf) {
        self.paths.retain(|entry| entry != &path);
        self.paths.insert(0, path.clone());
        self.paths.truncate(MAX_RECENT_REPOSITORIES);
        self.last = Some(path);
    }

    /// Remove only the saved reference.
    pub fn forget(&mut self, path: &Path) {
        self.paths.retain(|entry| entry != path);
        if self.last.as_deref() == Some(path) {
            self.last = None;
        }
    }
}

/// Current checked-out reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Checkout {
    /// Attached branch, as a display label.
    Branch(String),
    /// Detached commit, as a short display label.
    Detached(String),
    /// Initial branch with no commits.
    Unborn(String),
}

/// Basic Stage 03 status summary.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChangeCounts {
    /// Paths with staged changes.
    pub staged: usize,
    /// Paths with unstaged changes.
    pub unstaged: usize,
    /// Untracked paths.
    pub untracked: usize,
    /// Conflicted paths.
    pub conflicted: usize,
}

/// Information from the last successful inspection of the active workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Workspace {
    /// Canonical root or bare path.
    pub path: PathBuf,
    /// Current checked-out reference.
    pub checkout: Checkout,
    /// Whether an endpoint is configured, without retaining the address.
    pub has_remote: bool,
    /// Working tree summary, absent for a bare repository.
    pub changes: Option<ChangeCounts>,
}

/// Phase of a foreground repository operation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Activity {
    /// Waiting for the initial local services.
    #[default]
    Starting,
    /// Ready to accept requests.
    Idle,
    /// Resolving a candidate without inspecting it.
    Identifying,
    /// Waiting for explicit authorization.
    Trust,
    /// Refreshing workspace information.
    Inspecting,
    /// Initializing a new repository.
    Creating,
    /// Cloning a repository.
    Cloning,
    /// Waiting for cancellation and cleanup.
    Cancelling,
}

/// The kind of repository published by a provisioning operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProvisionKind {
    /// A new repository was initialized.
    Create,
    /// A public repository was cloned.
    Clone,
}

/// A reported phase of Git's clone progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferPhase {
    /// Counting objects.
    Counting,
    /// Compressing objects.
    Compressing,
    /// Receiving objects.
    Receiving,
    /// Resolving deltas.
    Resolving,
}

/// Percentage within a named phase.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferProgress {
    /// Current phase.
    pub phase: TransferPhase,
    /// Percentage in this phase.
    pub percent: u8,
}

/// A localized issue category. Private subprocess output stays out of the view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceIssue {
    /// Missing or incompatible Git.
    GitUnavailable,
    /// The specified directory no longer exists.
    Unavailable,
    /// The directory does not resolve to a Git repository.
    NotRepository,
    /// Access denied.
    PermissionDenied,
    /// Policy rejection or denied authorization.
    Security,
    /// Invalid path, project name, branch or URL.
    InvalidInput,
    /// A creation destination is occupied.
    DestinationExists,
    /// The operation was cancelled.
    Cancelled,
    /// An operation failed and can be retried.
    OperationFailed,
    /// Native folder action failed.
    FolderActionFailed,
}

/// Data consumed by the repository workspace view.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RepositoryView {
    /// Branches, history and merge state for the active workspace.
    pub navigation: Option<crate::navigation::NavigationView>,
    /// Active changes workspace, after a real inspection.
    pub changes: Option<crate::changes::ChangesView>,
    /// Saved catalog and selected history.
    pub recent: RecentRepositories,
    /// A known unavailable path in the catalog.
    pub unavailable: Vec<PathBuf>,
    /// Last inspected active workspace.
    pub active: Option<Workspace>,
    /// Canonical path awaiting authorization.
    pub candidate: Option<PathBuf>,
    /// Current operation.
    pub activity: Activity,
    /// Requested destination while provisioning or cancelling.
    pub destination: Option<PathBuf>,
    /// Successful publication awaiting the separate trust decision.
    pub provisioned: Option<ProvisionKind>,
    /// Most recent recoverable operation issue.
    pub issue: Option<WorkspaceIssue>,
    /// Native Git is available.
    pub git_available: bool,
    /// Preferences or recents could not be loaded or saved.
    pub storage_issue: bool,
    /// Native watcher is active. Focus and manual refresh remain available.
    pub watching: bool,
    /// Transfer status of the current clone.
    pub progress: Option<TransferProgress>,
}

/// Intent sent by the native view to the worker coordinator.
#[derive(Clone)]
pub enum RepositoryAction {
    /// Request a branch, history or merge operation.
    Navigate(crate::navigation::NavigationIntent),
    /// Dispatch an intent for the active changes workspace.
    Changes(crate::changes::ChangesIntent),
    /// Ask the native platform dialog for a folder.
    Browse,
    /// Choose a parent directory for a new workspace.
    BrowseDestination,
    /// Repeat native Git discovery after installation or configuration.
    RetryGit,
    /// Resolve a typed or chosen repository location.
    Open(PathBuf),
    /// Accept or decline the candidate currently shown.
    Authorize { path: PathBuf, accepted: bool },
    /// Remove a path from the catalog.
    Forget(PathBuf),
    /// Reveal an existing folder.
    Reveal(PathBuf),
    /// Reinspect the current workspace.
    Refresh,
    /// The native window regained focus.
    Focus,
    /// Initialize a repository in a new directory.
    Create {
        destination: PathBuf,
        branch: String,
    },
    /// Clone from an explicitly submitted public URL.
    Clone { destination: PathBuf, url: String },
    /// Cancel the current request.
    Cancel,
    /// Save changed local appearance preferences.
    SavePreferences(Preferences),
}

impl std::fmt::Debug for RepositoryAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let action = match self {
            Self::Browse => "Browse",
            Self::Changes(_) => "Changes",
            Self::Navigate(_) => "Navigate",
            Self::BrowseDestination => "BrowseDestination",
            Self::Open(_) => "Open",
            Self::Authorize { .. } => "Authorize",
            Self::Forget(_) => "Forget",
            Self::Reveal(_) => "Reveal",
            Self::Refresh => "Refresh",
            Self::Focus => "Focus",
            Self::Create { .. } => "Create",
            Self::Clone { .. } => "Clone",
            Self::Cancel => "Cancel",
            Self::RetryGit => "RetryGit",
            Self::SavePreferences(_) => "SavePreferences",
        };
        f.write_str(action)
    }
}
