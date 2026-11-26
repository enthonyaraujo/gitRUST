use crate::{
    GitError, HeadState, ObjectId, ReferenceName,
    model::{MAX_RECORDS, decimal, split_once},
};

/// An author or committer identity with its original bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    /// Display name in its original encoding.
    pub name: Vec<u8>,
    /// Email value in its original encoding.
    pub email: Vec<u8>,
    /// Seconds since the Unix epoch.
    pub unix_seconds: i64,
    /// Signed offset in minutes east of UTC.
    pub offset_minutes: i16,
}

/// One commit read from its size-framed Git object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitInfo {
    /// Full ID. Use `ObjectId::abbreviated` for display.
    pub id: ObjectId,
    /// Complete parent IDs in order.
    pub parents: Vec<ObjectId>,
    /// Original author.
    pub author: Signature,
    /// Committer who recorded the commit.
    pub committer: Signature,
    /// First message line in its original encoding.
    pub subject: Vec<u8>,
    /// Recognized signature header, with no verification assertion.
    pub signing: Option<CommitSignature>,
    /// Complete message bytes.
    pub message: Vec<u8>,
}

/// Signature data present in the commit object.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitSignature {
    /// `OpenPGP` armored signature.
    OpenPgp,
    /// SSH armored signature.
    Ssh,
    /// A signature in another format.
    Other,
}

/// Result of a bounded history page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct History {
    /// HEAD commit used as this page's root, if present.
    pub root: Option<ObjectId>,
    /// Commits in Git's history walk order.
    pub commits: Vec<CommitInfo>,
    /// Offset for another page if one was detected.
    pub next_offset: Option<u32>,
}

/// A local branch or remote-tracking ref.
#[allow(clippy::struct_excessive_bools)] // Symbolic aliases, remote tracking and checkout are independent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchInfo {
    /// Full reference.
    pub name: ReferenceName,
    /// Commit associated with the reference.
    pub id: ObjectId,
    /// Full configured upstream, if Git could resolve it.
    pub upstream: Option<ReferenceName>,
    /// True for the checked-out branch of this worktree.
    pub current: bool,
    /// Ahead and behind counts relative to an available local upstream.
    pub divergence: Option<(u64, u64)>,
    /// Git reports a checkout in some worktree.
    pub occupied: bool,
    /// This reference is symbolic.
    pub symbolic: bool,
    /// True for a remote-tracking reference.
    pub remote_tracking: bool,
}

/// Local and remote-tracking branches with the observed HEAD.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BranchList {
    /// Current HEAD state.
    pub head: HeadState,
    /// References in Git's order.
    pub branches: Vec<BranchInfo>,
}

impl BranchList {
    pub(crate) fn parse(head: HeadState, bytes: &[u8], width: usize) -> Result<Self, GitError> {
        let mut branches = Vec::new();
        for record in terminated_lines(bytes)? {
            if branches.len() == MAX_RECORDS {
                return Err(GitError::ParsedLimitExceeded("branch"));
            }
            let mut fields = record.split(|&byte| byte == 0);
            let name = ReferenceName::full(fields.next().ok_or_else(malformed)?)?;
            let id = object(fields.next().ok_or_else(malformed)?, width)?;
            let upstream = fields.next().ok_or_else(malformed)?;
            let upstream = if upstream.is_empty() {
                None
            } else {
                Some(ReferenceName::full(upstream)?)
            };
            let tracking = fields.next().ok_or_else(malformed)?;
            let occupied = !fields.next().ok_or_else(malformed)?.is_empty();
            let symbolic = !fields.next().ok_or_else(malformed)?.is_empty();
            if fields.next() != Some(b"".as_slice()) || fields.next().is_some() {
                return Err(malformed());
            }
            let remote_tracking = name.as_bytes().starts_with(b"refs/remotes/");
            if !remote_tracking && !name.as_bytes().starts_with(b"refs/heads/") {
                return Err(malformed());
            }
            let current = matches!(&head, HeadState::Attached(branch, _) if branch == &name);
            branches.push(BranchInfo {
                name,
                id,
                current,
                remote_tracking,
                divergence: if upstream.is_some() && tracking.is_empty() {
                    Some((0, 0))
                } else {
                    divergence(tracking)?
                },
                occupied,
                symbolic,
                upstream,
            });
        }
        Ok(Self { head, branches })
    }
}

fn divergence(bytes: &[u8]) -> Result<Option<(u64, u64)>, GitError> {
    if bytes.is_empty() || bytes == b"[gone]" {
        return Ok(None);
    }
    let values = bytes
        .strip_prefix(b"[")
        .and_then(|bytes| bytes.strip_suffix(b"]"))
        .ok_or_else(malformed)?;
    let mut counts = (0, 0);
    for part in values.split(|&byte| byte == b',') {
        let part = part.strip_prefix(b" ").unwrap_or(part);
        if let Some(value) = part.strip_prefix(b"ahead ") {
            counts.0 = decimal(value, "ahead count")?;
        } else if let Some(value) = part.strip_prefix(b"behind ") {
            counts.1 = decimal(value, "behind count")?;
        } else {
            return Err(malformed());
        }
    }
    Ok(Some(counts))
}

pub(crate) fn terminated_lines(bytes: &[u8]) -> Result<impl Iterator<Item = &[u8]>, GitError> {
    let take = if bytes.is_empty() { 0 } else { usize::MAX };
    let bytes = if bytes.is_empty() {
        bytes
    } else {
        bytes.strip_suffix(b"\n").ok_or_else(malformed)?
    };
    Ok(bytes.split(|&byte| byte == b'\n').take(take))
}

pub(crate) fn object(bytes: &[u8], width: usize) -> Result<ObjectId, GitError> {
    if bytes.len() != width {
        return Err(malformed());
    }
    ObjectId::parse(bytes)
}

fn malformed() -> GitError {
    GitError::MalformedOutput("commit or reference record")
}

pub(crate) fn commits(mut bytes: &[u8], ids: Vec<ObjectId>) -> Result<Vec<CommitInfo>, GitError> {
    let mut commits = Vec::with_capacity(ids.len());
    for id in ids {
        let (header, rest) = split_once(bytes, b'\n').ok_or_else(malformed)?;
        let fields: Vec<_> = header.splitn(4, |&byte| byte == b' ').collect();
        let [oid, b"commit", size] = fields.as_slice() else {
            return Err(malformed());
        };
        if *oid != id.as_str().as_bytes() {
            return Err(malformed());
        }
        let size: usize = decimal(size, "commit length")?;
        let (body, rest) = rest.split_at_checked(size).ok_or_else(malformed)?;
        bytes = rest.strip_prefix(b"\n").ok_or_else(malformed)?;
        commits.push(parse_commit(id, body)?);
    }
    if !bytes.is_empty() {
        return Err(malformed());
    }
    Ok(commits)
}

pub(crate) fn parse_commit(id: ObjectId, bytes: &[u8]) -> Result<CommitInfo, GitError> {
    let boundary = bytes
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .ok_or_else(malformed)?;
    let width = id.as_str().len();
    let mut parents = Vec::new();
    let mut author = None;
    let mut committer = None;
    let mut signing = None;
    let mut headers = bytes[..boundary].split(|&byte| byte == b'\n');
    let tree = headers
        .next()
        .and_then(|line| line.strip_prefix(b"tree "))
        .ok_or_else(malformed)?;
    object(tree, width)?;
    for header in headers {
        if let Some(oid) = header.strip_prefix(b"parent ") {
            parents.push(object(oid, width)?);
        } else if let Some(raw) = header
            .strip_prefix(b"gpgsig ")
            .or_else(|| header.strip_prefix(b"gpgsig-sha256 "))
        {
            signing = Some(match raw {
                b"-----BEGIN PGP SIGNATURE-----" => CommitSignature::OpenPgp,
                b"-----BEGIN SSH SIGNATURE-----" => CommitSignature::Ssh,
                _ => CommitSignature::Other,
            });
        } else if let Some(raw) = header.strip_prefix(b"author ") {
            if author.replace(signature(raw)?).is_some() {
                return Err(malformed());
            }
        } else if let Some(raw) = header.strip_prefix(b"committer ")
            && committer.replace(signature(raw)?).is_some()
        {
            return Err(malformed());
        }
    }
    let message = &bytes[boundary + 2..];
    let subject = split_once(message, b'\n').map_or(message, |(subject, _)| subject);
    Ok(CommitInfo {
        id,
        parents,
        author: author.ok_or_else(malformed)?,
        committer: committer.ok_or_else(malformed)?,
        subject: subject.to_vec(),
        message: message.to_vec(),
        signing,
    })
}

fn signature(raw: &[u8]) -> Result<Signature, GitError> {
    let email_end = raw
        .iter()
        .rposition(|&byte| byte == b'>')
        .ok_or_else(malformed)?;
    let profile = &raw[..email_end];
    let email_start = profile
        .iter()
        .rposition(|&byte| byte == b'<')
        .ok_or_else(malformed)?;
    let time = raw[email_end + 1..]
        .strip_prefix(b" ")
        .ok_or_else(malformed)?;
    let (seconds, offset) = split_once(time, b' ').ok_or_else(malformed)?;
    let [sign @ (b'+' | b'-'), hours @ .., minute_tens, minute_ones] = offset else {
        return Err(malformed());
    };
    if hours.len() != 2 || !offset[1..].iter().all(u8::is_ascii_digit) {
        return Err(malformed());
    }
    let hours: i16 = decimal(hours, "timezone hours")?;
    let minutes: i16 = decimal(&[*minute_tens, *minute_ones], "timezone minutes")?;
    if hours > 23 || minutes > 59 {
        return Err(malformed());
    }
    let offset_minutes = (hours * 60 + minutes) * if *sign == b'-' { -1 } else { 1 };
    let name = profile[..email_start]
        .strip_suffix(b" ")
        .ok_or_else(malformed)?;
    Ok(Signature {
        name: name.to_vec(),
        email: profile[email_start + 1..].to_vec(),
        unix_seconds: decimal(seconds, "commit time")?,
        offset_minutes,
    })
}
