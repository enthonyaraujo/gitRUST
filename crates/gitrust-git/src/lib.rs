//! Architectural boundary for Git integration in `gitRUST`.
//!
//! # Stage 00 Status
//! The functional Git engine is **not implemented** in Stage 00; it is scheduled
//! for **Stage 02 (Git Engine)** as documented in `docs/ARCHITECTURE.md` and
//! `docs/adr/0003-git-cli-initial-integration-strategy.md`.
//!
//! This crate currently defines compile-time safety invariants required by the
//! security baseline:
//! - Direct process execution with discrete `OsString` arguments (never via shell).
//! - Non-UTF-8 path preservation (`PathBuf` / `OsStr`).
//! - Explicit classification of read-only vs. write vs. destructive operations.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::PathBuf;

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
    /// An argument contained an embedded NUL byte, which OS `exec` APIs reject.
    InteriorNulByte {
        /// Zero-based index of the offending argument.
        argument_index: usize,
    },
}

impl fmt::Display for CommandSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyRepositoryPath => write!(f, "repository path must not be empty"),
            Self::EmptySubcommand => write!(f, "git subcommand must not be empty"),
            Self::InteriorNulByte { argument_index } => {
                write!(f, "argument at index {argument_index} contains a NUL byte")
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
    subcommand: OsString,
    args: Vec<OsString>,
    safety_class: OperationSafetyClass,
}

impl GitCommandSpec {
    /// Validates and constructs a shell-free command specification.
    ///
    /// # Errors
    /// Returns [`CommandSpecError`] if the repository path or subcommand is empty,
    /// or if any argument contains an interior NUL byte.
    pub fn new<I, S>(
        repository_path: PathBuf,
        subcommand: impl AsRef<OsStr>,
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

        let subcommand_os = subcommand.as_ref().to_os_string();
        if subcommand_os.is_empty() {
            return Err(CommandSpecError::EmptySubcommand);
        }
        if contains_nul(&subcommand_os) {
            return Err(CommandSpecError::InteriorNulByte { argument_index: 0 });
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
            subcommand: subcommand_os,
            args: validated_args,
            safety_class,
        })
    }

    /// Target repository path (supports non-UTF-8 paths).
    #[must_use]
    pub fn repository_path(&self) -> &PathBuf {
        &self.repository_path
    }

    /// Git subcommand name (for example, `"status"`).
    #[must_use]
    pub fn subcommand(&self) -> &OsStr {
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
            "status",
            [
                "--porcelain=v2",
                "--untracked-files=all",
                "--",
                "; rm -rf /",
            ],
            OperationSafetyClass::ReadOnly,
        );
        assert!(spec.is_ok());
        if let Ok(spec) = spec {
            assert_eq!(spec.subcommand(), OsStr::new("status"));
            assert_eq!(spec.args().len(), 4);
            assert_eq!(spec.safety_class(), OperationSafetyClass::ReadOnly);
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
            assert_eq!(spec.repository_path(), &raw_path);
            assert_eq!(spec.args()[1], raw_file);
        }
    }
}
