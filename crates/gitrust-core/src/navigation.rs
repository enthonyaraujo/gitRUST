//! Native branch, history and merge state shared by the coordinator and UI.
#![allow(missing_docs)]

use std::{path::PathBuf, sync::Arc};

use crate::changes::{ChangePreview, CommitAuthor, CommitDraft};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    pub name: Arc<[u8]>,
    pub label: String,
    pub commit: String,
    pub upstream: Option<String>,
    pub divergence: Option<(u64, u64)>,
    pub remote: bool,
    pub current: bool,
    pub occupied: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub time: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Revision {
    pub id: String,
    pub subject: String,
    pub author: Person,
    pub parents: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryRow {
    pub revision: Revision,
    /// Lane of the commit, using stable lane numbering across pages.
    pub node: usize,
    /// Visible segments for every active lane. Edges end at the next row.
    pub edges: Vec<(usize, usize)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryFile {
    pub path: PathBuf,
    pub previous: Option<PathBuf>,
    pub lines: Option<(u64, u64)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureKind {
    OpenPgp,
    Ssh,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Details {
    pub revision: Revision,
    pub committer: Person,
    pub message: String,
    pub signature: Option<SignatureKind>,
    pub parent: Option<String>,
    pub files: Vec<HistoryFile>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Merge {
    #[default]
    None,
    Active {
        source: String,
        unresolved: usize,
        can_abort: bool,
    },
    Incompatible,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationNotice {
    BranchUpdated,
    Integrated,
    FastForward,
    MergePending,
    MergeCommitted,
    MergeAborted,
    Stale,
    InvalidRef,
    Occupied,
    Unmerged,
    SwitchBlocked,
    Dirty,
    AuthorRequired,
    SigningFailed,
    Incompatible,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Pagination {
    Initial,
    More,
    #[default]
    End,
    Limit,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NavigationView {
    pub version: u64,
    pub identity: u64,
    pub head: Option<String>,
    pub root: Option<String>,
    pub refs: Arc<[Reference]>,
    pub history: Arc<Vec<HistoryRow>>,
    pub pagination: Pagination,
    pub all_refs: bool,
    pub details: Option<Arc<Details>>,
    pub patch: Option<Arc<ChangePreview>>,
    pub merge: Merge,
    pub author: CommitAuthor,
    pub busy: bool,
    pub notice: Option<NavigationNotice>,
}

#[derive(Clone, Debug)]
pub struct NavigationIntent {
    pub version: u64,
    pub command: NavigationCommand,
}

#[derive(Clone, Debug)]
pub enum BranchBase {
    Head,
    Ref(Reference),
    Commit(String),
}

#[derive(Clone, Debug)]
pub enum NavigationCommand {
    UseAllRefs(bool),
    More,
    Details(String, Option<String>),
    Patch(String, Option<String>, PathBuf),
    Create(String, BranchBase),
    Switch(Reference),
    Rename(Reference, String),
    Delete(Reference),
    Merge(Reference, CommitAuthor),
    Finish(String, CommitDraft),
    Abort,
}
