//! Architectural boundary for Git integration in `gitRUST`.
//!
//! # Stage 00 Status
//! The functional Git engine is **not implemented** in Stage 00; it is scheduled
//! for **Stage 02 (Git Engine)** as documented in `docs/ARCHITECTURE.md` and
//! `docs/adr/0003-git-cli-initial-integration-strategy.md`.
//!
//! This crate currently enforces compile-time and constructor safety invariants
//! required by the security baseline:
//! - Direct process execution with discrete `OsString` arguments (never via shell).
//! - Strict Git subcommand validation rejecting leading-dash option injection (`-c`, `--exec-path`).
//! - Non-UTF-8 path preservation (`Path` / `PathBuf` / `OsStr`).
//! - Explicit classification of read-only vs. write vs. destructive operations.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use gitrust_core::CrateStageStatus;

/// Current implementation readiness of `gitrust-git`.
pub const STAGE_STATUS: CrateStageStatus = CrateStageStatus::Planned { target_stage: 2 };

/// Planned backend implementations behind the future Git engine port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitBackendKind {
    /// System `git` executable invoked directly without a shell (Stage 02 primary).
    GitCli,
    /// Pure-Rust `gitoxide` (`gix`) selective integration candidate for future evaluation.
    GitoxideCandidate,
}

/// Mutability and risk classification for repository operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSafetyClass {
    /// Pure inspection that does not mutate working tree, index, refs, or remotes.
    ReadOnly,
    /// Standard mutating operation requiring per-repository write serialization.
    SerializedWrite,
    /// Potentially data-losing operation requiring explicit user confirmation.
    DestructiveWrite,
}

/// Validation errors when constructing a shell-free Git invocation specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSpecError {
    /// Repository working directory path was empty.
    EmptyRepositoryPath,
    /// Subcommand name was empty.
    EmptySubcommand,
    /// Subcommand failed strict ASCII Git command-name validation (e.g. started with `-`
    /// or contained whitespace, slashes, or shell metacharacters).
    InvalidSubcommand,
    /// Repository path or an argument contained an embedded NUL byte, which OS `exec` APIs reject.
    InteriorNulByte {
        /// Zero-based index (`0` for repository path, `1..=N` for discrete arguments).
        argument_index: usize,
    },
}

impl fmt::Display for CommandSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRepositoryPath => write!(f, "repository path must not be empty"),
            Self::EmptySubcommand => write!(f, "git subcommand must not be empty"),
            Self::InvalidSubcommand => {
                write!(
                    f,
                    "git subcommand must start with an ASCII lowercase letter and contain only [a-z0-9-]"
                )
            }
            Self::InteriorNulByte { argument_index } => {
                write!(
                    f,
                    "path or argument at index {argument_index} contains a NUL byte"
                )
            }
        }
    }
}

impl std::error::Error for CommandSpecError {}

/// Shell-free specification for a future Git CLI command invocation.
///
/// Preserves arbitrary OS byte sequences (`PathBuf` and `OsString`) so repositories
/// with non-UTF-8 filenames remain representable on Unix and Windows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommandSpec {
    repository_path: PathBuf,
    subcommand: String,
    args: Vec<OsString>,
    safety_class: OperationSafetyClass,
}

impl GitCommandSpec {
    /// Validates and constructs a shell-free command specification.
    ///
    /// # Errors
    /// Returns [`CommandSpecError`] if the repository path is empty or contains NUL,
    /// if `subcommand` is empty or not a canonical `[a-z][a-z0-9-]*` Git subcommand,
    /// or if any argument contains an interior NUL byte.
    pub fn new<I, S>(
        repository_path: PathBuf,
        subcommand: &str,
        args: I,
        safety_class: OperationSafetyClass,
    ) -> Result<Self, CommandSpecError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        if repository_path.as_os_str().is_empty() {
            return Err(CommandSpecError::EmptyRepositoryPath);
        }
        if contains_nul(repository_path.as_os_str()) {
            return Err(CommandSpecError::InteriorNulByte { argument_index: 0 });
        }

        if subcommand.is_empty() {
            return Err(CommandSpecError::EmptySubcommand);
        }
        if !is_valid_git_subcommand(subcommand) {
            return Err(CommandSpecError::InvalidSubcommand);
        }

        let mut validated_args = Vec::new();
        for (idx, arg) in args.into_iter().enumerate() {
            let os_arg = arg.into();
            if contains_nul(&os_arg) {
                return Err(CommandSpecError::InteriorNulByte {
                    argument_index: idx + 1,
                });
            }
            validated_args.push(os_arg);
        }

        Ok(Self {
            repository_path,
            subcommand: String::from(subcommand),
            args: validated_args,
            safety_class,
        })
    }

    /// Target repository path (supports non-UTF-8 paths).
    #[must_use]
    pub fn repository_path(&self) -> &Path {
        &self.repository_path
    }

    /// Validated canonical Git subcommand name (for example, `"status"` or `"rev-parse"`).
    #[must_use]
    pub fn subcommand(&self) -> &str {
        &self.subcommand
    }

    /// Discrete process arguments passed directly without shell expansion.
    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Declared concurrency and confirmation requirements for this operation.
    #[must_use]
    pub const fn safety_class(&self) -> OperationSafetyClass {
        self.safety_class
    }
}

fn is_valid_git_subcommand(subcommand: &str) -> bool {
    let mut bytes = subcommand.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn contains_nul(value: &OsStr) -> bool {
    value.as_encoded_bytes().contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_discrete_arguments_without_shell_interpretation() {
        let spec = GitCommandSpec::new(
            PathBuf::from("/tmp/example-repo"),
            "rev-parse",
            ["--verify", "--end-of-options", "HEAD", "--", "; rm -rf /"],
            OperationSafetyClass::ReadOnly,
        );
        assert!(spec.is_ok());
        if let Ok(spec) = spec {
            assert_eq!(spec.subcommand(), "rev-parse");
            assert_eq!(spec.args().len(), 5);
            assert_eq!(spec.safety_class(), OperationSafetyClass::ReadOnly);
        }
    }

    #[test]
    fn rejects_leading_dash_option_injection_and_malformed_subcommands() {
        for malicious in [
            "-c",
            "--upload-pack=evil",
            "--exec-path=/tmp",
            "status;id",
            "../git",
            "Status",
            "status --short",
        ] {
            assert_eq!(
                GitCommandSpec::new(
                    PathBuf::from("/tmp/repo"),
                    malicious,
                    Vec::<OsString>::new(),
                    OperationSafetyClass::ReadOnly,
                ),
                Err(CommandSpecError::InvalidSubcommand),
                "expected subcommand '{malicious}' to be rejected"
            );
        }
    }

    #[test]
    fn rejects_interior_nul_bytes_and_empty_inputs() {
        assert_eq!(
            GitCommandSpec::new(
                PathBuf::new(),
                "status",
                Vec::<OsString>::new(),
                OperationSafetyClass::ReadOnly,
            ),
            Err(CommandSpecError::EmptyRepositoryPath)
        );

        assert_eq!(
            GitCommandSpec::new(
                PathBuf::from("/tmp/bad\0repo"),
                "status",
                Vec::<OsString>::new(),
                OperationSafetyClass::ReadOnly,
            ),
            Err(CommandSpecError::InteriorNulByte { argument_index: 0 })
        );

        assert_eq!(
            GitCommandSpec::new(
                PathBuf::from("/tmp/repo"),
                "",
                Vec::<OsString>::new(),
                OperationSafetyClass::ReadOnly,
            ),
            Err(CommandSpecError::EmptySubcommand)
        );

        assert_eq!(
            GitCommandSpec::new(
                PathBuf::from("/tmp/repo"),
                "commit",
                ["-m", "bad\0message"],
                OperationSafetyClass::SerializedWrite,
            ),
            Err(CommandSpecError::InteriorNulByte { argument_index: 2 })
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_unix_paths_and_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let raw_path = PathBuf::from(OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xFF]));
        let raw_file =
            OsString::from_vec(vec![b'f', b'i', b'l', b'e', b'-', 0xFE, b'.', b'r', b's']);

        let spec = GitCommandSpec::new(
            raw_path.clone(),
            "diff",
            [OsString::from("--"), raw_file.clone()],
            OperationSafetyClass::ReadOnly,
        );
        assert!(spec.is_ok());
        if let Ok(spec) = spec {
            assert_eq!(spec.repository_path(), raw_path.as_path());
            assert_eq!(spec.args()[1], raw_file);
        }
    }
}
