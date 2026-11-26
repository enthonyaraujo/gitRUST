use std::{
    collections::BTreeSet,
    ffi::OsString,
    fmt,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use gitrust_core::changes::{ChangeKey, ChangeSide, CommitAuthor, CommitDraft, CommitSigning};

use crate::{
    BranchInfo, BranchList, ChangePatch, CommitInfo, GitError, HeadState, ObjectId, ReferenceName,
    RepoPath, RepositoryDiff, RepositoryStatus, TaskValue, TrustedRepository, change_rows,
    history::object, model::decimal,
};

use super::{
    Builtin, GitCommandSpec, Runner, checked, git_path, path_from_bytes, process, strip_line_end,
    write,
};

/// Reference and object shown at the time of the action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefTip {
    /// Full local or remote-tracking reference.
    pub name: ReferenceName,
    /// Expected commit object.
    pub id: ObjectId,
}

/// Starting point for a new local branch.
#[derive(Clone, Debug)]
pub enum BranchSource {
    /// Current HEAD as captured in the mutation snapshot.
    Head,
    /// A selected reference and its observed tip.
    Reference(RefTip),
    /// A selected commit object.
    Commit(ObjectId),
}

/// Branch and merge operations accepted by the local engine.
#[derive(Clone, Debug)]
pub enum RefMutation {
    /// Create a local branch and track its source if it is remote-tracking.
    Create(String, BranchSource),
    /// Switch to an existing local branch.
    Switch(RefTip),
    /// Rename a local branch.
    Rename(RefTip, String),
    /// Delete a branch integrated into HEAD.
    Delete(RefTip),
    /// Apply a merge with fast-forward allowed and commit creation deferred.
    Merge(RefTip, CommitAuthor),
    /// Create the merge commit from the current index.
    Finish(ObjectId, CommitDraft),
    /// Abort a merge started with a clean worktree and unchanged since start.
    Abort(Arc<AbortProof>),
}

/// Current native merge state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MergeState {
    /// No merge in progress.
    None,
    /// Source and current unresolved file count.
    Active(ObjectId, usize),
    /// Another sequencer operation occupies the worktree.
    Other,
}

/// State for the branch selector and merge controls.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NavigationSnapshot {
    /// Branches and HEAD.
    pub refs: BranchList,
    /// Current sequencer state.
    pub merge: MergeState,
    /// Local commit identity, if configured.
    pub author: CommitAuthor,
}

/// Outcome after Git has returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationOutcome {
    /// Branch list or checkout changed.
    BranchUpdated,
    /// Source is already reachable from HEAD.
    Integrated,
    /// Branch advanced without creating a new commit.
    FastForward,
    /// Three-way merge applied, awaiting resolutions or a commit.
    MergePending,
    /// Merge commit created.
    MergeCommitted,
    /// Merge aborted.
    MergeAborted,
}

/// Result of a local branch or merge operation.
#[derive(Debug)]
pub struct NavigationResult {
    /// Observed outcome.
    pub outcome: NavigationOutcome,
    /// Opaque abort proof for a clean, app-started merge.
    pub abort: Option<Arc<AbortProof>>,
}

pub(super) type WorktreeBytes = Vec<(PathBuf, Option<Vec<u8>>)>;

/// The exact post-merge state checked again during abort.
#[derive(Eq, PartialEq)]
pub struct AbortProof {
    repository: crate::RepositoryCandidate,
    source: ObjectId,
    status: RepositoryStatus,
    files: WorktreeBytes,
}

impl fmt::Debug for AbortProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AbortProof(<private worktree snapshot>)")
    }
}

/// Changed path and statistics for one selected comparison.
#[derive(Debug)]
pub struct CommitFile {
    /// Current path or, for a deletion, its previous path.
    pub path: PathBuf,
    /// Original path of a move.
    pub previous: Option<PathBuf>,
    /// Added and removed lines, absent for binary changes.
    pub lines: Option<(u64, u64)>,
}

/// Commit and selected parent comparison.
#[derive(Debug)]
pub struct CommitDetails {
    /// Commit object metadata.
    pub commit: CommitInfo,
    /// Selected parent, absent for a root commit.
    pub parent: Option<ObjectId>,
    /// Files changed relative to the selected parent.
    pub files: Vec<CommitFile>,
}

impl Runner<'_> {
    pub(super) fn branches(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
    ) -> Result<BranchList, GitError> {
        let head = self.head(repo, width)?;
        let command = GitCommandSpec::new(Builtin::ForEachRef).args([
            "--format=%(refname)%00%(objectname)%00%(upstream)%00%(upstream:track)%00%(if)%(worktreepath)%(then)1%(end)%00%(symref)%00",
            "--", "refs/heads/", "refs/remotes/",
        ]);
        let output = self.record(repo.repository().location(), command, &[], false)?;
        BranchList::parse(head, output, width)
    }

    pub(super) fn navigation(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
    ) -> Result<NavigationSnapshot, GitError> {
        let refs = self.branches(repo, width)?;
        let merge = if repo.repository().worktree().is_none() || incompatible(repo)? {
            MergeState::Other
        } else if let Some(source) = merge_source(repo, width)? {
            let status = self.worktree_status(repo, width, false)?;
            MergeState::Active(
                source,
                status
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry.kind, crate::EntryKind::Unmerged { .. }))
                    .count(),
            )
        } else {
            MergeState::None
        };
        let (draft, _) = self.draft(repo, width, false)?;
        Ok(NavigationSnapshot {
            refs,
            merge,
            author: draft.author,
        })
    }

    pub(super) fn mutate_navigation(
        &mut self,
        repo: &TrustedRepository,
        expected: &RepositoryStatus,
        operation: &RefMutation,
    ) -> Result<TaskValue, GitError> {
        let (_, width, _) = self.prepare(repo)?;
        let cwd = repo
            .repository()
            .worktree()
            .ok_or(GitError::UnsupportedRepositoryFeature(
                "branch mutation in bare repository",
            ))?;
        repo.validate_reflogs(self.control)?;
        repo.validate_worktrees(self.control)?;
        validate_merge_files(repo)?;
        let status = self.worktree_status(repo, width, false)?;
        if &status != expected {
            return Err(GitError::StaleSnapshot);
        }
        let snapshot = self.navigation(repo, width)?;
        let valid = matches!(
            (&snapshot.merge, operation),
            (
                MergeState::None,
                RefMutation::Create(..)
                    | RefMutation::Switch(_)
                    | RefMutation::Rename(..)
                    | RefMutation::Delete(_)
                    | RefMutation::Merge(..)
            ) | (
                MergeState::Active(_, _),
                RefMutation::Finish(..) | RefMutation::Abort(_)
            )
        );
        if !valid {
            return Err(GitError::UnsupportedRepositoryFeature(
                "incompatible repository operation in progress",
            ));
        }
        self.control.check()?;
        let result = match operation {
            RefMutation::Merge(tip, author) => {
                self.start_merge(repo, width, &snapshot.refs, &status, tip, author)?
            }
            RefMutation::Finish(source, draft) => {
                self.finish_merge(repo, width, &snapshot, source, draft)?
            }
            RefMutation::Abort(proof) => {
                if self.abort_proof(repo, width)? != **proof {
                    return Err(GitError::StaleSnapshot);
                }
                self.checked_local(cwd, GitCommandSpec::new(Builtin::Merge).arg("--abort"))?;
                result(NavigationOutcome::MergeAborted)
            }
            _ => {
                self.change_branch(cwd, &snapshot.refs, operation)?;
                result(NavigationOutcome::BranchUpdated)
            }
        };
        Ok(TaskValue::NavigationResult(result))
    }

    fn change_branch(
        &self,
        cwd: &Path,
        snapshot: &BranchList,
        operation: &RefMutation,
    ) -> Result<(), GitError> {
        let command = match operation {
            RefMutation::Create(name, source) => {
                self.validate_branch(cwd, name)?;
                let (start, track) = match source {
                    BranchSource::Head => (head_id(&snapshot.head)?.as_str().into(), false),
                    BranchSource::Commit(id) => {
                        self.require_commit(cwd, id, id.as_str().len())?;
                        (id.as_str().into(), false)
                    }
                    BranchSource::Reference(tip) => {
                        let reference = tip_in(snapshot, tip)?;
                        if reference.remote_tracking {
                            (native(reference.name.as_bytes())?, true)
                        } else {
                            (reference.id.as_str().into(), false)
                        }
                    }
                };
                GitCommandSpec::new(Builtin::Branch)
                    .args([
                        if track {
                            "--track=direct"
                        } else {
                            "--no-track"
                        },
                        "--",
                        name,
                    ])
                    .arg(start)
            }
            RefMutation::Switch(tip) => {
                let branch = local_tip(snapshot, tip)?;
                if branch.occupied && !branch.current {
                    return Err(GitError::BranchOccupied);
                }
                let output = self.run(
                    Some(cwd),
                    GitCommandSpec::new(Builtin::Switch)
                        .args([
                            "--no-guess",
                            "--no-overwrite-ignore",
                            "--no-recurse-submodules",
                            "--",
                        ])
                        .arg(short_local(&branch.name)?),
                    &[],
                    self.limits,
                )?;
                if !output.status.success() {
                    return Err(GitError::BranchSwitchBlocked);
                }
                return Ok(());
            }
            RefMutation::Rename(tip, name) => {
                let branch = local_tip(snapshot, tip)?;
                if branch.occupied && !branch.current {
                    return Err(GitError::BranchOccupied);
                }
                self.validate_branch(cwd, name)?;
                GitCommandSpec::new(Builtin::Branch)
                    .args(["--move", "--"])
                    .arg(short_local(&branch.name)?)
                    .arg(name)
            }
            RefMutation::Delete(tip) => {
                let branch = local_tip(snapshot, tip)?;
                if branch.occupied || branch.current {
                    return Err(GitError::BranchOccupied);
                }
                if !self.ancestor(cwd, &branch.id, head_id(&snapshot.head)?)? {
                    return Err(GitError::BranchUnmerged);
                }
                GitCommandSpec::new(Builtin::Branch)
                    .args(["--delete", "--"])
                    .arg(short_local(&branch.name)?)
            }
            _ => return Err(GitError::InvalidArgument("branch mutation")),
        };
        self.checked_local(cwd, command)?;
        Ok(())
    }

    fn validate_branch(&self, cwd: &Path, name: &str) -> Result<(), GitError> {
        if name.starts_with('-') || matches!(name, "HEAD" | "@") || name.len() > 255 {
            return Err(GitError::InvalidReference);
        }
        ReferenceName::new(format!("refs/heads/{name}"))?;
        self.checked_local(
            cwd,
            GitCommandSpec::new(Builtin::CheckRef).args(["--branch", name]),
        )?;
        Ok(())
    }

    fn start_merge(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
        refs: &BranchList,
        status: &RepositoryStatus,
        tip: &RefTip,
        author: &CommitAuthor,
    ) -> Result<NavigationResult, GitError> {
        let HeadState::Attached(_, head) = &refs.head else {
            return Err(GitError::InvalidReference);
        };
        let tip = &local_tip(refs, tip)?.id;
        self.merge_object(repo, width, head, tip, status, author, false)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn merge_object(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
        head: &ObjectId,
        tip: &ObjectId,
        status: &RepositoryStatus,
        author: &CommitAuthor,
        fast_forward: bool,
    ) -> Result<NavigationResult, GitError> {
        let cwd = repo.repository().location();
        validate_merge_files(repo)?;
        repo.validate_reflogs(self.control)?;
        if self.ancestor(cwd, tip, head)? {
            return Ok(result(NavigationOutcome::Integrated));
        }
        if fast_forward && !self.ancestor(cwd, head, tip)? {
            return Err(GitError::Remote(
                gitrust_core::remotes::RemoteNotice::Diverged,
            ));
        }
        if !status.entries.is_empty() {
            return Err(GitError::MergeDirty);
        }
        let identity = write::identity(author, None)?;
        let spec = GitCommandSpec::new(Builtin::Merge).args([
            if fast_forward { "--ff-only" } else { "--ff" },
            "--no-commit",
            "--no-edit",
            "--no-gpg-sign",
            "--no-autostash",
            "--no-rerere-autoupdate",
            "--no-overwrite-ignore",
            "--no-verify-signatures",
            "--",
            tip.as_str(),
        ]);
        let output = self.run_policy(
            Some(cwd),
            spec,
            &[],
            self.limits,
            process::Policy::Author(&identity, false),
            &[
                "-c".into(),
                "rerere.enabled=false".into(),
                "-c".into(),
                "maintenance.auto=false".into(),
            ],
        )?;
        if merge_source(repo, width)?.as_ref() == Some(tip) {
            let abort = self.abort_proof(repo, width).ok().map(Arc::new);
            return Ok(NavigationResult {
                outcome: NavigationOutcome::MergePending,
                abort,
            });
        }
        checked(output, false)?;
        Ok(result(NavigationOutcome::FastForward))
    }

    fn finish_merge(
        &self,
        repo: &TrustedRepository,
        width: usize,
        snapshot: &NavigationSnapshot,
        source: &ObjectId,
        draft: &CommitDraft,
    ) -> Result<NavigationResult, GitError> {
        if snapshot.merge != MergeState::Active(source.clone(), 0)
            || draft.amend
            || !matches!(snapshot.refs.head, HeadState::Attached(..))
            || merge_source(repo, width)?.as_ref() != Some(source)
        {
            return Err(GitError::StaleSnapshot);
        }
        let identity = write::identity(&draft.author, None)?;
        let (signature, mut options) = write::signing_options(&draft.signing)?;
        options.extend(["-c".into(), "maintenance.auto=false".into()]);
        let command = GitCommandSpec::new(Builtin::Commit)
            .args(["--cleanup=verbatim", "-F", "-"])
            .arg(signature);
        let output = self.run_policy(
            Some(repo.repository().location()),
            command,
            &write::commit_message(draft)?,
            self.limits,
            process::Policy::Author(
                &identity,
                matches!(draft.signing, CommitSigning::OpenPgp(_)),
            ),
            &options,
        )?;
        if !output.status.success() && !matches!(draft.signing, CommitSigning::None) {
            return Err(GitError::SigningFailed);
        }
        checked(output, false)?;
        Ok(result(NavigationOutcome::MergeCommitted))
    }

    fn abort_proof(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
    ) -> Result<AbortProof, GitError> {
        let source = merge_source(repo, width)?.ok_or(GitError::StaleSnapshot)?;
        let status = self.worktree_status(repo, width, false)?;
        let files = self.worktree_bytes(repo, &status)?;
        Ok(AbortProof {
            repository: repo.repository().clone(),
            source,
            status,
            files,
        })
    }

    pub(super) fn worktree_bytes(
        &self,
        repo: &TrustedRepository,
        status: &RepositoryStatus,
    ) -> Result<WorktreeBytes, GitError> {
        let mut remaining = 16 * 1024 * 1024u64;
        let mut files = Vec::new();
        for path in change_rows(status)?
            .into_iter()
            .map(|row| row.key.path)
            .collect::<BTreeSet<_>>()
        {
            let root = repo.repository().location();
            write::inspect_path(root, &path, false)?;
            let path_on_disk = root.join(&path);
            let bytes = match fs::symlink_metadata(&path_on_disk) {
                Ok(meta) if meta.is_file() => {
                    let mut bytes = Vec::new();
                    File::open(path_on_disk)
                        .map_err(GitError::io)?
                        .take(remaining + 1)
                        .read_to_end(&mut bytes)
                        .map_err(GitError::io)?;
                    remaining = remaining
                        .checked_sub(bytes.len() as u64)
                        .ok_or(GitError::ParsedLimitExceeded("abort proof bytes"))?;
                    Some(bytes)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                _ => {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "abort proof requires regular files",
                    ));
                }
            };
            self.control.check()?;
            files.push((path, bytes));
        }
        Ok(files)
    }

    pub(super) fn ancestor(
        &self,
        cwd: &Path,
        old: &ObjectId,
        new: &ObjectId,
    ) -> Result<bool, GitError> {
        let out = self.run(
            Some(cwd),
            GitCommandSpec::new(Builtin::MergeBase).args([
                "--is-ancestor",
                "--",
                old.as_str(),
                new.as_str(),
            ]),
            &[],
            self.limits,
        )?;
        if out.status.code() == Some(1) {
            Ok(false)
        } else {
            checked(out, false).map(|_| true)
        }
    }

    pub(super) fn checked_local(
        &self,
        cwd: &Path,
        command: GitCommandSpec,
    ) -> Result<crate::ProcessOutput, GitError> {
        checked(self.run(Some(cwd), command, &[], self.limits)?, false)
    }
}

fn result(outcome: NavigationOutcome) -> NavigationResult {
    NavigationResult {
        outcome,
        abort: None,
    }
}

fn tip_in<'a>(list: &'a BranchList, tip: &RefTip) -> Result<&'a BranchInfo, GitError> {
    list.branches
        .iter()
        .find(|reference| {
            reference.name == tip.name && reference.id == tip.id && !reference.symbolic
        })
        .ok_or(GitError::StaleSnapshot)
}

fn local_tip<'a>(list: &'a BranchList, tip: &RefTip) -> Result<&'a BranchInfo, GitError> {
    let branch = tip_in(list, tip)?;
    if branch.remote_tracking {
        return Err(GitError::InvalidReference);
    }
    Ok(branch)
}

fn native(bytes: &[u8]) -> Result<OsString, GitError> {
    path_from_bytes(bytes).map(PathBuf::into_os_string)
}

fn short_local(name: &ReferenceName) -> Result<OsString, GitError> {
    let short = name
        .as_bytes()
        .strip_prefix(b"refs/heads/")
        .ok_or(GitError::InvalidReference)?;
    if short.starts_with(b"-") || short == b"HEAD" || short == b"@" {
        return Err(GitError::InvalidReference);
    }
    native(short)
}

fn head_id(head: &HeadState) -> Result<&ObjectId, GitError> {
    match head {
        HeadState::Attached(_, id) | HeadState::Detached(id) => Ok(id),
        HeadState::Unborn(_) => Err(GitError::InvalidReference),
    }
}

pub(super) fn merge_source(
    repo: &TrustedRepository,
    width: usize,
) -> Result<Option<ObjectId>, GitError> {
    let path = repo.repository().git_dir().join("MERGE_HEAD");
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() <= 65 => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        _ => {
            return Err(GitError::UnsupportedRepositoryFeature(
                "nonstandard merge state",
            ));
        }
    };
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(GitError::io)?
        .take(66)
        .read_to_end(&mut bytes)
        .map_err(GitError::io)?;
    if bytes.len() as u64 != meta.len() {
        return Err(GitError::StaleSnapshot);
    }
    object(strip_line_end(&bytes)?, width).map(Some)
}

fn validate_merge_files(repo: &TrustedRepository) -> Result<(), GitError> {
    for name in [
        "MERGE_HEAD",
        "MERGE_MSG",
        "MERGE_MODE",
        "ORIG_HEAD",
        "AUTO_MERGE",
        "COMMIT_EDITMSG",
    ] {
        match fs::symlink_metadata(repo.repository().git_dir().join(name)) {
            Ok(meta) if meta.is_file() => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt as _;
                    if meta.nlink() != 1 {
                        return Err(GitError::SecurityPolicyViolation(
                            "hard-linked Git transaction file",
                        ));
                    }
                }
            }
            Ok(_) => {
                return Err(GitError::SecurityPolicyViolation(
                    "redirected Git transaction file",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(GitError::io(error)),
        }
    }
    Ok(())
}

pub(super) fn incompatible(repo: &TrustedRepository) -> Result<bool, GitError> {
    for name in [
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-apply",
        "rebase-merge",
        "sequencer",
        "MERGE_AUTOSTASH",
    ] {
        match fs::symlink_metadata(repo.repository().git_dir().join(name)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(GitError::io(error)),
        }
    }
    Ok(false)
}

impl Runner<'_> {
    fn comparison(
        &self,
        cwd: &Path,
        id: &ObjectId,
        parent: Option<&ObjectId>,
        width: usize,
    ) -> Result<(CommitInfo, Option<ObjectId>, ObjectId), GitError> {
        self.require_commit(cwd, id, width)?;
        let (commit, _) = self.read_commit(cwd, id.clone())?;
        let parent = match parent {
            Some(parent) if commit.parents.contains(parent) => Some(parent.clone()),
            Some(_) => return Err(GitError::InvalidReference),
            None => commit.parents.first().cloned(),
        };
        let base = if let Some(parent) = &parent {
            parent.clone()
        } else {
            let empty = self.checked_local(
                cwd,
                GitCommandSpec::new(Builtin::HashObject).args(["-t", "tree", "--stdin"]),
            )?;
            object(strip_line_end(&empty.stdout)?, width)?
        };
        Ok((commit, parent, base))
    }

    pub(super) fn details(
        &self,
        cwd: &Path,
        id: &ObjectId,
        parent: Option<&ObjectId>,
        width: usize,
    ) -> Result<CommitDetails, GitError> {
        let (commit, parent, base) = self.comparison(cwd, id, parent, width)?;
        let spec = GitCommandSpec::new(Builtin::Diff).args([
            "--numstat",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--no-relative",
            "--ignore-submodules=all",
            "--end-of-options",
            base.as_str(),
            id.as_str(),
            "--",
        ]);
        let output = self.checked_local(cwd, spec)?;
        Ok(CommitDetails {
            commit,
            parent,
            files: numstat(&output.stdout)?,
        })
    }

    pub(super) fn commit_patch(
        &self,
        cwd: &Path,
        id: &ObjectId,
        parent: Option<&ObjectId>,
        path: &RepoPath,
        width: usize,
    ) -> Result<ChangePatch, GitError> {
        let (commit, _, base) = self.comparison(cwd, id, parent, width)?;
        let mut previous = None;
        let spec = GitCommandSpec::new(Builtin::Diff).args([
            "--name-status",
            "-z",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--end-of-options",
            base.as_str(),
            id.as_str(),
            "--",
        ]);
        let output = self.checked_local(cwd, spec)?;
        let mut records = crate::model::records(&output.stdout)?;
        while let Some(status) = records.next() {
            let first = records
                .next()
                .ok_or(GitError::MalformedOutput("name-status"))?;
            if matches!(status.first(), Some(b'R' | b'C')) {
                let second = records
                    .next()
                    .ok_or(GitError::MalformedOutput("rename target"))?;
                if path_from_bytes(second)? == path.as_path() {
                    previous = Some(path_from_bytes(first)?);
                    break;
                }
            }
        }
        let mut spec = Self::patch_spec(width)
            .args(["--end-of-options", base.as_str(), commit.id.as_str(), "--"])
            .arg(git_path(path.as_path()));
        if let Some(previous) = previous {
            spec = spec.arg(git_path(&previous));
        }
        let output = checked(self.run(Some(cwd), spec, &[], self.limits)?, true)?;
        let mut parsed = RepositoryDiff::parse(&output.stdout)?;
        let raw = output
            .stdout
            .windows(2)
            .position(|window| window == [0, 0])
            .map_or(&[][..], |boundary| &output.stdout[boundary + 2..]);
        if parsed.files.len() > 1 {
            return Err(GitError::MalformedOutput("ambiguous commit patch"));
        }
        Ok(ChangePatch {
            key: ChangeKey {
                path: path.as_path().to_owned(),
                side: ChangeSide::Staged,
            },
            raw: raw.to_vec(),
            file: parsed.files.pop(),
        })
    }
}

fn numstat(bytes: &[u8]) -> Result<Vec<CommitFile>, GitError> {
    let mut records = crate::model::records(bytes)?;
    let mut files = Vec::new();
    while let Some(record) = records.next() {
        if files.len() == crate::model::MAX_RECORDS {
            return Err(GitError::ParsedLimitExceeded("commit files"));
        }
        let mut fields = record.splitn(3, |&byte| byte == b'\t');
        let malformed = || GitError::MalformedOutput("numstat");
        let added = fields.next().ok_or_else(malformed)?;
        let removed = fields.next().ok_or_else(malformed)?;
        let path = fields.next().ok_or_else(malformed)?;
        let lines = if [added, removed] == [b"-", b"-"] {
            None
        } else {
            Some((decimal(added, "added")?, decimal(removed, "removed")?))
        };
        let (path, previous) = if path.is_empty() {
            let previous = path_from_bytes(records.next().ok_or_else(malformed)?)?;
            (records.next().ok_or_else(malformed)?, Some(previous))
        } else {
            (path, None)
        };
        let path = RepoPath::new(path_from_bytes(path)?)?.as_path().to_owned();
        files.push(CommitFile {
            path,
            previous,
            lines,
        });
    }
    Ok(files)
}
