use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use gitrust_core::changes::{
    ChangeKey, ChangeSide, ChangedPath, CommitAuthor, CommitDraft, CommitSigning,
};

use crate::{
    ChangePatch, CommitInfo, Mutation, MutationAction, RepoPath, Signature, change_rows,
    history::parse_commit, model::records, mutation::path_operands,
};

use super::{
    Builtin, DiffTarget, GitCommandSpec, GitError, HeadState, ObjectId, RepositoryDiff, Runner,
    TaskValue, TrustedRepository, checked, git_path, object, path_from_bytes, process, status_spec,
    strip_line_end,
};

const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_KEYS: usize = 4096;
const MAX_REVIEW_BYTES: usize = 16 * 1024 * 1024;

impl Runner<'_> {
    pub(super) fn preview(
        &self,
        repo: &TrustedRepository,
        key: &ChangeKey,
        width: usize,
    ) -> Result<ChangePatch, GitError> {
        let cwd = repo
            .repository()
            .worktree()
            .ok_or(GitError::UnsupportedRepositoryFeature(
                "changes in a bare repository",
            ))?;
        let native = RepoPath::new(key.path.clone())?;
        inspect_path(cwd, native.as_path(), key.side == ChangeSide::Untracked)?;
        if key.side == ChangeSide::Conflict {
            let spec = GitCommandSpec::new(Builtin::Diff)
                .args(["--cc", "--no-color", "--no-ext-diff", "--no-textconv", "--"])
                .arg(git_path(native.as_path()));
            let output = checked(self.run(Some(cwd), spec, &[], self.limits)?, false)?;
            return Ok(ChangePatch {
                key: key.clone(),
                raw: output.stdout,
                file: None,
            });
        }
        let target = if key.side == ChangeSide::Staged {
            DiffTarget::Index
        } else {
            DiffTarget::Worktree
        };
        let spec = self.diff_spec(
            cwd,
            &target,
            Some(&native),
            width,
            key.side == ChangeSide::Untracked,
        )?;
        let spec = if key.side == ChangeSide::Staged {
            let status = self.status_for_preview(repo, width)?;
            let from = status.entries.iter().find_map(|entry| match &entry.kind {
                crate::EntryKind::RenamedOrCopied { from, .. }
                    if entry.path.to_path_buf().is_ok_and(|path| path == key.path) =>
                {
                    Some(from)
                }
                _ => None,
            });
            if let Some(from) = from {
                spec.arg(git_path(&from.to_path_buf()?))
            } else {
                spec
            }
        } else {
            spec
        };
        let output = checked(self.run(Some(cwd), spec, &[], self.limits)?, true)?;
        let mut files = RepositoryDiff::parse(&output.stdout)?.files;
        let raw = output
            .stdout
            .windows(2)
            .position(|window| window == [0, 0])
            .map_or(&[][..], |boundary| &output.stdout[boundary + 2..]);
        let file_index = files
            .iter()
            .position(|file| file.path.to_path_buf().is_ok_and(|path| path == key.path));
        let raw = match (files.len(), file_index) {
            (0 | 1, _) => raw,
            (count, Some(index)) => single_patch(raw, index, count)
                .ok_or(GitError::MalformedOutput("ambiguous file patch"))?,
            _ => return Err(GitError::StaleSnapshot),
        };
        let file = file_index.map(|index| files.swap_remove(index));
        Ok(ChangePatch {
            key: key.clone(),
            raw: raw.to_vec(),
            file,
        })
    }

    fn status_for_preview(
        &self,
        repo: &TrustedRepository,
        width: usize,
    ) -> Result<crate::RepositoryStatus, GitError> {
        let spec = status_spec(false);
        let symbolic = repo.head_reference()?;
        let data = checked(
            self.run(Some(repo.repository().location()), spec, &[], self.limits)?,
            false,
        )?;
        crate::RepositoryStatus::parse_with_head(&data.stdout, symbolic.as_ref(), Some(width))
    }

    pub(super) fn review_discard(
        &self,
        repo: &TrustedRepository,
        keys: &[ChangeKey],
        width: usize,
    ) -> Result<Vec<ChangePatch>, GitError> {
        check_batch(keys.len())?;
        let status = self.status_for_preview(repo, width)?;
        let rows = change_rows(&status)?;
        check_discard_keys(&rows, keys.iter())?;
        let mut total = 0;
        let mut patches = Vec::new();
        for key in keys {
            let patch = self.preview(repo, key, width)?;
            total += patch.raw.len();
            if total > MAX_REVIEW_BYTES {
                return Err(GitError::ParsedLimitExceeded("discard review bytes"));
            }
            patches.push(patch);
        }
        Ok(patches)
    }

    pub(super) fn mutate(
        &mut self,
        repo: &TrustedRepository,
        mutation: &Mutation,
    ) -> Result<TaskValue, GitError> {
        let (_, width, _) = self.prepare(repo)?;
        let cwd = repo
            .repository()
            .worktree()
            .ok_or(GitError::UnsupportedRepositoryFeature(
                "mutation in a bare repository",
            ))?;
        let mut edit = IndexEdit::acquire(repo.repository().git_dir())?;
        let shared = self.run(
            Some(cwd),
            GitCommandSpec::new(Builtin::RevParse).arg("--shared-index-path"),
            &[],
            self.limits,
        )?;
        if !checked(shared, false)?.stdout.is_empty() {
            return Err(GitError::UnsupportedRepositoryFeature(
                "split index mutations",
            ));
        }
        let actual = self.worktree_status(repo, width, false)?;
        if actual != *mutation.expected {
            return Err(GitError::StaleSnapshot);
        }
        let rows = change_rows(&actual)?;
        let head = self.head(repo, width)?;
        match &mutation.action {
            MutationAction::Stage(keys) => {
                let paths = selection_paths(cwd, keys, &rows, false)?;
                self.run_paths(
                    cwd,
                    &edit,
                    GitCommandSpec::new(Builtin::Add).arg("-A"),
                    &paths,
                )?;
            }
            MutationAction::Unstage(keys) => {
                let paths = selection_paths(cwd, keys, &rows, true)?;
                self.run_paths(cwd, &edit, unstage_spec(&head), &paths)?;
            }
            MutationAction::Partial { patch, selection } => {
                path_operands(patch.key(), &rows)?;
                if self.preview(repo, patch.key(), width)? != **patch {
                    return Err(GitError::StaleSnapshot);
                }
                let subset = patch.subset(selection)?;
                for check in [true, false] {
                    let mut spec = GitCommandSpec::new(Builtin::Apply).args([
                        "--cached",
                        "--unidiff-zero",
                        "--whitespace=nowarn",
                    ]);
                    if check {
                        spec = spec.arg("--check");
                    }
                    self.run_index(cwd, &edit, spec.args(["--", "-"]), &subset)?;
                }
            }
            MutationAction::Discard(patches) => {
                let index_changed = self.discard(repo, &edit, patches, &rows, &head, width)?;
                if !index_changed {
                    return Ok(TaskValue::Changed(None));
                }
            }
            MutationAction::Commit(draft) => {
                let id = self.commit(repo, &edit, &head, &rows, width, draft)?;
                return Ok(TaskValue::Changed(Some(id)));
            }
        }
        self.control.check()?;
        edit.publish()?;
        Ok(TaskValue::Changed(None))
    }

    fn run_index(
        &self,
        cwd: &Path,
        edit: &IndexEdit,
        spec: GitCommandSpec,
        input: &[u8],
    ) -> Result<crate::ProcessOutput, GitError> {
        checked(
            self.run_policy(
                Some(cwd),
                spec,
                input,
                self.limits,
                process::Policy::Index(&edit.scratch, &[]),
                &[],
            )?,
            false,
        )
    }

    fn run_paths(
        &self,
        cwd: &Path,
        edit: &IndexEdit,
        spec: GitCommandSpec,
        paths: &[PathBuf],
    ) -> Result<(), GitError> {
        let mut input = Vec::new();
        for path in paths {
            input.extend_from_slice(git_path(path).as_encoded_bytes());
            input.push(0);
        }
        self.run_index(
            cwd,
            edit,
            spec.args(["--pathspec-from-file=-", "--pathspec-file-nul", "--"]),
            &input,
        )?;
        Ok(())
    }

    fn discard(
        &self,
        repo: &TrustedRepository,
        edit: &IndexEdit,
        patches: &[ChangePatch],
        rows: &[ChangedPath],
        head: &HeadState,
        width: usize,
    ) -> Result<bool, GitError> {
        check_batch(patches.len())?;
        check_discard_keys(rows, patches.iter().map(ChangePatch::key))?;
        let cwd = repo.repository().location();
        for patch in patches {
            if self.preview(repo, patch.key(), width)? != *patch {
                return Err(GitError::StaleSnapshot);
            }
        }
        let mut staged = Vec::new();
        let mut unstaged = Vec::new();
        let mut untracked = Vec::new();
        for patch in patches {
            let group = match patch.key.side {
                ChangeSide::Staged => &mut staged,
                ChangeSide::Unstaged => &mut unstaged,
                ChangeSide::Untracked => &mut untracked,
                ChangeSide::Conflict => {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "discarding conflicts",
                    ));
                }
            };
            group.extend(path_operands(patch.key(), rows)?);
        }
        if !staged.is_empty() && matches!(head, HeadState::Unborn(_)) {
            return Err(GitError::UnsupportedRepositoryFeature(
                "discard staged changes in an unborn repository; unstage first",
            ));
        }
        for path in staged.iter().chain(&unstaged) {
            inspect_path(cwd, path, false)?;
        }
        for path in &untracked {
            inspect_path(cwd, path, true)?;
        }
        if !unstaged.is_empty() {
            self.run_paths(
                cwd,
                edit,
                GitCommandSpec::new(Builtin::Restore).arg("--worktree"),
                &unstaged,
            )?;
        }
        let index_changed = !staged.is_empty();
        if index_changed {
            self.run_paths(
                cwd,
                edit,
                GitCommandSpec::new(Builtin::Restore).args([
                    "--source=HEAD",
                    "--staged",
                    "--worktree",
                ]),
                &staged,
            )?;
        }
        for path in untracked {
            self.control.check()?;
            remove_untracked(cwd, &path)?;
        }
        Ok(index_changed)
    }

    pub(super) fn draft(
        &self,
        repo: &TrustedRepository,
        width: usize,
        amend: bool,
    ) -> Result<(CommitDraft, Option<ObjectId>), GitError> {
        let cwd = repo.repository().location();
        let raw = checked(
            self.run(
                Some(cwd),
                GitCommandSpec::new(Builtin::Config).args([
                    "--local",
                    "--no-includes",
                    "--null",
                    "--list",
                ]),
                &[],
                self.limits,
            )?,
            false,
        )?;
        let mut author = CommitAuthor::default();
        for record in records(&raw.stdout)? {
            if let Some(name) = record.strip_prefix(b"user.name\n") {
                author.name = String::from_utf8_lossy(name).into_owned();
            }
            if let Some(email) = record.strip_prefix(b"user.email\n") {
                author.email = String::from_utf8_lossy(email).into_owned();
            }
        }
        let mut draft = CommitDraft {
            author,
            amend,
            ..CommitDraft::default()
        };
        let mut amended = None;
        if amend {
            let HeadState::Attached(_, id) = self.head(repo, width)? else {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "amend requires an existing branch commit",
                ));
            };
            let (commit, _) = self.read_commit(cwd, id.clone())?;
            amended = Some(id);
            let message = String::from_utf8_lossy(&commit.message);
            let (title, body) = message.split_once('\n').unwrap_or((&message, ""));
            title.clone_into(&mut draft.title);
            body.strip_prefix('\n')
                .unwrap_or(body)
                .trim_end_matches('\n')
                .clone_into(&mut draft.description);
        }
        Ok((draft, amended))
    }

    pub(super) fn read_commit(
        &self,
        cwd: &Path,
        id: ObjectId,
    ) -> Result<(CommitInfo, bool), GitError> {
        let spec = GitCommandSpec::new(Builtin::CatFile).args(["commit", id.as_str()]);
        let output = checked(self.run(Some(cwd), spec, &[], self.limits)?, false)?;
        let signed = output
            .stdout
            .split(|byte| *byte == b'\n')
            .take_while(|line| !line.is_empty())
            .any(|line| line.starts_with(b"gpgsig ") || line.starts_with(b"gpgsig-sha256 "));
        Ok((parse_commit(id, &output.stdout)?, signed))
    }

    fn commit(
        &self,
        repo: &TrustedRepository,
        edit: &IndexEdit,
        head: &HeadState,
        rows: &[ChangedPath],
        width: usize,
        draft: &CommitDraft,
    ) -> Result<ObjectId, GitError> {
        if rows.iter().any(|row| row.key.side == ChangeSide::Conflict) {
            return Err(GitError::UnsupportedRepositoryFeature(
                "unresolved conflict",
            ));
        }
        if !draft.amend && !rows.iter().any(|row| row.key.side == ChangeSide::Staged) {
            return Err(GitError::InvalidArgument("commit requires staged changes"));
        }
        for name in [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-apply",
            "rebase-merge",
            "sequencer",
        ] {
            match fs::symlink_metadata(repo.repository().git_dir().join(name)) {
                Ok(_) => {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "operation in progress",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(GitError::io(error)),
            }
        }
        let cwd = repo.repository().location();
        let (reference, previous) = match head {
            HeadState::Attached(branch, id) => (branch, Some(id)),
            HeadState::Unborn(branch) if !draft.amend => (branch, None),
            _ => {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "commit requires an attached branch",
                ));
            }
        };
        if !reference.as_bytes().starts_with(b"refs/heads/") {
            return Err(GitError::InvalidReference);
        }
        let message = commit_message(draft)?;
        let mut original = None;
        let parents = if draft.amend {
            let (old, signed) =
                self.read_commit(cwd, previous.ok_or(GitError::StaleSnapshot)?.clone())?;
            if signed && matches!(draft.signing, CommitSigning::None) {
                return Err(GitError::SigningFailed);
            }
            original = Some(old.author);
            old.parents
        } else {
            previous.cloned().into_iter().collect()
        };
        let identity = identity(&draft.author, original.as_ref())?;
        let tree = self.run_index(cwd, edit, GitCommandSpec::new(Builtin::WriteTree), &[])?;
        let tree = object(strip_line_end(&tree.stdout)?, width)?;
        let (signature, options) = signing_options(&draft.signing)?;
        let mut spec = GitCommandSpec::new(Builtin::CommitTree)
            .arg(tree.as_str())
            .arg(signature)
            .args(["-F", "-"]);
        for parent in &parents {
            spec = spec.args(["-p", parent.as_str()]);
        }
        let policy = if matches!(draft.signing, CommitSigning::OpenPgp(_)) {
            process::Policy::OpenPgp(&edit.scratch, &identity)
        } else {
            process::Policy::Index(&edit.scratch, &identity)
        };
        let output = self.run_policy(Some(cwd), spec, &message, self.limits, policy, &options)?;
        if !output.status.success() && !matches!(draft.signing, CommitSigning::None) {
            return Err(GitError::SigningFailed);
        }
        let id = object(strip_line_end(&checked(output, false)?.stdout)?, width)?;
        repo.validate_reflogs(self.control)?;
        if self.head(repo, width)? != *head {
            return Err(GitError::StaleSnapshot);
        }
        self.control.check()?;
        let missing = "0".repeat(width);
        let spec = GitCommandSpec::new(Builtin::UpdateRef)
            .args(["--no-deref", "-m", "commit from gitRUST", "--"])
            .arg(path_from_bytes(reference.as_bytes())?.into_os_string())
            .args([
                id.as_str(),
                previous.map_or(missing.as_str(), ObjectId::as_str),
            ]);
        checked(
            self.run_policy(
                Some(cwd),
                spec,
                &[],
                self.limits,
                process::Policy::Index(&edit.scratch, &identity),
                &[],
            )?,
            false,
        )?;
        Ok(id)
    }
}

fn single_patch(raw: &[u8], index: usize, file_count: usize) -> Option<&[u8]> {
    let mut starts = raw
        .windows(11)
        .enumerate()
        .filter_map(|(i, bytes)| {
            (bytes == b"diff --git ")
                .then_some(i)
                .filter(|&i| i == 0 || raw[i - 1] == b'\n')
        })
        .collect::<Vec<_>>();
    if starts.len() != file_count {
        return None;
    }
    starts.push(raw.len());
    raw.get(*starts.get(index)?..*starts.get(index + 1)?)
}

fn check_batch(count: usize) -> Result<(), GitError> {
    if !(1..=MAX_KEYS).contains(&count) {
        return Err(GitError::InvalidArgument("path selection count"));
    }
    Ok(())
}

fn check_discard_keys<'a>(
    rows: &[ChangedPath],
    keys: impl Iterator<Item = &'a ChangeKey>,
) -> Result<(), GitError> {
    let mut seen = HashSet::new();
    for key in keys {
        if key.side == ChangeSide::Conflict || !seen.insert(&key.path) {
            return Err(GitError::UnsupportedRepositoryFeature(
                "discard requires distinct resolved paths",
            ));
        }
        let operands = path_operands(key, rows)?;
        if key.side == ChangeSide::Staged
            && rows
                .iter()
                .any(|row| row.key.side != ChangeSide::Staged && operands.contains(&row.key.path))
        {
            return Err(GitError::UnsupportedRepositoryFeature(
                "staged discard would overwrite unstaged content",
            ));
        }
    }
    Ok(())
}

fn selection_paths(
    cwd: &Path,
    keys: &[ChangeKey],
    rows: &[ChangedPath],
    staged: bool,
) -> Result<Vec<PathBuf>, GitError> {
    if keys.is_empty() || keys.len() > rows.len() {
        return Err(GitError::InvalidArgument("path selection count"));
    }
    let indexed: HashMap<_, _> = rows.iter().map(|row| (&row.key, row)).collect();
    let mut paths = Vec::new();
    for key in keys {
        if (key.side == ChangeSide::Staged) != staged {
            return Err(GitError::InvalidArgument("selected comparison layer"));
        }
        let row = indexed.get(key).ok_or(GitError::StaleSnapshot)?;
        paths.push(key.path.clone());
        if row.status == 'R'
            && let Some(original) = &row.from
        {
            paths.push(original.clone());
        }
    }
    paths.sort_unstable();
    paths.dedup();
    for path in &paths {
        inspect_path(cwd, path, false)?;
    }
    Ok(paths)
}

fn unstage_spec(head: &HeadState) -> GitCommandSpec {
    if matches!(head, HeadState::Unborn(_)) {
        GitCommandSpec::new(Builtin::Rm).args(["--cached", "-f"])
    } else {
        GitCommandSpec::new(Builtin::Restore).args(["--staged", "--source=HEAD"])
    }
}

fn inspect_path(root: &Path, path: &Path, regular: bool) -> Result<(), GitError> {
    let resource = RepoPath::new(path.to_path_buf())?;
    let mut path = root.to_path_buf();
    let mut parts = resource.as_path().components().peekable();
    while let Some(part) = parts.next() {
        if part
            .as_os_str()
            .as_encoded_bytes()
            .eq_ignore_ascii_case(b".git")
        {
            return Err(GitError::InvalidArgument("repository metadata path"));
        }
        path.push(part);
        let leaf = parts.peek().is_none();
        match fs::symlink_metadata(&path) {
            Ok(meta) if leaf && (meta.is_file() || (!regular && meta.file_type().is_symlink())) => {
            }
            Ok(meta) if !leaf && meta.is_dir() => {}
            Ok(_) => {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "non-file or symlink ancestor",
                ));
            }
            Err(error) if !regular && error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(GitError::io(error)),
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn remove_untracked(root: &Path, path: &Path) -> Result<(), GitError> {
    use rustix::fs::{AtFlags, FileType, Mode, OFlags, open, openat, statat, unlinkat};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let err = |error| GitError::io(io::Error::from(error));
    let mut parent = open(root, flags, Mode::empty()).map_err(err)?;
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let leaf = statat(&parent, part.as_os_str(), AtFlags::SYMLINK_NOFOLLOW).map_err(err)?;
            if FileType::from_raw_mode(leaf.st_mode) != FileType::RegularFile {
                return Err(GitError::StaleSnapshot);
            }
            return unlinkat(&parent, part.as_os_str(), AtFlags::empty()).map_err(err);
        }
        parent = openat(&parent, part.as_os_str(), flags, Mode::empty()).map_err(err)?;
    }
    Err(GitError::InvalidArgument("untracked deletion path"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn remove_untracked(_root: &Path, _path: &Path) -> Result<(), GitError> {
    Err(GitError::UnsupportedRepositoryFeature(
        "descriptor-relative untracked deletion",
    ))
}

struct IndexEdit {
    _private: tempfile::TempDir,
    scratch: PathBuf,
    index: PathBuf,
    lock: PathBuf,
    own: Option<File>,
    published: bool,
}

impl IndexEdit {
    fn acquire(git_dir: &Path) -> Result<Self, GitError> {
        let index = git_dir.join("index");
        let lock = git_dir.join("index.lock");
        let mut builder = tempfile::Builder::new();
        builder.prefix(".gitrust-index-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let private = builder.tempdir_in(git_dir).map_err(GitError::io)?;
        let scratch = private.path().join("index");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    GitError::RepositoryLocked
                } else {
                    GitError::io(error)
                }
            })?;
        let edit = Self {
            _private: private,
            scratch,
            index,
            lock,
            own: Some(file),
            published: false,
        };
        match fs::symlink_metadata(&edit.index) {
            Ok(meta) if meta.is_file() && meta.len() <= MAX_INDEX_BYTES => {
                let mut input = File::open(&edit.index)
                    .map_err(GitError::io)?
                    .take(MAX_INDEX_BYTES + 1);
                let mut output = File::create_new(&edit.scratch).map_err(GitError::io)?;
                if io::copy(&mut input, &mut output).map_err(GitError::io)? > MAX_INDEX_BYTES {
                    return Err(GitError::ParsedLimitExceeded("index bytes"));
                }
            }
            Ok(_) => {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "oversized or redirected index",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(GitError::io(error)),
        }
        Ok(edit)
    }

    fn publish(&mut self) -> Result<(), GitError> {
        let mut own = self.own.take().ok_or(GitError::RepositoryLocked)?;
        let mut input = File::open(&self.scratch)
            .map_err(GitError::io)?
            .take(MAX_INDEX_BYTES + 1);
        if io::copy(&mut input, &mut own).map_err(GitError::io)? > MAX_INDEX_BYTES {
            return Err(GitError::ParsedLimitExceeded("index bytes"));
        }
        own.flush().map_err(GitError::io)?;
        own.sync_all().map_err(GitError::io)?;
        drop(own);
        fs::rename(&self.lock, &self.index).map_err(GitError::io)?;
        self.published = true;
        Ok(())
    }
}

impl Drop for IndexEdit {
    fn drop(&mut self) {
        drop(self.own.take());
        if !self.published {
            let _ = fs::remove_file(&self.lock);
        }
    }
}

fn commit_message(draft: &CommitDraft) -> Result<Vec<u8>, GitError> {
    let title = draft.title.trim();
    if title.is_empty()
        || title.len() > 4096
        || title.chars().any(char::is_control)
        || draft.description.len() > 128 * 1024
        || draft.description.as_bytes().contains(&0)
    {
        return Err(GitError::InvalidArgument("commit message"));
    }
    if draft.description.is_empty() {
        return Ok(format!("{title}\n").into_bytes());
    }
    Ok(format!("{title}\n\n{}\n", draft.description.trim_end_matches('\n')).into_bytes())
}

fn identity(
    author: &CommitAuthor,
    original: Option<&Signature>,
) -> Result<Vec<(&'static str, OsString)>, GitError> {
    let valid = |text: &str| {
        !text.is_empty()
            && text.len() <= 4096
            && !text
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>'))
    };
    let name = author.name.trim();
    let email = author.email.trim();
    if !valid(name)
        || !valid(email)
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
    {
        return Err(GitError::AuthorRequired);
    }
    let mut env = vec![
        ("GIT_COMMITTER_NAME", name.into()),
        ("GIT_COMMITTER_EMAIL", email.into()),
    ];
    if let Some(author) = original {
        let offset = author.offset_minutes.unsigned_abs();
        let sign = if author.offset_minutes < 0 { '-' } else { '+' };
        env.extend([
            (
                "GIT_AUTHOR_NAME",
                path_from_bytes(&author.name)?.into_os_string(),
            ),
            (
                "GIT_AUTHOR_EMAIL",
                path_from_bytes(&author.email)?.into_os_string(),
            ),
            (
                "GIT_AUTHOR_DATE",
                format!(
                    "@{} {sign}{:02}{:02}",
                    author.unix_seconds,
                    offset / 60,
                    offset % 60
                )
                .into(),
            ),
        ]);
    } else {
        env.extend([
            ("GIT_AUTHOR_NAME", name.into()),
            ("GIT_AUTHOR_EMAIL", email.into()),
        ]);
    }
    Ok(env)
}

fn signing_options(signing: &CommitSigning) -> Result<(OsString, Vec<OsString>), GitError> {
    let (format, program_key, program, key) = match signing {
        CommitSigning::None => return Ok(("--no-gpg-sign".into(), Vec::new())),
        CommitSigning::OpenPgp(fingerprint)
            if matches!(fingerprint.len(), 40 | 64)
                && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) =>
        {
            (
                "openpgp",
                "gpg.openpgp.program",
                "/usr/bin/gpg",
                OsString::from(fingerprint),
            )
        }
        CommitSigning::Ssh(path)
            if path.is_absolute() && !path.as_os_str().as_encoded_bytes().contains(&0) =>
        {
            (
                "ssh",
                "gpg.ssh.program",
                "/usr/bin/ssh-keygen",
                path.as_os_str().to_owned(),
            )
        }
        _ => return Err(GitError::SigningFailed),
    };
    if !trusted_signer(Path::new(program)) {
        return Err(GitError::SigningFailed);
    }
    let mut flag = OsString::from("--gpg-sign=");
    flag.push(key);
    Ok((
        flag,
        vec![
            "-c".into(),
            format!("gpg.format={format}").into(),
            "-c".into(),
            format!("{program_key}={program}").into(),
        ],
    ))
}

fn trusted_signer(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Ok(resolved) = fs::canonicalize(path) else {
            return false;
        };
        resolved.is_file()
            && resolved.ancestors().all(|entry| {
                fs::metadata(entry).is_ok_and(|meta| meta.uid() == 0 && meta.mode() & 0o022 == 0)
            })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
