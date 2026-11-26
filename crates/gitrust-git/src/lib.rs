//! Local Git inspection and mutation with a bounded background executor.

mod command;
mod creation;
mod diff;
mod engine;
mod executable;
mod history;
mod model;
mod mutation;
mod process;
mod repository;
mod status;

pub use command::{DiffTarget, HistoryPage, Inspection, OperationSafetyClass, RepoPath, Request};
pub use creation::{CloneRepository, CloneSource, CreateRepository};
pub use diff::{DiffFile, DiffHunk, DiffLine, DiffLineKind, RepositoryDiff};
pub use engine::{
    CommitDetails, CommitFile, EngineOptions, GitEngine, GitTask, GitTaskId, GitTaskState,
    TaskReport, TaskValue,
};
pub use executable::{GitExecutable, GitVersion};
pub use history::{BranchInfo, BranchList, CommitInfo, CommitSignature, History, Signature};
pub use model::{
    FileChange, GitError, GitPath, HeadState, ObjectId, OutputStream, ProcessOutput, ReferenceName,
};
pub use mutation::{ChangePatch, Mutation, MutationAction, change_rows};
pub use repository::{RepositoryCandidate, RepositoryIdentity, RepositoryInfo, TrustedRepository};
pub use status::{BranchStatus, EntryKind, RepositoryStatus, StatusEntry, SubmoduleState};

/// Stage in which this crate becomes functional.
pub const STAGE_STATUS: gitrust_core::CrateStageStatus =
    gitrust_core::CrateStageStatus::Active { stage: 2 };
