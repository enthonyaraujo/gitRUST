//! Transport choices and remote workspace state.
#![allow(missing_docs)]

use std::{fmt, path::PathBuf, sync::Arc};

#[derive(Clone, Default)]
pub enum Credential {
    #[default]
    Anonymous,
    MemoryCache,
    SecretService,
    Password {
        user: String,
        secret: String,
    },
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential(<private>)")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Auth {
    pub credential: Credential,
    pub ca: Option<PathBuf>,
    pub known_hosts: Option<PathBuf>,
    pub identity: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Remote {
    pub name: String,
    /// Endpoints rejected by the transport policy are never returned here.
    pub fetch: Option<String>,
    pub push: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Integration {
    #[default]
    FastForward,
    Merge,
    Rebase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteNotice {
    Updated,
    Fetched,
    Pushed,
    PushedWithoutTracking,
    UpToDate,
    Integrated,
    MergePending,
    RebasePending,
    RebaseAborted,
    NoUpstream,
    Diverged,
    Dirty,
    SignedRebase,
    Authentication,
    Agent,
    HostKey,
    ChangedHostKey,
    Certificate,
    Offline,
    NonFastForward,
    Rejected,
    Invalid,
    Stale,
    Uncertain,
    Cancelled,
    Timeout,
    Failed,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemotesView {
    pub version: u64,
    pub entries: Arc<[Remote]>,
    pub upstream: Option<(String, String)>,
    pub current: Option<String>,
    pub ahead_behind: Option<(u64, u64)>,
    pub worktree_dirty: bool,
    pub preference: Integration,
    pub last_fetch: Arc<[(String, u64)]>,
    pub busy: bool,
    pub notice: Option<RemoteNotice>,
    pub rebase: RebaseState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RebaseState {
    #[default]
    None,
    Recoverable,
    Unverified,
}

impl RebaseState {
    #[must_use]
    pub const fn in_progress(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Clone, Debug)]
pub struct RemoteIntent {
    pub version: u64,
    pub action: RemoteAction,
}

#[derive(Clone)]
pub enum RemoteAction {
    Add(String, String, Option<String>),
    Edit(String, String, Option<String>),
    Rename(String, String),
    Remove(String),
    Upstream(String, String),
    Fetch(Vec<String>, Auth),
    Pull(String, Integration, Auth),
    Push {
        remote: String,
        branch: String,
        track: bool,
        auth: Auth,
    },
    ContinueRebase,
    AbortRebase,
}

impl fmt::Debug for RemoteAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteAction(<private configuration>)")
    }
}
