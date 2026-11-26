use crate::{
    FileChange, GitError, GitPath, ObjectId, ReferenceName,
    model::{MAX_RECORDS, decimal, mode, records},
};

/// Branch headers from porcelain v2.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BranchStatus {
    /// HEAD object, absent before the first commit.
    pub oid: Option<ObjectId>,
    /// Short local branch name, absent when detached.
    pub branch: Option<ReferenceName>,
    /// Short upstream name, if configured.
    pub upstream: Option<ReferenceName>,
    /// Commits ahead and behind the upstream, when known.
    pub divergence: Option<(u64, u64)>,
}

/// Submodule flags reported by Git.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmoduleState {
    /// Ordinary entry.
    None,
    /// Gitlink entry.
    Submodule {
        /// The checked-out submodule commit changed.
        commit: bool,
        /// The submodule contains tracked modifications.
        modified: bool,
        /// The submodule contains untracked files.
        untracked: bool,
    },
}

/// Details that depend on a porcelain entry's record type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryKind {
    /// Ordinary tracked file.
    Ordinary,
    /// Rename or copy with its original byte-preserving path and score.
    RenamedOrCopied {
        /// Original path.
        from: GitPath,
        /// Similarity percentage.
        similarity: u8,
    },
    /// Conflict with stage 1, 2, 3 and worktree modes and stage object IDs.
    Unmerged {
        /// File modes.
        modes: [u32; 4],
        /// Stage object IDs.
        objects: [ObjectId; 3],
    },
    /// Untracked entry.
    Untracked,
    /// Ignored entry.
    Ignored,
}

/// One porcelain v2 entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusEntry {
    /// Current path.
    pub path: GitPath,
    /// Index status, if tracked.
    pub index: Option<FileChange>,
    /// Worktree status, if tracked.
    pub worktree: Option<FileChange>,
    /// Submodule flags, if tracked.
    pub submodule: Option<SubmoduleState>,
    /// HEAD, index and worktree modes for ordinary and moved entries.
    pub modes: Option<[u32; 3]>,
    /// HEAD and index object IDs for ordinary and moved entries.
    pub objects: Option<[ObjectId; 2]>,
    /// Kind-specific information.
    pub kind: EntryKind,
}

/// Structured status with byte-preserving paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryStatus {
    /// Branch metadata.
    pub branch: BranchStatus,
    /// Changes in Git's output order.
    pub entries: Vec<StatusEntry>,
}

impl RepositoryStatus {
    /// Parse complete output from porcelain v2 with NUL framing and branch headers.
    ///
    /// The porcelain format uses the literal `(detached)` as a sentinel.
    /// The engine combines live output with symbolic HEAD to disambiguate it.
    ///
    /// # Errors
    /// Rejects damaged, incomplete and unknown records.
    pub fn parse(bytes: &[u8]) -> Result<Self, GitError> {
        Self::parse_with_head(bytes, None, None)
    }

    pub(crate) fn parse_with_head(
        bytes: &[u8],
        head: Option<&ReferenceName>,
        mut width: Option<usize>,
    ) -> Result<Self, GitError> {
        let mut records = records(bytes)?;
        let mut branch = BranchStatus::default();
        let mut entries = Vec::new();
        let mut saw_oid = false;
        let mut saw_head = false;
        while let Some(record) = records.next() {
            if let Some(value) = record.strip_prefix(b"# branch.oid ").filter(|_| !saw_oid) {
                saw_oid = true;
                if value != b"(initial)" {
                    branch.oid = Some(status_object(value, &mut width)?);
                }
            } else if let Some(value) = record.strip_prefix(b"# branch.head ").filter(|_| !saw_head)
            {
                saw_head = true;
                if value != b"(detached)"
                    || head.is_some_and(|name| name.as_bytes() == b"refs/heads/(detached)")
                {
                    branch.branch = Some(ReferenceName::short(value)?);
                }
            } else if let Some(value) = record
                .strip_prefix(b"# branch.upstream ")
                .filter(|_| branch.upstream.is_none())
            {
                branch.upstream = Some(ReferenceName::short(value)?);
            } else if let Some(value) = record
                .strip_prefix(b"# branch.ab +")
                .filter(|_| branch.divergence.is_none())
            {
                let (ahead, behind) =
                    crate::model::split_once(value, b' ').ok_or_else(malformed)?;
                branch.divergence = Some((
                    decimal(ahead, "ahead count")?,
                    decimal(
                        behind.strip_prefix(b"-").ok_or_else(malformed)?,
                        "behind count",
                    )?,
                ));
            } else {
                if entries.len() == MAX_RECORDS {
                    return Err(GitError::ParsedLimitExceeded("status entry"));
                }
                entries.push(parse_entry(record, &mut records, &mut width)?);
            }
        }
        if !saw_oid || !saw_head || (branch.branch.is_none() && branch.oid.is_none()) {
            return Err(malformed());
        }
        Ok(Self { branch, entries })
    }
}

fn malformed() -> GitError {
    GitError::MalformedOutput("porcelain v2 record")
}

fn status_object(bytes: &[u8], width: &mut Option<usize>) -> Result<ObjectId, GitError> {
    let id = ObjectId::parse(bytes)?;
    if *width.get_or_insert(bytes.len()) != bytes.len() {
        return Err(malformed());
    }
    Ok(id)
}

fn parse_entry<'a>(
    record: &[u8],
    records: &mut impl Iterator<Item = &'a [u8]>,
    width: &mut Option<usize>,
) -> Result<StatusEntry, GitError> {
    let (tag, value) = crate::model::split_once(record, b' ').ok_or_else(malformed)?;
    if matches!(tag, b"?" | b"!") {
        return Ok(StatusEntry {
            path: GitPath::parse(value)?,
            index: None,
            worktree: None,
            submodule: None,
            modes: None,
            objects: None,
            kind: if tag == b"?" {
                EntryKind::Untracked
            } else {
                EntryKind::Ignored
            },
        });
    }
    let count = match tag {
        b"1" => 8,
        b"2" => 9,
        b"u" => 10,
        _ => return Err(malformed()),
    };
    let parts: Vec<_> = value.splitn(count, |&byte| byte == b' ').collect();
    if parts.len() != count {
        return Err(malformed());
    }
    let [index, worktree] = parts[0] else {
        return Err(malformed());
    };
    let submodule = match parts[1] {
        b"N..." => SubmoduleState::None,
        [
            b'S',
            commit @ (b'C' | b'.'),
            modified @ (b'M' | b'.'),
            untracked @ (b'U' | b'.'),
        ] => SubmoduleState::Submodule {
            commit: *commit == b'C',
            modified: *modified == b'M',
            untracked: *untracked == b'U',
        },
        _ => return Err(malformed()),
    };
    let mut entry = StatusEntry {
        path: GitPath::parse(parts[count - 1])?,
        index: Some(FileChange::parse(*index)?),
        worktree: Some(FileChange::parse(*worktree)?),
        submodule: Some(submodule),
        modes: None,
        objects: None,
        kind: EntryKind::Ordinary,
    };
    let modes = [mode(parts[2])?, mode(parts[3])?, mode(parts[4])?];
    if tag == b"u" {
        entry.kind = EntryKind::Unmerged {
            modes: [modes[0], modes[1], modes[2], mode(parts[5])?],
            objects: [
                status_object(parts[6], width)?,
                status_object(parts[7], width)?,
                status_object(parts[8], width)?,
            ],
        };
    } else {
        entry.modes = Some(modes);
        entry.objects = Some([
            status_object(parts[5], width)?,
            status_object(parts[6], width)?,
        ]);
        if tag == b"2" {
            let [b'R' | b'C', score @ ..] = parts[7] else {
                return Err(malformed());
            };
            let similarity = decimal(score, "rename score")?;
            if similarity > 100 {
                return Err(malformed());
            }
            entry.kind = EntryKind::RenamedOrCopied {
                from: GitPath::parse(records.next().ok_or_else(malformed)?)?,
                similarity,
            };
        }
    }
    Ok(entry)
}
