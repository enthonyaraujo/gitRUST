use std::{collections::HashSet, fmt, path::PathBuf, sync::Arc};

use gitrust_core::changes::{
    ChangeKey, ChangePreview, ChangeSide, ChangedPath, CommitDraft, PatchHunk, PatchLine, PatchRow,
    PatchSelection,
};

use crate::{
    DiffFile, DiffLine, DiffLineKind, EntryKind, FileChange, GitError, OperationSafetyClass,
    RepositoryStatus,
};

/// A patch read by the engine. Its original text and hunks never come back from UI input.
#[derive(Clone, Eq, PartialEq)]
pub struct ChangePatch {
    pub(crate) key: ChangeKey,
    pub(crate) raw: Vec<u8>,
    pub(crate) file: Option<DiffFile>,
}

impl fmt::Debug for ChangePatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangePatch")
            .field("side", &self.key.side)
            .finish_non_exhaustive()
    }
}

impl ChangePatch {
    /// The affected path and comparison layer.
    #[must_use]
    pub const fn key(&self) -> &ChangeKey {
        &self.key
    }

    /// Construct the separate text projection for the native UI.
    #[must_use]
    pub fn display(&self) -> ChangePreview {
        let hunks = self
            .file
            .iter()
            .flat_map(|file| &file.hunks)
            .map(|hunk| {
                let mut header = format!(
                    "@@ -{},{} +{},{} @@",
                    hunk.old.0, hunk.old.1, hunk.new.0, hunk.new.1
                );
                if !hunk.scope.is_empty() {
                    header.push(' ');
                    header.push_str(&String::from_utf8_lossy(&hunk.scope));
                }
                PatchHunk {
                    header,
                    lines: hunk
                        .lines
                        .iter()
                        .map(|line| PatchLine {
                            old: line.old_line,
                            new: line.new_line,
                            text: String::from_utf8_lossy(&line.content).into_owned(),
                            newline: line.has_newline,
                        })
                        .collect(),
                }
            })
            .collect::<Vec<_>>();
        let text = String::from_utf8_lossy(&self.raw).into_owned();
        let metadata = if hunks.is_empty() {
            text.lines().collect::<Vec<_>>()
        } else {
            text.lines()
                .take_while(|line| !line.starts_with("@@ "))
                .filter(|line| {
                    !line.starts_with("diff --git ")
                        && !line.starts_with("index ")
                        && !line.starts_with("--- ")
                        && !line.starts_with("+++ ")
                })
                .collect()
        };
        let columns = metadata
            .iter()
            .copied()
            .chain(hunks.iter().flat_map(|hunk| {
                std::iter::once(hunk.header.as_str())
                    .chain(hunk.lines.iter().map(|line| line.text.as_str()))
            }))
            .map(columns)
            .max()
            .unwrap_or(0);
        let mut unified: Vec<_> = (0..metadata.len()).map(PatchRow::Metadata).collect();
        let mut split = unified.clone();
        for (index, hunk) in hunks.iter().enumerate() {
            project_hunk(index, &hunk.lines, &mut unified, &mut split);
        }
        let metadata = metadata.into_iter().map(str::to_owned).collect();
        ChangePreview {
            key: self.key.clone(),
            text,
            metadata,
            unified,
            split,
            columns,
            binary: self.file.as_ref().is_some_and(|file| file.binary),
            partial: self.partial_file().is_some(),
            hunks,
        }
    }

    fn partial_file(&self) -> Option<&DiffFile> {
        self.file.as_ref().filter(|file| {
            let regular = |mode| matches!(mode, 0o100_644 | 0o100_755);
            self.key.side != ChangeSide::Conflict
                && !file.binary
                && !file.hunks.is_empty()
                && file.from.is_none()
                && match file.change {
                    FileChange::Modified => {
                        regular(file.modes[0]) && file.modes[0] == file.modes[1]
                    }
                    FileChange::Added => file.modes[0] == 0 && regular(file.modes[1]),
                    FileChange::Deleted => regular(file.modes[0]) && file.modes[1] == 0,
                    _ => false,
                }
        })
    }

    pub(crate) fn subset(&self, selections: &[PatchSelection]) -> Result<Vec<u8>, GitError> {
        let file = self
            .partial_file()
            .ok_or(GitError::UnsupportedRepositoryFeature(
                "partial changes for this file",
            ))?;
        validate_choices(file, selections)?;
        let choices: HashSet<_> = selections.iter().copied().collect();
        let hunks: HashSet<_> = choices.iter().map(|choice| choice.hunk).collect();
        let selected = |hunk, line| {
            choices.contains(&PatchSelection { hunk, line: None })
                || choices.contains(&PatchSelection {
                    hunk,
                    line: Some(line),
                })
        };
        let reverse = self.key.side == ChangeSide::Staged;
        let (base_mode, target_mode) = if reverse {
            (file.modes[1], file.modes[0])
        } else {
            (file.modes[0], file.modes[1])
        };
        let deletes = target_mode == 0
            && file.hunks.iter().enumerate().all(|(hunk, data)| {
                data.lines
                    .iter()
                    .enumerate()
                    .all(|(line, _)| selected(hunk, line))
            });
        let mut patch = patch_header(file, base_mode, target_mode, deletes);
        let mut delta = 0i64;
        for (position, hunk) in file.hunks.iter().enumerate() {
            if !hunks.contains(&position) {
                continue;
            }
            let mut counts = [0u32; 2];
            let mut body = Vec::new();
            let mut target_has_ended = false;
            for (index, line) in hunk.lines.iter().enumerate() {
                let selected = selected(position, index);
                let kind = match (line.kind, reverse) {
                    (DiffLineKind::Added, false) | (DiffLineKind::Removed, true) => {
                        DiffLineKind::Added
                    }
                    (DiffLineKind::Removed, false) | (DiffLineKind::Added, true) => {
                        DiffLineKind::Removed
                    }
                    (DiffLineKind::Context, _) => DiffLineKind::Context,
                };
                let writes_target = match kind {
                    DiffLineKind::Added => selected,
                    DiffLineKind::Removed => !selected,
                    DiffLineKind::Context => true,
                };
                if writes_target && target_has_ended {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "selected lines would follow a missing newline",
                    ));
                }
                target_has_ended |= writes_target && !line.has_newline;
                match (kind, selected) {
                    (DiffLineKind::Added, false) => {}
                    (DiffLineKind::Added, true) => {
                        counts[1] += 1;
                        push_line(&mut body, b'+', line);
                    }
                    (DiffLineKind::Removed, true) => {
                        counts[0] += 1;
                        push_line(&mut body, b'-', line);
                    }
                    _ => {
                        counts[0] += 1;
                        counts[1] += 1;
                        push_line(&mut body, b' ', line);
                    }
                }
            }
            let base = if reverse { hunk.new } else { hunk.old };
            let before = i64::from(base.0) - i64::from(base.1 != 0);
            let old_start = before + i64::from(counts[0] != 0);
            let new_start = before + delta + i64::from(counts[1] != 0);
            patch.extend_from_slice(
                format!(
                    "@@ -{old_start},{} +{new_start},{} @@\n",
                    counts[0], counts[1]
                )
                .as_bytes(),
            );
            patch.extend_from_slice(&body);
            delta += i64::from(counts[1]) - i64::from(counts[0]);
        }
        Ok(patch)
    }
}

fn validate_choices(file: &DiffFile, selections: &[PatchSelection]) -> Result<(), GitError> {
    if selections.is_empty()
        || selections.iter().any(|selection| {
            file.hunks.get(selection.hunk).is_none_or(|hunk| {
                selection.line.is_some_and(|index| {
                    hunk.lines
                        .get(index)
                        .is_none_or(|line| line.kind == DiffLineKind::Context)
                })
            })
        })
    {
        return Err(GitError::InvalidArgument("patch selection"));
    }
    Ok(())
}

fn patch_header(file: &DiffFile, base_mode: u32, target_mode: u32, deletes: bool) -> Vec<u8> {
    let filename = file.path.as_bytes();
    let old = quoted_path(b'a', filename);
    let new = quoted_path(b'b', filename);
    let modes = if base_mode == 0 {
        format!("new file mode {target_mode:06o}\n")
    } else if deletes {
        format!("deleted file mode {base_mode:06o}\n")
    } else {
        String::new()
    };
    let source = if base_mode == 0 { "/dev/null" } else { &old };
    let destination = if deletes { "/dev/null" } else { &new };
    format!("diff --git {old} {new}\n{modes}--- {source}\n+++ {destination}\n").into_bytes()
}

fn columns(text: &str) -> usize {
    text.chars().fold(0, |column, character| {
        column
            + match character {
                '\t' => 4 - column % 4,
                character if character.is_ascii() => 1,
                _ => 2,
            }
    })
}

fn project_hunk(
    index: usize,
    lines: &[PatchLine],
    unified: &mut Vec<PatchRow>,
    split: &mut Vec<PatchRow>,
) {
    unified.push(PatchRow::Hunk(index));
    split.push(PatchRow::Hunk(index));
    let mut first = 0;
    while first < lines.len() {
        let line = &lines[first];
        if line.old.is_some() && line.new.is_some() {
            let row = PatchRow::Lines(index, [Some(first); 2]);
            unified.push(row);
            split.push(row);
            if !line.newline {
                let row = PatchRow::NoNewline([true, true]);
                unified.push(row);
                split.push(row);
            }
            first += 1;
            continue;
        }
        let last = first
            + lines[first..]
                .iter()
                .take_while(|line| line.old.is_none() || line.new.is_none())
                .count();
        let mut old = Vec::new();
        let mut new = Vec::new();
        for (offset, line) in lines[first..last].iter().enumerate() {
            let position = first + offset;
            let removed = line.old.is_some();
            if removed {
                old.push(position);
            } else {
                new.push(position);
            }
        }
        for offset in 0..old.len().max(new.len()) {
            let pair = [old.get(offset).copied(), new.get(offset).copied()];
            split.push(PatchRow::Lines(index, pair));
            let marker =
                pair.map(|position| position.is_some_and(|position| !lines[position].newline));
            if marker.into_iter().any(|missing| missing) {
                split.push(PatchRow::NoNewline(marker));
            }
        }
        // Preserve Git's order in unified mode and associate each line with
        // its split counterpart for within-line highlighting.
        for (side, positions) in [&old, &new].into_iter().enumerate() {
            for (offset, &position) in positions.iter().enumerate() {
                let pair = [old.get(offset).copied(), new.get(offset).copied()];
                unified.push(PatchRow::Lines(
                    index,
                    if side == 0 { pair } else { [pair[1], pair[0]] },
                ));
                if !lines[position].newline {
                    unified.push(PatchRow::NoNewline([true, false]));
                }
            }
        }
        first = last;
    }
}

fn quoted_path(prefix: u8, path: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("\"");
    out.push(char::from(prefix));
    out.push('/');
    for byte in path {
        match byte {
            b' '..=b'~' if !matches!(byte, b'"' | b'\\') => out.push(char::from(*byte)),
            _ => {
                let _ = write!(out, "\\{byte:03o}");
            }
        }
    }
    out.push('"');
    out
}

fn push_line(buffer: &mut Vec<u8>, mark: u8, line: &DiffLine) {
    buffer.push(mark);
    buffer.extend_from_slice(&line.content);
    buffer.push(b'\n');
    if !line.has_newline {
        buffer.extend_from_slice(b"\\ No newline at end of file\n");
    }
}

/// A local operation and the complete last status shown to the caller.
#[derive(Debug, Clone)]
pub struct Mutation {
    /// Captured status and index object identities.
    pub expected: Arc<RepositoryStatus>,
    /// Action approved against that status.
    pub action: MutationAction,
}

/// Typed allowlist of mutating operations.
#[derive(Clone)]
pub enum MutationAction {
    /// Update the index from selected worktree paths.
    Stage(Vec<ChangeKey>),
    /// Reset the selected entries in the index.
    Unstage(Vec<ChangeKey>),
    /// Apply only selected changes from an engine-issued patch.
    Partial {
        /// Original, byte-preserving patch.
        patch: Arc<ChangePatch>,
        /// Approved hunks and lines.
        selection: Vec<PatchSelection>,
    },
    /// Discard exactly the changes explicitly confirmed by the user.
    Discard(Vec<ChangePatch>),
    /// Commit the index or rewrite HEAD if explicitly requested.
    Commit(CommitDraft),
}

impl fmt::Debug for MutationAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Stage(_) => "Stage",
            Self::Unstage(_) => "Unstage",
            Self::Partial { .. } => "Partial",
            Self::Discard(_) => "Discard",
            Self::Commit(_) => "Commit",
        })
    }
}

impl Mutation {
    pub(crate) const fn safety_class(&self) -> OperationSafetyClass {
        if matches!(
            &self.action,
            MutationAction::Discard(_) | MutationAction::Commit(CommitDraft { amend: true, .. })
        ) {
            OperationSafetyClass::DestructiveWrite
        } else {
            OperationSafetyClass::SerializedWrite
        }
    }
}

/// Convert actual porcelain entries into separate rows for both layers.
///
/// # Errors
/// Returns an error for paths unrepresentable on the current platform.
pub fn change_rows(status: &RepositoryStatus) -> Result<Vec<ChangedPath>, GitError> {
    let mut rows = Vec::with_capacity(status.entries.len());
    for entry in &status.entries {
        let path = entry.path.to_path_buf()?;
        let sources = match &entry.kind {
            EntryKind::RenamedOrCopied { from, .. } => Some(from.to_path_buf()?),
            _ => None,
        };
        let modes = entry.modes.unwrap_or_default();
        let mut add = |side, status, mode_changed| {
            rows.push(ChangedPath {
                key: ChangeKey {
                    path: path.clone(),
                    side,
                },
                status,
                from: matches!(status, 'R' | 'C')
                    .then(|| sources.clone())
                    .flatten(),
                mode_changed,
            });
        };
        match entry.kind {
            EntryKind::Untracked => add(ChangeSide::Untracked, '?', false),
            EntryKind::Unmerged { .. } => add(ChangeSide::Conflict, 'U', false),
            EntryKind::Ignored => {}
            _ => {
                if let Some(mark) = entry.index.and_then(change_letter) {
                    add(
                        ChangeSide::Staged,
                        mark,
                        modes[0] != modes[1] && modes[0] != 0 && modes[1] != 0,
                    );
                }
                if let Some(mark) = entry.worktree.and_then(change_letter) {
                    add(
                        ChangeSide::Unstaged,
                        mark,
                        modes[1] != modes[2] && modes[1] != 0 && modes[2] != 0,
                    );
                }
            }
        }
    }
    Ok(rows)
}

const fn change_letter(change: FileChange) -> Option<char> {
    match change {
        FileChange::Unchanged => None,
        FileChange::Added => Some('A'),
        FileChange::Modified => Some('M'),
        FileChange::Deleted => Some('D'),
        FileChange::Renamed => Some('R'),
        FileChange::Copied => Some('C'),
        FileChange::TypeChanged => Some('T'),
        FileChange::Unmerged => Some('U'),
        FileChange::Unknown => Some('X'),
        FileChange::Broken => Some('B'),
    }
}

pub(crate) fn path_operands(
    key: &ChangeKey,
    rows: &[ChangedPath],
) -> Result<Vec<PathBuf>, GitError> {
    let row = rows
        .iter()
        .find(|row| row.key == *key)
        .ok_or(GitError::StaleSnapshot)?;
    let mut paths = vec![key.path.clone()];
    if row.status == 'R'
        && let Some(old) = &row.from
    {
        paths.push(old.clone());
    }
    Ok(paths)
}
