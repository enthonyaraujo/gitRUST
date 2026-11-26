use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    CloneRepository, HeadState, RepositoryCandidate, model::allowed_path, process::Policy,
};

use super::{
    Builtin, GitCommandSpec, GitError, ProcessLimits, Runner, TaskValue, checked, git_path, process,
};

#[derive(Clone, Copy)]
pub(super) enum Operation<'a> {
    Init(&'a str),
    Clone(&'a CloneRepository),
}

impl Runner<'_> {
    pub(super) fn provision(
        &self,
        destination: &Path,
        operation: Operation<'_>,
    ) -> Result<TaskValue, GitError> {
        let destination = resolve_destination(destination)?;
        let parent = destination
            .parent()
            .ok_or(GitError::InvalidArgument("repository parent"))?;
        let mut builder = tempfile::Builder::new();
        builder.prefix(".gitrust-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let private = builder.tempdir_in(parent).map_err(GitError::io)?;
        let working = private.path().join("repository");
        match operation {
            Operation::Init(branch) => {
                let command = GitCommandSpec::new(Builtin::Init)
                    .arg("--quiet")
                    .arg("--template=")
                    .arg(format!("--initial-branch={branch}"))
                    .arg("--")
                    .arg(git_path(&working));
                checked(self.run(None, command, &[], self.limits)?, false)?;
            }
            Operation::Clone(request) => self.clone_metadata(&working, request)?,
        }
        let candidate = RepositoryCandidate::discover(&working, self.control)?;
        // Only metadata produced in our private directory is validated here.
        // The returned candidate still requires session authorization.
        let prepared = candidate.authorize_local_metadata();
        let (_, width, _) = self.prepare(&prepared)?;
        let head = self.head(&prepared, width)?;
        if matches!(operation, Operation::Clone(_)) && !matches!(head, HeadState::Unborn(_)) {
            let command =
                GitCommandSpec::new(Builtin::ReadTree).args(["--reset", "-u", "HEAD", "--"]);
            checked(self.run(Some(&working), command, &[], self.limits)?, false)?;
            self.prepare(&prepared)?;
        }
        self.control.check()?;
        publish(&working, &destination)?;
        Ok(TaskValue::Provisioned(
            prepared.repository().published_worktree(destination),
        ))
    }

    fn clone_metadata(&self, working: &Path, request: &CloneRepository) -> Result<(), GitError> {
        let mut args = [
            "--no-pager",
            "--no-optional-locks",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.https.allow=always",
            "-c",
            "http.sslVerify=true",
            "-c",
            "http.followRedirects=false",
            "-c",
            "credential.helper=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "core.protectHFS=true",
            "-c",
            "core.protectNTFS=true",
        ]
        .map(OsString::from)
        .to_vec();
        let command = GitCommandSpec::new(Builtin::Clone)
            .args([
                "--no-local",
                "--no-checkout",
                "--no-recurse-submodules",
                "--template=",
                "--progress",
                "--config=core.symlinks=false",
                "--",
                request.source.as_str(),
            ])
            .arg(git_path(working));
        args.extend(command.args);
        let cwd = self
            .executable
            .path()
            .parent()
            .ok_or(GitError::InvalidArgument("Git executable directory"))?;
        let limits = ProcessLimits {
            stdout: self.limits.stdout,
            stderr: self.limits.stderr.max(512 * 1024),
        };
        let mut output = process::run_with_policy(
            self.executable.path(),
            cwd,
            &args,
            &[],
            limits,
            self.control,
            Policy::Clone(request.progress.as_ref()),
        )?;
        // Git and the remote can echo the source URL or server messages.
        output.stdout.clear();
        output.stderr.clear();
        checked(output, false)?;
        Ok(())
    }
}

fn resolve_destination(destination: &Path) -> Result<PathBuf, GitError> {
    let parent = destination
        .parent()
        .ok_or(GitError::InvalidArgument("repository parent"))?;
    let name = destination
        .file_name()
        .ok_or(GitError::InvalidArgument("repository name"))?;
    let parent = fs::canonicalize(parent).map_err(GitError::io)?;
    if !allowed_path(&parent) || !fs::metadata(&parent).map_err(GitError::io)?.is_dir() {
        return Err(GitError::InvalidArgument("repository parent"));
    }
    let destination = parent.join(name);
    ensure_free(&destination)?;
    Ok(destination)
}

fn ensure_free(destination: &Path) -> Result<(), GitError> {
    match fs::symlink_metadata(destination) {
        Ok(_) => Err(GitError::DestinationExists),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(GitError::io(error)),
    }
}

fn publish(working: &Path, destination: &Path) -> Result<(), GitError> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let result = rustix::fs::renameat_with(
        rustix::fs::CWD,
        working,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from);
    #[cfg(windows)]
    let result = {
        // tempfile's Windows backend uses MoveFileExW without replacement or copying.
        // The outer TempDir retains responsibility for all scratch cleanup.
        tempfile::TempPath::try_from_path(working).and_then(|mut path| {
            path.disable_cleanup(true);
            path.persist_noclobber(destination)
                .map_err(|error| error.error)
        })
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    let result = Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exclusive publication unavailable",
    ));
    result.map_err(|error| {
        if matches!(ensure_free(destination), Err(GitError::DestinationExists)) {
            GitError::DestinationExists
        } else {
            GitError::io(error)
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn publication_preserves_a_destination_that_appeared_after_validation() {
        let parent = tempfile::tempdir().expect("isolated destination parent");
        let working = parent.path().join("working");
        fs::create_dir(&working).expect("prepared directory");
        fs::write(working.join("own"), b"prepared data").expect("prepared contents");
        let destination = resolve_destination(&parent.path().join("destination"))
            .expect("initially unoccupied target");
        fs::create_dir(&destination).expect("concurrent directory creation");
        assert!(matches!(
            publish(&working, &destination),
            Err(GitError::DestinationExists)
        ));
        assert_eq!(
            fs::read(working.join("own")).expect("preserved source"),
            b"prepared data"
        );
        assert!(destination.is_dir());
        assert_eq!(
            fs::read_dir(destination)
                .expect("preserved directory")
                .count(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn publication_preserves_a_destination_symlink_and_its_target() {
        let parent = tempfile::tempdir().expect("isolated destination parent");
        let working = parent.path().join("working");
        let existing = parent.path().join("existing");
        fs::create_dir(&working).expect("prepared directory");
        fs::create_dir(&existing).expect("existing directory");
        let destination = resolve_destination(&parent.path().join("destination"))
            .expect("initially unoccupied target");
        std::os::unix::fs::symlink(&existing, &destination).expect("concurrent symlink creation");
        assert!(matches!(
            publish(&working, &destination),
            Err(GitError::DestinationExists)
        ));
        assert!(working.is_dir());
        assert_eq!(
            fs::read_link(destination).expect("preserved symlink"),
            existing
        );
        assert!(existing.is_dir());
    }
}
