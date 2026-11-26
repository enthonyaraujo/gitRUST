use std::{
    ffi::OsStr,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use crate::{
    GitError, HeadState, ObjectId, ReferenceName,
    command::has_nul,
    model::{allowed_path, path_from_bytes},
    process::Control,
};

const POINTER_BYTES: u64 = 16 * 1024;
const CONFIG_BYTES: u64 = 1024 * 1024;

/// Canonical common directory shared by a repository's linked worktrees.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RepositoryIdentity(PathBuf);

impl RepositoryIdentity {
    /// Canonical common Git directory.
    #[must_use]
    pub fn common_dir(&self) -> &Path {
        &self.0
    }
}

/// Filesystem identification only. It does not permit a Git invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryCandidate {
    location: PathBuf,
    worktree: Option<PathBuf>,
    git_dir: PathBuf,
    identity: RepositoryIdentity,
}

/// An application decision that the identified metadata is local, controlled
/// by the user and cannot be concurrently changed by an untrusted writer.
/// The engine still validates topology, ownership and configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedRepository(RepositoryCandidate);

/// Verified repository context and the observed HEAD.
#[derive(Debug, Eq, PartialEq)]
pub struct RepositoryInfo {
    /// An endpoint is configured in local metadata. Addresses remain private.
    pub has_remote: bool,
    /// Canonical root, directories and shared identity.
    pub repository: RepositoryCandidate,
    /// Symbolic, detached or unborn state at the time of observation.
    pub head: HeadState,
    /// Storage object format reported by Git.
    pub object_format: String,
}

impl RepositoryCandidate {
    /// Working tree root. Bare repositories return `None`.
    #[must_use]
    pub fn worktree(&self) -> Option<&Path> {
        self.worktree.as_deref()
    }

    /// Git directory specific to this worktree.
    #[must_use]
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Shared canonical identity used by the scheduler.
    #[must_use]
    pub const fn identity(&self) -> &RepositoryIdentity {
        &self.identity
    }

    /// Turn an explicit caller trust decision into a scoped capability.
    ///
    /// Call this only after obtaining authorization to execute local Git in
    /// this repository. This still does not authorize scripts or network use.
    #[must_use]
    pub fn authorize_local_metadata(self) -> TrustedRepository {
        TrustedRepository(self)
    }

    pub(crate) fn discover(path: &Path, control: Control<'_>) -> Result<Self, GitError> {
        if path.as_os_str().is_empty() || has_nul(path.as_os_str()) {
            return Err(GitError::InvalidArgument("repository path"));
        }
        let path = canonical(path)?;
        if !directory(&path)? {
            return Err(GitError::InvalidRepository);
        }
        for dir in path.ancestors() {
            control.check()?;
            if let Some(meta) = metadata(&dir.join(".git"))? {
                return Self::worktree_at(dir, &meta);
            }
            if metadata(&dir.join("HEAD"))?.is_some()
                && metadata(&dir.join("objects"))?.is_some()
                && metadata(&dir.join("config"))?.is_some()
            {
                validate_layout(dir, dir)?;
                return Ok(Self {
                    location: dir.into(),
                    worktree: None,
                    git_dir: dir.into(),
                    identity: RepositoryIdentity(dir.into()),
                });
            }
        }
        Err(GitError::InvalidRepository)
    }

    fn worktree_at(root: &Path, marker: &fs::Metadata) -> Result<Self, GitError> {
        let entry = root.join(".git");
        if is_link(marker) {
            return Err(GitError::SecurityPolicyViolation("linked .git entry"));
        }
        let (git_dir, common) = if marker.is_dir() {
            (entry.clone(), entry)
        } else if marker.is_file() {
            let bytes = control_file(&entry, POINTER_BYTES)?;
            let bytes = bytes
                .strip_prefix(b"gitdir: ")
                .ok_or(GitError::InvalidRepository)?;
            let git_dir = resolve_pointer(root, bytes)?;
            let common_pointer = git_dir.join("commondir");
            if metadata(&common_pointer)?.is_none() {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "submodule or separate Git directory",
                ));
            }
            let common = resolve_pointer(&git_dir, &control_file(&common_pointer, POINTER_BYTES)?)?;
            let worktrees = common.join("worktrees");
            if git_dir.parent() != Some(worktrees.as_path()) || !directory(&worktrees)? {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "nonstandard worktree metadata",
                ));
            }
            let backlink = resolve_pointer(
                &git_dir,
                &control_file(&git_dir.join("gitdir"), POINTER_BYTES)?,
            )?;
            if backlink != entry {
                return Err(GitError::SecurityPolicyViolation(
                    "worktree metadata backlink mismatch",
                ));
            }
            (git_dir, common)
        } else {
            return Err(GitError::SecurityPolicyViolation("special .git entry"));
        };
        validate_layout(&git_dir, &common)?;
        Ok(Self {
            location: root.into(),
            worktree: Some(root.into()),
            git_dir,
            identity: RepositoryIdentity(common),
        })
    }

    pub(crate) fn published_worktree(location: PathBuf) -> Self {
        let git_dir = location.join(".git");
        Self {
            worktree: Some(location.clone()),
            location,
            identity: RepositoryIdentity(git_dir.clone()),
            git_dir,
        }
    }

    pub(crate) fn location(&self) -> &Path {
        &self.location
    }
}

impl TrustedRepository {
    /// The candidate for which trust was granted.
    #[must_use]
    pub const fn repository(&self) -> &RepositoryCandidate {
        &self.0
    }

    pub(crate) fn preflight(&self, control: Control<'_>) -> Result<Vec<PathBuf>, GitError> {
        let original = &self.0;
        let current = RepositoryCandidate::discover(original.location(), control)?;
        if current != *original {
            return Err(GitError::SecurityPolicyViolation(
                "repository topology changed",
            ));
        }
        let common = original.identity.common_dir();
        let git_dir = original.git_dir();
        for locked in [
            git_dir.join("index.lock"),
            git_dir.join("HEAD.lock"),
            common.join("packed-refs.lock"),
            common.join("config.lock"),
        ] {
            if metadata(&locked)?.is_some() {
                return Err(GitError::RepositoryLocked);
            }
        }
        let mut config = vec![common.join("config")];
        for path in [
            common.join("config.worktree"),
            git_dir.join("config.worktree"),
        ] {
            if !config.contains(&path) && metadata(&path)?.is_some() {
                config.push(path);
            }
        }
        for path in &config {
            let meta = required_regular(path)?;
            if meta.len() > CONFIG_BYTES {
                return Err(GitError::SecurityPolicyViolation(
                    "oversized repository config",
                ));
            }
        }
        for directory_path in [
            common.join("objects/info"),
            common.join("info"),
            git_dir.join("info"),
        ] {
            if metadata(&directory_path)?.is_some() && !directory(&directory_path)? {
                return Err(GitError::InvalidRepository);
            }
        }
        for path in [
            common.join("objects/info/alternates"),
            common.join("objects/info/http-alternates"),
            common.join("info/grafts"),
            common.join("shallow"),
        ] {
            if metadata(&path)?.is_some() {
                return Err(GitError::UnsupportedRepositoryFeature(
                    "external object source or shallow boundary",
                ));
            }
        }
        for path in [git_dir.join("commondir"), git_dir.join("gitdir")] {
            if git_dir == common && metadata(&path)?.is_some() {
                return Err(GitError::SecurityPolicyViolation(
                    "unexpected repository indirection",
                ));
            }
        }
        for path in [
            git_dir.join("index"),
            common.join("packed-refs"),
            git_dir.join("info/attributes"),
            common.join("info/attributes"),
            common.join("info/exclude"),
        ] {
            if metadata(&path)?.is_some() {
                required_regular(&path)?;
            }
        }
        for path in [
            common.join("objects"),
            common.join("refs"),
            git_dir.join("refs"),
            common.join("info"),
            git_dir.join("info"),
        ] {
            if metadata(&path)?.is_some() {
                validate_tree(&path, control)?;
            }
        }
        for base in [common, git_dir] {
            for entry in fs::read_dir(base).map_err(GitError::io)? {
                let entry = entry.map_err(GitError::io)?;
                if entry
                    .file_name()
                    .as_encoded_bytes()
                    .starts_with(b"sharedindex.")
                {
                    required_regular(&entry.path())?;
                }
            }
        }
        Ok(config)
    }

    pub(crate) fn head_reference(&self) -> Result<Option<ReferenceName>, GitError> {
        let bytes = control_file(&self.0.git_dir.join("HEAD"), POINTER_BYTES)?;
        let bytes = trim_file_eol(&bytes);
        if let Some(name) = bytes.strip_prefix(b"ref: ") {
            return ReferenceName::full(name).map(Some);
        }
        ObjectId::parse(bytes)?;
        Ok(None)
    }
}

fn canonical(path: &Path) -> Result<PathBuf, GitError> {
    if !allowed_path(path) {
        return Err(GitError::UnsupportedRepositoryFeature(
            "network or device path",
        ));
    }
    let canonical = fs::canonicalize(path).map_err(GitError::io)?;
    if !allowed_path(&canonical) {
        return Err(GitError::UnsupportedRepositoryFeature(
            "network or device path",
        ));
    }
    Ok(canonical)
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>, GitError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(GitError::io(e)),
    }
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    meta.file_type().is_symlink()
}

fn directory(path: &Path) -> Result<bool, GitError> {
    if let Some(meta) = metadata(path)? {
        if is_link(&meta) {
            return Err(GitError::SecurityPolicyViolation(
                "linked metadata directory",
            ));
        }
        Ok(meta.is_dir())
    } else {
        Ok(false)
    }
}

fn required_regular(path: &Path) -> Result<fs::Metadata, GitError> {
    let meta = metadata(path)?.ok_or(GitError::InvalidRepository)?;
    if is_link(&meta) || !meta.is_file() {
        return Err(GitError::SecurityPolicyViolation(
            "linked or special metadata file",
        ));
    }
    Ok(meta)
}

fn control_file(path: &Path, max: u64) -> Result<Vec<u8>, GitError> {
    let meta = required_regular(path)?;
    if meta.len() > max {
        return Err(GitError::SecurityPolicyViolation("oversized metadata file"));
    }
    let mut buffer = Vec::new();
    fs::File::open(path)
        .map_err(GitError::io)?
        .take(max + 1)
        .read_to_end(&mut buffer)
        .map_err(GitError::io)?;
    if u64::try_from(buffer.len()).unwrap_or(u64::MAX) > max {
        return Err(GitError::SecurityPolicyViolation("oversized metadata file"));
    }
    Ok(buffer)
}

fn validate_layout(git: &Path, common: &Path) -> Result<(), GitError> {
    if !directory(git)? || !directory(common)? || !directory(&common.join("objects"))? {
        return Err(GitError::InvalidRepository);
    }
    required_regular(&git.join("HEAD"))?;
    required_regular(&common.join("config"))?;
    if !directory(&common.join("refs"))? {
        return Err(GitError::UnsupportedRepositoryFeature(
            "non-files reference storage",
        ));
    }
    Ok(())
}

fn trim_file_eol(mut bytes: &[u8]) -> &[u8] {
    while let Some(rest) = bytes
        .strip_suffix(b"\n")
        .or_else(|| bytes.strip_suffix(b"\r"))
    {
        bytes = rest;
    }
    bytes
}

fn resolve_pointer(base: &Path, bytes: &[u8]) -> Result<PathBuf, GitError> {
    let bytes = trim_file_eol(bytes);
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(GitError::InvalidRepository);
    }
    let path = base.join(path_from_bytes(bytes)?);
    let actual = canonical(&path)?;
    // Inspect the endpoint itself before accepting a canonical identity.
    if metadata(&path)?.is_some_and(|meta| is_link(&meta)) {
        return Err(GitError::SecurityPolicyViolation(
            "linked worktree metadata endpoint",
        ));
    }
    Ok(actual)
}

fn validate_tree(path: &Path, control: Control<'_>) -> Result<(), GitError> {
    fn visit(path: &Path, depth: u8, control: Control<'_>) -> Result<(), GitError> {
        control.check()?;
        if depth > 64 {
            return Err(GitError::UnsupportedRepositoryFeature(
                "excessive metadata depth",
            ));
        }
        if !directory(path)? {
            return Err(GitError::InvalidRepository);
        }
        for entry in fs::read_dir(path).map_err(GitError::io)? {
            control.check()?;
            let entry = entry.map_err(GitError::io)?;
            let meta = fs::symlink_metadata(entry.path()).map_err(GitError::io)?;
            if is_link(&meta) || !(meta.is_dir() || meta.is_file()) {
                return Err(GitError::SecurityPolicyViolation(
                    "linked or special metadata entry",
                ));
            }
            if entry.path().extension() == Some(OsStr::new("promisor")) {
                return Err(GitError::UnsupportedRepositoryFeature("promisor objects"));
            }
            if meta.is_dir() {
                visit(&entry.path(), depth + 1, control)?;
            }
        }
        Ok(())
    }
    visit(path, 0, control)
}

pub(crate) fn validate_config(bytes: &[u8]) -> Result<bool, GitError> {
    let mut has_remote = false;
    for record in crate::model::records(bytes)? {
        let (key, value) = crate::model::split_once(record, b'\n').unwrap_or((record, &[]));
        if key.starts_with(b"include.") || key.starts_with(b"includeif.") {
            return Err(GitError::SecurityPolicyViolation("local config include"));
        }
        if key.ends_with(b".promisor")
            || key.ends_with(b".partialclonefilter")
            || key == b"extensions.partialclone"
        {
            return Err(GitError::UnsupportedRepositoryFeature("partial clone"));
        }
        let safe = match key {
            b"core.repositoryformatversion" => matches!(value, b"0" | b"1"),
            b"lfs.repositoryformatversion" => value == b"0",
            b"extensions.objectformat" => matches!(value, b"sha1" | b"sha256"),
            b"extensions.worktreeconfig" | b"extensions.preciousobjects" => {
                matches!(value, b"true" | b"false")
            }
            b"extensions.refstorage" => value == b"files",
            b"core.bare"
            | b"core.filemode"
            | b"core.logallrefupdates"
            | b"core.ignorecase"
            | b"core.symlinks"
            | b"core.precomposeunicode"
            | b"core.autocrlf"
            | b"core.eol"
            | b"core.safecrlf"
            | b"core.trustctime"
            | b"core.checkstat"
            | b"core.abbrev"
            | b"core.sharedrepository"
            | b"user.name"
            | b"user.email"
            | b"init.defaultbranch" => true,
            _ => {
                section_key(
                    key,
                    b"remote.",
                    &[b"url", b"fetch", b"pushurl", b"push", b"tagopt"],
                ) || section_key(
                    key,
                    b"branch.",
                    &[b"remote", b"merge", b"pushremote", b"description"],
                )
            }
        };
        if !safe {
            return Err(GitError::SecurityPolicyViolation(
                "non-allowlisted local configuration",
            ));
        }
        has_remote |= !value.is_empty() && section_key(key, b"remote.", &[b"url", b"pushurl"]);
    }
    Ok(has_remote)
}

fn section_key(key: &[u8], prefix: &[u8], fields: &[&[u8]]) -> bool {
    key.strip_prefix(prefix)
        .and_then(|rest| {
            rest.iter()
                .rposition(|&b| b == b'.')
                .map(|at| (&rest[..at], &rest[at + 1..]))
        })
        .is_some_and(|(name, key)| !name.is_empty() && fields.contains(&key))
}
