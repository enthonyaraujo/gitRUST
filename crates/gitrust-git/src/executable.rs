use std::{
    ffi::OsString,
    fmt,
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use crate::{
    GitError,
    command::has_nul,
    model::{allowed_path, decimal, strip_line_end},
    process::{self, Control, ProcessLimits},
};

/// Version returned by the selected system executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitVersion {
    /// Major release.
    pub major: u32,
    /// Minor release.
    pub minor: u32,
    /// Patch release.
    pub patch: u32,
    /// Optional platform or package suffix.
    pub suffix: String,
}

impl GitVersion {
    /// Minimum version with the Stage 02 command-line protections.
    pub const MINIMUM: (u32, u32, u32) = (2, 45, 0);

    /// Parse a single complete `git --version` output line.
    ///
    /// # Errors
    /// Rejects malformed values and prerelease builds.
    pub fn parse(output: &[u8]) -> Result<Self, GitError> {
        let line = strip_line_end(output)?;
        let value = line
            .strip_prefix(b"git version ")
            .ok_or(GitError::InvalidExecutable)?;
        let (major, rest) = part(value)?;
        let (minor, rest) = part(rest.strip_prefix(b".").ok_or(GitError::InvalidExecutable)?)?;
        let (patch, suffix) = part(rest.strip_prefix(b".").ok_or(GitError::InvalidExecutable)?)?;
        let suffix = std::str::from_utf8(suffix).map_err(|_| GitError::InvalidExecutable)?;
        if suffix.contains(|c: char| c.is_control())
            || (!suffix.is_empty() && !suffix.starts_with(['.', ' ', '+', '-']))
            || suffix.starts_with("-rc")
            || suffix.starts_with(".rc")
        {
            return Err(GitError::InvalidExecutable);
        }
        Ok(Self {
            major,
            minor,
            patch,
            suffix: suffix.into(),
        })
    }
}

fn part(bytes: &[u8]) -> Result<(u32, &[u8]), GitError> {
    let end = bytes
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(bytes.len());
    Ok((
        decimal(&bytes[..end], "Git version").map_err(|_| GitError::InvalidExecutable)?,
        &bytes[end..],
    ))
}

impl fmt::Display for GitVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}{}",
            self.major, self.minor, self.patch, self.suffix
        )
    }
}

/// A verified absolute path to a native Git binary.
///
/// The application's PATH and administrator-selected binaries are part of the
/// local trust boundary. Version output cannot prove a binary's provenance.
#[derive(Clone, Debug)]
pub struct GitExecutable {
    path: PathBuf,
    version: GitVersion,
}

impl GitExecutable {
    /// Inspect only absolute directories from the process PATH.
    ///
    /// # Errors
    /// Returns an invalid executable, version or not-found error as appropriate.
    pub fn discover() -> Result<Self, GitError> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        Self::discover_in(std::env::split_paths(&path))
    }

    fn discover_in(directories: impl IntoIterator<Item = PathBuf>) -> Result<Self, GitError> {
        let filename = if cfg!(windows) { "git.exe" } else { "git" };
        for dir in directories
            .into_iter()
            .filter(|dir| dir.is_absolute() && allowed_path(dir))
        {
            let candidate = dir.join(filename);
            match fs::symlink_metadata(&candidate) {
                Ok(_) => return Self::from_absolute(&candidate),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(GitError::io(e)),
            }
        }
        Err(GitError::GitNotFound)
    }

    /// Validate an administrator-selected absolute executable without using a shell.
    ///
    /// # Errors
    /// Rejects non-native files, scripts, missing binaries and old versions.
    pub fn from_absolute(path: &Path) -> Result<Self, GitError> {
        if !path.is_absolute()
            || !allowed_path(path)
            || has_nul(path.as_os_str())
            || (cfg!(windows) && !exe_extension(path))
        {
            return Err(GitError::InvalidExecutable);
        }
        let path = canonical_executable(path)?;
        if !allowed_path(&path) || (cfg!(windows) && !exe_extension(&path)) || !native_file(&path)?
        {
            return Err(GitError::InvalidExecutable);
        }
        let parent = path.parent().ok_or(GitError::InvalidExecutable)?;
        let stop = AtomicBool::new(false);
        let output = process::run(
            &path,
            parent,
            &[OsString::from("--version")],
            &[],
            ProcessLimits {
                stdout: 4096,
                stderr: 4096,
            },
            Control {
                deadline: Instant::now() + Duration::from_secs(3),
                cancelled: &stop,
                shutdown: &stop,
            },
        )?;
        if !output.status.success() {
            return Err(GitError::InvalidExecutable);
        }
        let version = GitVersion::parse(&output.stdout)?;
        if (version.major, version.minor, version.patch) < GitVersion::MINIMUM {
            return Err(GitError::UnsupportedGitVersion(version));
        }
        Ok(Self { path, version })
    }

    /// Location of the binary used by the engine.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Version measured at discovery.
    #[must_use]
    pub const fn version(&self) -> &GitVersion {
        &self.version
    }
}

fn exe_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn native_file(path: &Path) -> Result<bool, GitError> {
    let meta = fs::metadata(path).map_err(GitError::io)?;
    if !meta.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Ok(false);
        }
    }
    let mut magic = [0; 4];
    let mut file = File::open(path).map_err(GitError::io)?;
    if let Err(e) = file.read_exact(&mut magic) {
        return if e.kind() == io::ErrorKind::UnexpectedEof {
            Ok(false)
        } else {
            Err(GitError::io(e))
        };
    }
    #[cfg(windows)]
    return Ok(magic.starts_with(b"MZ"));
    #[cfg(target_os = "macos")]
    return Ok(matches!(
        u32::from_be_bytes(magic),
        0xfeed_face
            | 0xcefa_edfe
            | 0xfeed_facf
            | 0xcffa_edfe
            | 0xcafe_babe
            | 0xbeba_feca
            | 0xcafe_babf
            | 0xbfba_feca
    ));
    #[cfg(not(any(windows, target_os = "macos")))]
    return Ok(magic == *b"\x7fELF");
}

fn canonical_executable(path: &Path) -> Result<PathBuf, GitError> {
    let path = fs::canonicalize(path).map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            GitError::GitNotFound
        } else {
            GitError::io(e)
        }
    })?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn parses_git_versions_and_rejects_malformed_native_versions() {
        let windows =
            GitVersion::parse(b"git version 2.56.1.windows.1\n").expect("Git for Windows version");
        assert_eq!((windows.major, windows.minor, windows.patch), (2, 56, 1));
        assert_eq!(windows.suffix, ".windows.1");
        let apple =
            GitVersion::parse(b"git version 2.50.1 (Apple Git-162)\n").expect("Apple version");
        assert_eq!(apple.suffix, " (Apple Git-162)");
        assert!(GitVersion::parse(b"git version 2.45.0-rc1\n").is_err());
        assert!(GitVersion::parse(b"2.45.0\n").is_err());
        assert!(GitVersion::parse(b"git version 2.45\n").is_err());
    }

    #[test]
    fn ignores_relative_path_entries_and_rejects_scripts_or_invalid_files() {
        let temp = tempfile::tempdir().expect("fixture directory");
        let missing = temp.path().join("not-present");
        assert!(matches!(
            GitExecutable::discover_in([PathBuf::from("."), missing]),
            Err(GitError::GitNotFound)
        ));
        let fake = temp
            .path()
            .join(if cfg!(windows) { "git.exe" } else { "git" });
        fs::write(&fake, b"not a native binary").expect("fixture substitute");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&fake, fs::Permissions::from_mode(0o755))
                .expect("fixture executable mode");
        }
        assert!(matches!(
            GitExecutable::discover_in([temp.path().into()]),
            Err(GitError::InvalidExecutable)
        ));
        assert!(matches!(
            GitExecutable::from_absolute(Path::new("git")),
            Err(GitError::InvalidExecutable)
        ));
    }
}
