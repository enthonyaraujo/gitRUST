use crate::{
    FileChange, GitError, GitPath, ObjectId,
    model::{MAX_RECORDS, decimal, mode, split_once},
};

/// Type of a line in a two-way patch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffLineKind {
    /// Present on both sides.
    Context,
    /// Present on the target side.
    Added,
    /// Present on the base side.
    Removed,
}

/// A numbered patch line with its original bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffLine {
    /// Relationship to the compared files.
    pub kind: DiffLineKind,
    /// One-based base line number, if applicable.
    pub old_line: Option<u32>,
    /// One-based target line number, if applicable.
    pub new_line: Option<u32>,
    /// Content without the patch prefix or line feed. A carriage return in
    /// CRLF content remains part of the bytes.
    pub content: Vec<u8>,
    /// Whether the original content ends with a line feed.
    pub has_newline: bool,
}

/// One unified-format hunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffHunk {
    /// First base line and line count.
    pub old: (u32, u32),
    /// First target line and line count.
    pub new: (u32, u32),
    /// Optional function or section descriptor.
    pub scope: Vec<u8>,
    /// Lines including context.
    pub lines: Vec<DiffLine>,
}

/// Raw metadata and corresponding patch content for a changed file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffFile {
    /// Current path, or the old path on deletion.
    pub path: GitPath,
    /// Original name on a rename or copy.
    pub from: Option<GitPath>,
    /// Change classification from Git.
    pub change: FileChange,
    /// Similarity or rewrite score when emitted by Git.
    pub score: Option<u8>,
    /// Base and target modes. Zero means the side is absent.
    pub modes: [u32; 2],
    /// Base and target IDs. All zeros mean the ID is unknown or absent.
    pub objects: [ObjectId; 2],
    /// Git considered at least one patch fragment binary.
    pub binary: bool,
    /// Textual hunks in patch order.
    pub hunks: Vec<DiffHunk>,
}

/// A two-way Git diff with NUL-framed metadata and byte-preserving hunks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryDiff {
    /// Changes in Git's order.
    pub files: Vec<DiffFile>,
}

impl RepositoryDiff {
    /// Parse complete output from `git diff --raw -z --patch`.
    ///
    /// # Errors
    /// Rejects malformed metadata, unsupported combined diffs and truncated hunks.
    pub fn parse(mut bytes: &[u8]) -> Result<Self, GitError> {
        if bytes.is_empty() {
            return Ok(Self { files: Vec::new() });
        }
        let mut files = Vec::new();
        loop {
            let (header, rest) = split_once(bytes, 0).ok_or_else(malformed)?;
            bytes = rest;
            if header.is_empty() {
                break;
            }
            if files.len() == MAX_RECORDS {
                return Err(GitError::ParsedLimitExceeded("diff file"));
            }
            let file = parse_file(header, &mut bytes)?;
            let width = file.objects[0].as_str().len();
            if file.objects[1].as_str().len() != width
                || files
                    .first()
                    .is_some_and(|first: &DiffFile| first.objects[0].as_str().len() != width)
            {
                return Err(malformed());
            }
            files.push(file);
        }
        let patch = bytes.strip_suffix(b"\n").ok_or_else(malformed)?;
        let mut lines = patch.split(|&byte| byte == b'\n').peekable();
        let mut file_index = 0;
        let mut line_budget = 500_000;
        let mut expect_added_half = false;
        while let Some(header) = lines.next() {
            if !header.starts_with(b"diff --git ") {
                return Err(malformed());
            }
            let file = files.get_mut(file_index).ok_or_else(malformed)?;
            let mut deleted_half = false;
            let mut added_half = false;
            while let Some(&line) = lines.peek() {
                if line.starts_with(b"diff --git ") {
                    break;
                }
                lines.next();
                if line.starts_with(b"@@ ") {
                    file.hunks
                        .push(parse_hunk(line, &mut lines, &mut line_budget)?);
                } else if line.starts_with(b"Binary files ") && line.ends_with(b" differ") {
                    file.binary = true;
                } else if line.starts_with(b"deleted file mode ")
                    && file.change == FileChange::TypeChanged
                {
                    deleted_half = true;
                } else if line.starts_with(b"new file mode ")
                    && file.change == FileChange::TypeChanged
                {
                    added_half = true;
                } else if ![
                    b"index ".as_slice(),
                    b"old mode ",
                    b"new mode ",
                    b"new file mode ",
                    b"deleted file mode ",
                    b"similarity index ",
                    b"dissimilarity index ",
                    b"rename from ",
                    b"rename to ",
                    b"copy from ",
                    b"copy to ",
                    b"--- ",
                    b"+++ ",
                ]
                .iter()
                .any(|prefix| line.starts_with(prefix))
                {
                    return Err(malformed());
                }
            }
            if expect_added_half && (!added_half || deleted_half) {
                return Err(malformed());
            }
            expect_added_half = deleted_half;
            if !expect_added_half {
                file_index += 1;
            }
        }
        if file_index != files.len() {
            return Err(malformed());
        }
        Ok(Self { files })
    }
}

fn malformed() -> GitError {
    GitError::MalformedOutput("two-way diff")
}

fn parse_file(header: &[u8], bytes: &mut &[u8]) -> Result<DiffFile, GitError> {
    let header = header.strip_prefix(b":").ok_or_else(malformed)?;
    let fields: Vec<_> = header.splitn(6, |&byte| byte == b' ').collect();
    let [
        old_mode,
        new_mode,
        old_object,
        new_object,
        [change, score @ ..],
    ] = fields.as_slice()
    else {
        return Err(malformed());
    };
    let change = FileChange::parse(*change)?;
    let score = if score.is_empty() {
        None
    } else {
        Some(decimal::<u8>(score, "diff score")?)
    };
    if score.is_some_and(|score| score > 100) {
        return Err(malformed());
    }
    let (path, rest) = split_once(bytes, 0).ok_or_else(malformed)?;
    let path = GitPath::parse(path)?;
    *bytes = rest;
    let (path, from) = if matches!(change, FileChange::Renamed | FileChange::Copied) {
        let (to, rest) = split_once(bytes, 0).ok_or_else(malformed)?;
        *bytes = rest;
        (GitPath::parse(to)?, Some(path))
    } else {
        (path, None)
    };
    Ok(DiffFile {
        path,
        from,
        change,
        score,
        modes: [mode(old_mode)?, mode(new_mode)?],
        objects: [ObjectId::parse(old_object)?, ObjectId::parse(new_object)?],
        binary: false,
        hunks: Vec::new(),
    })
}

fn range(value: &[u8], sign: u8) -> Result<(u32, u32), GitError> {
    let value = value.strip_prefix(&[sign]).ok_or_else(malformed)?;
    let (start, count) = split_once(value, b',').unwrap_or((value, b"1"));
    let (start, count): (u32, u32) = (decimal(start, "hunk range")?, decimal(count, "hunk range")?);
    if (count > 0 && start == 0) || start.checked_add(count).is_none() {
        return Err(malformed());
    }
    Ok((start, count))
}

fn parse_hunk<'a>(
    header: &[u8],
    input: &mut std::iter::Peekable<impl Iterator<Item = &'a [u8]>>,
    line_budget: &mut usize,
) -> Result<DiffHunk, GitError> {
    let header = header.strip_prefix(b"@@ ").ok_or_else(malformed)?;
    let (old, rest) = split_once(header, b' ').ok_or_else(malformed)?;
    let (new, rest) = split_once(rest, b' ').ok_or_else(malformed)?;
    let rest = rest.strip_prefix(b"@@").ok_or_else(malformed)?;
    let scope = if rest.is_empty() {
        rest
    } else {
        rest.strip_prefix(b" ").ok_or_else(malformed)?
    };
    let old = range(old, b'-')?;
    let new = range(new, b'+')?;
    if old.1 == 0 && new.1 == 0 {
        return Err(malformed());
    }
    if usize::try_from(old.1.max(new.1)).unwrap_or(usize::MAX) > *line_budget {
        return Err(GitError::ParsedLimitExceeded("diff line"));
    }
    let mut hunk = DiffHunk {
        old,
        new,
        scope: scope.to_vec(),
        lines: Vec::new(),
    };
    let (mut old_count, mut new_count) = (0, 0);
    while old_count < old.1 || new_count < new.1 {
        *line_budget = line_budget
            .checked_sub(1)
            .ok_or(GitError::ParsedLimitExceeded("diff line"))?;
        let [prefix, content @ ..] = input.next().ok_or_else(malformed)? else {
            return Err(malformed());
        };
        let (kind, on_old, on_new) = match prefix {
            b' ' => (DiffLineKind::Context, true, true),
            b'+' => (DiffLineKind::Added, false, true),
            b'-' => (DiffLineKind::Removed, true, false),
            _ => return Err(malformed()),
        };
        if (on_old && old_count == old.1) || (on_new && new_count == new.1) {
            return Err(malformed());
        }
        let has_newline = input
            .next_if(|&line| line == b"\\ No newline at end of file")
            .is_none();
        hunk.lines.push(DiffLine {
            kind,
            old_line: on_old.then_some(old.0 + old_count),
            new_line: on_new.then_some(new.0 + new_count),
            content: content.to_vec(),
            has_newline,
        });
        old_count += u32::from(on_old);
        new_count += u32::from(on_new);
    }
    Ok(hunk)
}
