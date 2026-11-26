use std::path::{Path, PathBuf};

use gitrust_core::changes::{ChangeKey, ChangeSide};

use crate::{
    BranchList, ChangePatch, CommitInfo, GitError, ObjectId, RepoPath, RepositoryDiff,
    TrustedRepository, history::object, model::decimal,
};

use super::{Builtin, GitCommandSpec, Runner, checked, git_path, path_from_bytes, strip_line_end};

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

    fn checked_local(
        &self,
        cwd: &Path,
        command: GitCommandSpec,
    ) -> Result<crate::ProcessOutput, GitError> {
        checked(self.run(Some(cwd), command, &[], self.limits)?, false)
    }
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
