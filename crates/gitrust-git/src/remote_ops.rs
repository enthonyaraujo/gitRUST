use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
};

use gitrust_core::{
    changes::CommitAuthor,
    remotes::{Auth, Credential, Integration, Remote, RemoteAction, RemoteNotice},
    workspaces::TransferProgress,
};

use crate::{
    AbortProof, GitError, HeadState, NavigationOutcome, ObjectId, ReferenceName, RemoteEndpoint,
    RepositoryCandidate, RepositoryStatus, TaskValue, TrustedRepository,
    model::{records, split_once},
    transport::{Network, network_notice},
};

use super::{
    Builtin, GitCommandSpec, Runner, checked, navigation, process, status_spec, strip_line_end,
    write,
};

/// Observed local remote configuration and tracking state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RemoteSnapshot {
    /// Credential-free endpoints. Unsupported endpoints are absent.
    pub entries: Vec<Remote>,
    /// Configured remote and upstream branch.
    pub upstream: Option<(String, String)>,
    /// Current branch.
    pub current: Option<String>,
    /// Local divergence against the last known upstream.
    pub divergence: Option<(u64, u64)>,
    /// Whether status has staged, unstaged, untracked or conflicted changes.
    pub worktree_dirty: bool,
    /// Whether the current worktree has an interrupted rebase.
    pub rebase: bool,
    /// Preferred integration strategy from allowlisted local configuration.
    pub preference: Integration,
}

/// An action bound to the inspected repository and its expected state.
#[derive(Clone, Debug)]
pub struct RemoteRequest {
    /// Configuration snapshot shown at confirmation.
    pub expected: RemoteSnapshot,
    /// Reviewed porcelain status when a worktree is present.
    pub status: Option<Arc<RepositoryStatus>>,
    /// Reviewed command and authentication.
    pub action: RemoteAction,
    /// Native binary providing the restricted SSH entry point.
    pub bridge: Option<PathBuf>,
    /// Bounded transfer notifications.
    pub progress: Option<mpsc::SyncSender<TransferProgress>>,
    /// Engine-created proof for an interrupted app rebase.
    pub rebase: Option<Arc<RebaseProof>>,
}

/// Result of an app-started operation. It exposes no command output.
#[derive(Debug)]
pub struct RemoteResult {
    /// Sanitized outcome.
    pub notice: RemoteNotice,
    /// Successfully fetched remotes, including partial batches.
    pub fetched: Vec<String>,
    /// Confirmable merge abort proof where applicable.
    pub merge: Option<Arc<AbortProof>>,
    /// Rebase state eligible for explicit continuation or abort.
    pub rebase: Option<Arc<RebaseProof>>,
}

impl RemoteResult {
    fn new(notice: RemoteNotice) -> Self {
        Self {
            notice,
            fetched: Vec::new(),
            merge: None,
            rebase: None,
        }
    }
}

/// Opaque proof for an app-owned, non-interactive rebase.
#[derive(Eq, PartialEq)]
pub struct RebaseProof {
    repository: RepositoryCandidate,
    original: Vec<u8>,
    branch: Vec<u8>,
    onto: Vec<u8>,
    instructions: [Vec<u8>; 2],
    status: RepositoryStatus,
    files: Option<navigation::WorktreeBytes>,
}

impl fmt::Debug for RebaseProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RebaseProof(<private repository state>)")
    }
}

#[derive(Default)]
struct Endpoints {
    fetch: Vec<Option<RemoteEndpoint>>,
    push: Vec<Option<RemoteEndpoint>>,
}

impl Runner<'_> {
    pub(super) fn remotes(
        &self,
        repo: &TrustedRepository,
        width: usize,
    ) -> Result<RemoteSnapshot, GitError> {
        let cwd = repo.repository().location();
        let output = self.checked_local(
            cwd,
            GitCommandSpec::new(Builtin::Config).args([
                "--local",
                "--no-includes",
                "--null",
                "--list",
            ]),
        )?;
        let mut entries: BTreeMap<String, Endpoints> = BTreeMap::new();
        let mut branch_remote = None;
        let mut branch_merge = None;
        let mut preference = Integration::FastForward;
        let mut only_ff = false;
        let current = match self.head(repo, width)? {
            HeadState::Attached(name, _) | HeadState::Unborn(name) => name
                .as_bytes()
                .strip_prefix(b"refs/heads/")
                .and_then(|name| std::str::from_utf8(name).ok())
                .map(str::to_owned),
            HeadState::Detached(_) => None,
        };
        let branch_key = current.as_ref().map(|name| format!("branch.{name}."));
        for record in records(&output.stdout)? {
            let (key, value) = split_once(record, b'\n').unwrap_or((record, &[]));
            let Ok(key) = std::str::from_utf8(key) else {
                continue;
            };
            let value = std::str::from_utf8(value).ok();
            if let Some((name, field)) = key
                .strip_prefix("remote.")
                .and_then(|key| key.rsplit_once('.'))
                && remote_name(name).is_ok()
            {
                let entry = entries.entry(name.to_owned()).or_default();
                let endpoint = value.and_then(|value| RemoteEndpoint::new(value).ok());
                match field {
                    "url" => entry.fetch.push(endpoint),
                    "pushurl" => entry.push.push(endpoint),
                    _ => {}
                }
            }
            if let Some(field) = branch_key
                .as_ref()
                .and_then(|prefix| key.strip_prefix(prefix))
            {
                match field {
                    "remote" => {
                        branch_remote = value
                            .filter(|name| remote_name(name).is_ok())
                            .map(str::to_owned);
                    }
                    "merge" => {
                        branch_merge = value
                            .and_then(|value| value.strip_prefix("refs/heads/"))
                            .filter(|name| branch_name(name).is_ok())
                            .map(str::to_owned);
                    }
                    _ => {}
                }
            }
            match (key, value) {
                ("pull.rebase", Some("true")) => preference = Integration::Rebase,
                ("pull.rebase", Some("false")) => preference = Integration::Merge,
                ("pull.ff", Some(value)) => only_ff = value == "only",
                _ => {}
            }
        }
        let entries = entries
            .into_iter()
            .map(|(name, urls)| {
                let fetch = single_url(&urls.fetch);
                let push = if urls.push.is_empty() {
                    fetch.clone()
                } else {
                    single_url(&urls.push)
                };
                Remote { name, fetch, push }
            })
            .collect();
        let status = if repo.repository().worktree().is_some() {
            Some(self.fresh_status(repo, width, false)?)
        } else {
            None
        };
        Ok(RemoteSnapshot {
            entries,
            current,
            upstream: branch_remote.zip(branch_merge),
            divergence: status.as_ref().and_then(|status| status.branch.divergence),
            worktree_dirty: status.is_some_and(|status| !status.entries.is_empty()),
            rebase: rebase_path(repo)?.is_some(),
            preference: if only_ff {
                Integration::FastForward
            } else {
                preference
            },
        })
    }

    fn fresh_status(
        &self,
        repo: &TrustedRepository,
        width: usize,
        ignored: bool,
    ) -> Result<RepositoryStatus, GitError> {
        let output = self.checked_local(repo.repository().location(), status_spec(ignored))?;
        let reference = repo.head_reference()?;
        RepositoryStatus::parse_with_head(&output.stdout, reference.as_ref(), Some(width))
    }

    pub(super) fn remote_request(
        &mut self,
        repo: &TrustedRepository,
        request: &RemoteRequest,
    ) -> Result<TaskValue, GitError> {
        let (_, width, _) = self.prepare(repo)?;
        let snapshot = self.remotes(repo, width)?;
        if snapshot != request.expected {
            return Err(GitError::StaleSnapshot);
        }
        if let Some(expected) = &request.status
            && self.fresh_status(repo, width, false)? != **expected
        {
            return Err(GitError::StaleSnapshot);
        }
        let result = match &request.action {
            RemoteAction::Fetch(names, auth) => {
                self.fetch_all(repo, request, &snapshot, names, auth)?
            }
            RemoteAction::Pull(name, integration, auth) => {
                self.pull(repo, width, request, name, *integration, auth)?
            }
            RemoteAction::Push {
                remote,
                branch,
                track,
                auth,
            } => self.push(repo, width, request, remote, branch, *track, auth)?,
            RemoteAction::Publish {
                remote,
                url,
                branch,
                auth,
            } => self.publish_existing(repo, width, request, remote, url, branch, auth)?,
            RemoteAction::CheckoutReview {
                endpoint,
                revision,
                branch,
                auth,
            } => self.checkout_review(repo, width, request, endpoint, revision, branch, auth)?,
            RemoteAction::RebaseLocal(branch) => self.rebase_local(repo, width, request, branch)?,
            RemoteAction::ContinueRebase | RemoteAction::AbortRebase => {
                self.recover_rebase(repo, width, request)?
            }
            operation => {
                if snapshot.rebase
                    || navigation::incompatible(repo)?
                    || navigation::merge_source(repo, width)?.is_some()
                {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "operation in progress",
                    ));
                }
                self.edit_remote(repo, &snapshot, operation)?;
                RemoteResult::new(RemoteNotice::Updated)
            }
        };
        Ok(TaskValue::RemoteResult(result))
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_existing(
        &self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
        name: &str,
        url: &str,
        branch: &str,
        auth: &Auth,
    ) -> Result<RemoteResult, GitError> {
        remote_name(name)?;
        branch_name(branch)?;
        if request.expected.current.as_deref() != Some(branch)
            || request
                .status
                .as_ref()
                .and_then(|status| status.branch.oid.as_ref())
                .is_none()
            || navigation::incompatible(repo)?
            || navigation::merge_source(repo, width)?.is_some()
        {
            return Err(GitError::StaleSnapshot);
        }
        RemoteEndpoint::new(url)?;
        if let Some(found) = request
            .expected
            .entries
            .iter()
            .find(|remote| remote.name == name)
        {
            if found.fetch.as_deref() != Some(url) || found.push.as_deref() != Some(url) {
                return Err(GitError::DestinationExists);
            }
        } else {
            self.edit_remote(
                repo,
                &request.expected,
                &RemoteAction::Add(name.into(), url.into(), None),
            )?;
        }
        let mut updated = request.clone();
        updated.expected = self.remotes(repo, width)?;
        if selected(&updated.expected, name)?.push.as_deref() != Some(url) {
            return Err(GitError::StaleSnapshot);
        }
        self.push(repo, width, &updated, name, branch, true, auth)
    }

    #[allow(clippy::too_many_arguments)]
    fn checkout_review(
        &self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
        url: &str,
        revision: &str,
        branch: &str,
        auth: &Auth,
    ) -> Result<RemoteResult, GitError> {
        branch_name(branch)?;
        let expected = request.status.as_ref().ok_or(GitError::StaleSnapshot)?;
        if !expected.entries.is_empty()
            || navigation::incompatible(repo)?
            || navigation::merge_source(repo, width)?.is_some()
        {
            return Err(GitError::MergeDirty);
        }
        let revision = super::object(revision.as_bytes(), width)?;
        let endpoint = RemoteEndpoint::new(url)?;
        let cwd = repo
            .repository()
            .worktree()
            .ok_or(GitError::InvalidReference)?;
        let command = GitCommandSpec::new(Builtin::Fetch).args([
            "--no-write-fetch-head",
            "--no-tags",
            "--no-prune",
            "--no-auto-maintenance",
            "--no-recurse-submodules",
            "--refmap=",
            "--progress",
            "--",
            endpoint.as_str(),
            revision.as_str(),
        ]);
        self.transfer(cwd, command, &endpoint, auth, request, false)?;
        self.prepare(repo)?;
        if self.fresh_status(repo, width, false)? != **expected
            || navigation::incompatible(repo)?
            || navigation::merge_source(repo, width)?.is_some()
        {
            return Err(GitError::StaleSnapshot);
        }
        self.require_commit(cwd, &revision, width)?;
        repo.validate_worktrees(self.control)?;
        repo.validate_reflogs(self.control)?;
        self.checked_local(
            cwd,
            GitCommandSpec::new(Builtin::Switch).args([
                "--no-overwrite-ignore",
                "--no-guess",
                "-c",
                branch,
                "--",
                revision.as_str(),
            ]),
        )
        .map_err(sanitized)?;
        Ok(RemoteResult::new(RemoteNotice::Updated))
    }

    fn edit_remote(
        &self,
        repo: &TrustedRepository,
        snapshot: &RemoteSnapshot,
        operation: &RemoteAction,
    ) -> Result<(), GitError> {
        let cwd = repo.repository().location();
        let command = match operation {
            RemoteAction::Add(name, fetch, push) => {
                remote_name(name)?;
                if snapshot.entries.iter().any(|remote| remote.name == *name) {
                    return Err(GitError::DestinationExists);
                }
                let fetch = RemoteEndpoint::new(fetch)?;
                let push = push.as_deref().map(RemoteEndpoint::new).transpose()?;
                self.checked_local(
                    cwd,
                    GitCommandSpec::new(Builtin::Remote).args(["add", "--", name, fetch.as_str()]),
                )
                .map_err(sanitized)?;
                if let Some(push) = push {
                    self.config(cwd, &format!("remote.{name}.pushurl"), Some(push.as_str()))?;
                }
                return Ok(());
            }
            RemoteAction::Edit(name, fetch, push) => {
                selected(snapshot, name)?;
                let fetch = RemoteEndpoint::new(fetch)?;
                let push = push.as_deref().map(RemoteEndpoint::new).transpose()?;
                self.config(cwd, &format!("remote.{name}.url"), Some(fetch.as_str()))?;
                self.config(
                    cwd,
                    &format!("remote.{name}.pushurl"),
                    push.as_ref().map(RemoteEndpoint::as_str),
                )?;
                return Ok(());
            }
            RemoteAction::Rename(old, name) => {
                selected(snapshot, old)?;
                remote_name(name)?;
                if snapshot.entries.iter().any(|remote| remote.name == *name) {
                    return Err(GitError::DestinationExists);
                }
                self.validate_tracking(cwd, old)?;
                self.validate_tracking_refs(repo, old)?;
                self.validate_tracking_refs(repo, name)?;
                repo.validate_reflogs(self.control)?;
                GitCommandSpec::new(Builtin::Remote).args(["rename", "--", old, name])
            }
            RemoteAction::Remove(name) => {
                selected(snapshot, name)?;
                self.validate_tracking(cwd, name)?;
                self.validate_tracking_refs(repo, name)?;
                repo.validate_reflogs(self.control)?;
                GitCommandSpec::new(Builtin::Remote).args(["remove", "--", name])
            }
            RemoteAction::Upstream(remote, branch) => {
                selected(snapshot, remote)?;
                return self.upstream(cwd, snapshot, remote, branch);
            }
            _ => return Err(GitError::InvalidArgument("remote operation")),
        };
        self.checked_local(cwd, command).map_err(sanitized)?;
        Ok(())
    }

    fn validate_tracking(&self, cwd: &Path, name: &str) -> Result<(), GitError> {
        let key = format!("remote.{name}.fetch");
        let output = self.run(
            Some(cwd),
            GitCommandSpec::new(Builtin::Config).args([
                "--local",
                "--null",
                "--get-all",
                "--",
                &key,
            ]),
            &[],
            self.limits,
        )?;
        if output.status.code() == Some(1) {
            return Ok(());
        }
        let output = checked(output, false).map_err(sanitized)?;
        let allowed = format!("refs/heads/*:refs/remotes/{name}/*");
        if records(&output.stdout)?
            .any(|value| value.strip_prefix(b"+").unwrap_or(value) != allowed.as_bytes())
        {
            return Err(GitError::UnsupportedRepositoryFeature(
                "custom remote fetch mapping",
            ));
        }
        Ok(())
    }

    fn config(&self, cwd: &Path, key: &str, value: Option<&str>) -> Result<(), GitError> {
        let command = GitCommandSpec::new(Builtin::Config).arg("--local");
        if let Some(value) = value {
            self.checked_local(cwd, command.args(["--replace-all", "--", key, value]))
                .map_err(sanitized)?;
        } else {
            let output = self.run(
                Some(cwd),
                command.args(["--unset-all", "--", key]),
                &[],
                self.limits,
            )?;
            if output.status.code() != Some(5) {
                checked(output, false).map_err(sanitized)?;
            }
        }
        Ok(())
    }

    fn upstream(
        &self,
        cwd: &Path,
        snapshot: &RemoteSnapshot,
        remote: &str,
        branch: &str,
    ) -> Result<(), GitError> {
        branch_name(branch)?;
        let current = snapshot
            .current
            .as_ref()
            .ok_or(GitError::InvalidReference)?;
        self.config(cwd, &format!("branch.{current}.remote"), Some(remote))?;
        self.config(
            cwd,
            &format!("branch.{current}.merge"),
            Some(&format!("refs/heads/{branch}")),
        )
    }

    fn transfer(
        &self,
        cwd: &Path,
        command: GitCommandSpec,
        endpoint: &RemoteEndpoint,
        auth: &Auth,
        request: &RemoteRequest,
        writing: bool,
    ) -> Result<Vec<u8>, GitError> {
        let network = Network::prepare(endpoint, auth, request.bridge.as_deref())?;
        let output = self.run_policy(
            Some(cwd),
            command,
            &[],
            self.limits,
            process::Policy::Network(&network, request.progress.as_ref()),
            &network.options,
        );
        match output {
            Ok(output) if output.status.success() => Ok(output.stdout),
            Ok(output) => {
                let bytes: Vec<_> = output.stderr.into_iter().chain(output.stdout).collect();
                Err(GitError::Remote(network_notice(&bytes, writing)))
            }
            Err(GitError::Cancelled | GitError::Timeout | GitError::OutputLimitExceeded(_))
                if writing =>
            {
                Err(GitError::Remote(RemoteNotice::Uncertain))
            }
            Err(error) => Err(error),
        }
    }

    fn fetch(
        &self,
        repo: &TrustedRepository,
        remote: &Remote,
        auth: &Auth,
        request: &RemoteRequest,
    ) -> Result<(), GitError> {
        let endpoint = endpoint(remote.fetch.as_deref())?;
        self.validate_tracking_refs(repo, &remote.name)?;
        let refspec = format!("+refs/heads/*:refs/remotes/{}/*", remote.name);
        let command = GitCommandSpec::new(Builtin::Fetch).args([
            "--atomic",
            "--no-write-fetch-head",
            "--no-tags",
            "--prune",
            "--no-prune-tags",
            "--no-auto-maintenance",
            "--no-recurse-submodules",
            "--refmap=",
            "--progress",
            "--",
            endpoint.as_str(),
            &refspec,
        ]);
        self.transfer(
            repo.repository().location(),
            command,
            &endpoint,
            auth,
            request,
            false,
        )?;
        Ok(())
    }

    fn validate_tracking_refs(
        &self,
        repo: &TrustedRepository,
        remote: &str,
    ) -> Result<(), GitError> {
        let root = repo
            .repository()
            .identity()
            .common_dir()
            .join("refs/remotes")
            .join(remote);
        let prefix = format!("refs/remotes/{remote}/");
        let mut pending = vec![root];
        while let Some(path) = pending.pop() {
            self.control.check()?;
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(GitError::io(error)),
            };
            if meta.is_dir() {
                for entry in fs::read_dir(&path).map_err(GitError::io)? {
                    self.control.check()?;
                    pending.push(entry.map_err(GitError::io)?.path());
                }
            } else if meta.is_file() {
                if let Some(target) = bounded(&path)?.strip_prefix(b"ref: ")
                    && !target.starts_with(prefix.as_bytes())
                {
                    return Err(GitError::SecurityPolicyViolation(
                        "tracking reference outside its namespace",
                    ));
                }
            } else {
                return Err(GitError::SecurityPolicyViolation(
                    "unsafe tracking reference",
                ));
            }
        }
        Ok(())
    }

    fn fetch_all(
        &self,
        repo: &TrustedRepository,
        request: &RemoteRequest,
        snapshot: &RemoteSnapshot,
        names: &[String],
        auth: &Auth,
    ) -> Result<RemoteResult, GitError> {
        if names.is_empty()
            || names.len() > 64
            || (names.len() > 1
                && matches!(
                    auth.credential,
                    Credential::Password { .. } | Credential::ScopedToken { .. }
                ))
        {
            return Err(GitError::InvalidArgument("remote selection"));
        }
        let urls: Vec<_> = names
            .iter()
            .map(|name| selected(snapshot, name))
            .collect::<Result<_, _>>()?;
        let mut outcome = RemoteResult::new(RemoteNotice::Fetched);
        for remote in urls {
            let ssh = endpoint(remote.fetch.as_deref()).is_ok_and(|endpoint| endpoint.is_ssh());
            let auth = if ssh {
                Auth {
                    known_hosts: auth.known_hosts.clone(),
                    identity: auth.identity.clone(),
                    ..Auth::default()
                }
            } else {
                Auth {
                    credential: auth.credential.clone(),
                    ca: auth.ca.clone(),
                    ..Auth::default()
                }
            };
            match self.fetch(repo, remote, &auth, request) {
                Ok(()) => outcome.fetched.push(remote.name.clone()),
                Err(error) => {
                    outcome.notice = failure(&error);
                    break;
                }
            }
        }
        Ok(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
        remote: &str,
        target: &str,
        track: bool,
        auth: &Auth,
    ) -> Result<RemoteResult, GitError> {
        branch_name(target)?;
        let cwd = repo.repository().location();
        let snapshot = &request.expected;
        let remote = selected(snapshot, remote)?;
        let endpoint = endpoint(remote.push.as_deref())?;
        if navigation::incompatible(repo)? || navigation::merge_source(repo, width)?.is_some() {
            return Err(GitError::UnsupportedRepositoryFeature(
                "operation in progress",
            ));
        }
        let HeadState::Attached(reference, head) = self.head(repo, width)? else {
            return Err(GitError::InvalidReference);
        };
        if request
            .status
            .as_ref()
            .and_then(|status| status.branch.oid.as_ref())
            != Some(&head)
            || snapshot.current.as_ref().is_none_or(|name| {
                reference.as_bytes().strip_prefix(b"refs/heads/") != Some(name.as_bytes())
            })
        {
            return Err(GitError::StaleSnapshot);
        }
        let refspec = format!("{}:refs/heads/{target}", head.as_str());
        let command = GitCommandSpec::new(Builtin::Push).args([
            "--porcelain",
            "--no-verify",
            "--no-follow-tags",
            "--recurse-submodules=no",
            "--progress",
            "--",
            endpoint.as_str(),
            &refspec,
        ]);
        let lines = self.transfer(cwd, command, &endpoint, auth, request, true)?;
        if track
            && (self.prepare(repo).is_err()
                || self.head(repo, width).ok() != Some(HeadState::Attached(reference, head))
                || self.upstream(cwd, snapshot, &remote.name, target).is_err())
        {
            return Ok(RemoteResult::new(RemoteNotice::PushedWithoutTracking));
        }
        Ok(RemoteResult::new(
            if lines
                .split(|&byte| byte == b'\n')
                .any(|line| line.starts_with(b"=\t"))
            {
                RemoteNotice::UpToDate
            } else {
                RemoteNotice::Pushed
            },
        ))
    }

    fn pull(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
        name: &str,
        integration: Integration,
        auth: &Auth,
    ) -> Result<RemoteResult, GitError> {
        repo.repository()
            .worktree()
            .ok_or(GitError::InvalidReference)?;
        let prior = request.status.as_ref().ok_or(GitError::StaleSnapshot)?;
        let snapshot = &request.expected;
        if snapshot.rebase
            || navigation::incompatible(repo)?
            || navigation::merge_source(repo, width)?.is_some()
        {
            return Err(GitError::UnsupportedRepositoryFeature(
                "operation in progress",
            ));
        }
        let (_, branch) = snapshot
            .upstream
            .as_ref()
            .filter(|(remote, _)| remote == name)
            .ok_or(GitError::Remote(RemoteNotice::NoUpstream))?;
        if !prior.entries.is_empty()
            || (integration == Integration::Rebase
                && !self.fresh_status(repo, width, true)?.entries.is_empty())
        {
            return Err(GitError::MergeDirty);
        }
        let HeadState::Attached(reference, head) = self.head(repo, width)? else {
            return Err(GitError::InvalidReference);
        };
        let remote = selected(snapshot, name)?;
        self.fetch(repo, remote, auth, request)?;
        let mut result = self
            .integrate_fetched(
                repo,
                width,
                request,
                (name, branch),
                integration,
                (reference, head),
            )
            .unwrap_or_else(|error| RemoteResult::new(failure(&error)));
        result.fetched.push(name.to_owned());
        Ok(result)
    }

    fn integrate_fetched(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
        upstream: (&str, &str),
        integration: Integration,
        (reference, head): (ReferenceName, ObjectId),
    ) -> Result<RemoteResult, GitError> {
        let (name, branch) = upstream;
        let cwd = repo.repository().location();
        self.prepare(repo)?;
        let prior = request.status.as_ref().ok_or(GitError::StaleSnapshot)?;
        let current = self.remotes(repo, width)?;
        let after = self.fresh_status(repo, width, false)?;
        if prior.entries != after.entries
            || self.head(repo, width)? != HeadState::Attached(reference, head.clone())
            || current.upstream != request.expected.upstream
            || current.entries != request.expected.entries
            || navigation::incompatible(repo)?
            || navigation::merge_source(repo, width)?.is_some()
        {
            return Err(GitError::StaleSnapshot);
        }
        let branch = format!("refs/remotes/{name}/{branch}");
        let output = self.checked_local(
            cwd,
            GitCommandSpec::new(Builtin::RevParse).args([
                "--verify",
                "--end-of-options",
                &format!("{branch}^{{commit}}"),
            ]),
        )?;
        let tip = super::object(strip_line_end(&output.stdout)?, width)?;
        let (draft, _) = self.draft(repo, width, false)?;
        let merge_first = integration != Integration::Rebase
            || self.ancestor(cwd, &head, &tip)?
            || self.ancestor(cwd, &tip, &head)?;
        if merge_first {
            let merge = self.merge_object(
                repo,
                width,
                &head,
                &tip,
                &after,
                &draft.author,
                if integration == Integration::FastForward {
                    navigation::MergeMode::FastForward
                } else {
                    navigation::MergeMode::Standard
                },
            )?;
            let mut result = RemoteResult::new(match merge.outcome {
                NavigationOutcome::MergePending => RemoteNotice::MergePending,
                NavigationOutcome::Integrated => RemoteNotice::UpToDate,
                _ => RemoteNotice::Integrated,
            });
            result.merge = merge.abort;
            Ok(result)
        } else {
            self.rebase_start(repo, width, &head, &tip, &draft.author)
        }
    }

    fn rebase_local(
        &mut self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
        branch: &gitrust_core::navigation::Reference,
    ) -> Result<RemoteResult, GitError> {
        let status = request.status.as_ref().ok_or(GitError::StaleSnapshot)?;
        let head = status.branch.oid.as_ref().ok_or(GitError::StaleSnapshot)?;
        if !status.entries.is_empty()
            || navigation::incompatible(repo)?
            || navigation::merge_source(repo, width)?.is_some()
        {
            return Err(GitError::MergeDirty);
        }
        repo.validate_worktrees(self.control)?;
        repo.validate_reflogs(self.control)?;
        let refs = self.branches(repo, width)?;
        if !matches!(&refs.head, HeadState::Attached(_, id) if id == head) {
            return Err(GitError::StaleSnapshot);
        }
        let target = crate::RefTip {
            name: ReferenceName::full(&branch.name)?,
            id: super::object(branch.commit.as_bytes(), width)?,
        };
        let branch = navigation::local_tip(&refs, &target)?;
        if branch.current {
            return Err(GitError::InvalidReference);
        }
        let (draft, _) = self.draft(repo, width, false)?;
        self.rebase_start(repo, width, head, &branch.id, &draft.author)
    }

    fn rebase_start(
        &self,
        repo: &TrustedRepository,
        width: usize,
        head: &ObjectId,
        tip: &ObjectId,
        author: &CommitAuthor,
    ) -> Result<RemoteResult, GitError> {
        let cwd = repo.repository().location();
        let range = format!("{}..{}", tip.as_str(), head.as_str());
        let merges = self.checked_local(
            cwd,
            GitCommandSpec::new(Builtin::RevList).args([
                "--max-count=1",
                "--min-parents=2",
                "--end-of-options",
                &range,
                "--",
            ]),
        )?;
        if !merges.stdout.is_empty() || !self.fresh_status(repo, width, true)?.entries.is_empty() {
            return Err(GitError::UnsupportedRepositoryFeature(
                "rebase needs a clean worktree and linear commits",
            ));
        }
        let rewritten = self.checked_local(
            cwd,
            GitCommandSpec::new(Builtin::RevList).args(["--end-of-options", &range, "--"]),
        )?;
        for id in super::terminated_lines(&rewritten.stdout)? {
            let (_, signed) = self.read_commit(cwd, super::object(id, width)?)?;
            if signed {
                return Err(GitError::Remote(RemoteNotice::SignedRebase));
            }
        }
        let command = GitCommandSpec::new(Builtin::Rebase).args([
            "--merge",
            "--no-autostash",
            "--no-update-refs",
            "--no-rebase-merges",
            "--no-fork-point",
            "--reapply-cherry-picks",
            "--empty=keep",
            "--",
            tip.as_str(),
        ]);
        self.rebase_command(repo, width, command, author, None)
    }

    fn recover_rebase(
        &self,
        repo: &TrustedRepository,
        width: usize,
        request: &RemoteRequest,
    ) -> Result<RemoteResult, GitError> {
        let expected = request.rebase.as_ref().ok_or(GitError::StaleSnapshot)?;
        let proof = self.rebase_proof(repo, width)?;
        if proof.repository != expected.repository
            || proof.original != expected.original
            || proof.branch != expected.branch
            || proof.onto != expected.onto
            || proof.instructions != expected.instructions
        {
            return Err(GitError::StaleSnapshot);
        }
        let abort = matches!(request.action, RemoteAction::AbortRebase);
        if abort && (proof.files.is_none() || proof != **expected) {
            return Err(GitError::StaleSnapshot);
        }
        if !abort
            && proof
                .status
                .entries
                .iter()
                .any(|entry| matches!(entry.kind, crate::EntryKind::Unmerged { .. }))
        {
            return Err(GitError::MergeDirty);
        }
        let command =
            GitCommandSpec::new(Builtin::Rebase).arg(if abort { "--abort" } else { "--continue" });
        let (draft, _) = self.draft(repo, width, false)?;
        self.rebase_command(
            repo,
            width,
            command,
            &draft.author,
            Some(if abort {
                RemoteNotice::RebaseAborted
            } else {
                RemoteNotice::Integrated
            }),
        )
    }

    fn rebase_command(
        &self,
        repo: &TrustedRepository,
        width: usize,
        command: GitCommandSpec,
        author: &CommitAuthor,
        success: Option<RemoteNotice>,
    ) -> Result<RemoteResult, GitError> {
        let identity = write::identity(author, None)?;
        let output = self.run_policy(
            Some(repo.repository().location()),
            command,
            &[],
            self.limits,
            process::Policy::Author(&identity, false),
            &[
                "-c".into(),
                "core.editor=/usr/bin/true".into(),
                "-c".into(),
                "maintenance.auto=false".into(),
                "-c".into(),
                "rerere.enabled=false".into(),
                "-c".into(),
                "commit.gpgSign=false".into(),
            ],
        )?;
        if !output.status.success() && rebase_path(repo)?.is_some() {
            let mut result = RemoteResult::new(RemoteNotice::RebasePending);
            result.rebase = Some(Arc::new(self.rebase_proof(repo, width)?));
            return Ok(result);
        }
        checked(output, false)?;
        Ok(RemoteResult::new(
            success.unwrap_or(RemoteNotice::Integrated),
        ))
    }

    fn rebase_proof(
        &self,
        repo: &TrustedRepository,
        width: usize,
    ) -> Result<RebaseProof, GitError> {
        let path = rebase_path(repo)?.ok_or(GitError::StaleSnapshot)?;
        repo.validate_rebase_metadata(self.control)?;
        for file in [
            "autostash",
            "update-refs",
            "strategy",
            "strategy_opts",
            "gpg_sign_opt",
            "signoff",
            "refs-to-delete",
        ] {
            match fs::symlink_metadata(path.join(file)) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(GitError::io(error)),
                Ok(_) => return Err(GitError::StaleSnapshot),
            }
        }
        let instructions = [
            bounded(&path.join("git-rebase-todo"))?,
            bounded(&path.join("done"))?,
        ];
        for bytes in &instructions {
            for line in bytes.split(|&byte| byte == b'\n') {
                if line.is_empty() || line.starts_with(b"#") {
                    continue;
                }
                let Some(id) = line
                    .strip_prefix(b"pick ")
                    .and_then(|line| line.split(|&byte| byte == b' ').next())
                else {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "rebase contains non-pick instructions",
                    ));
                };
                if id.len() < 4 || id.len() > width || !id.iter().all(u8::is_ascii_hexdigit) {
                    return Err(GitError::StaleSnapshot);
                }
            }
        }
        let original = bounded(&path.join("orig-head"))?;
        let branch = bounded(&path.join("head-name"))?;
        let onto = bounded(&path.join("onto"))?;
        super::object(strip_line_end(&original)?, width)?;
        super::object(strip_line_end(&onto)?, width)?;
        let reference = strip_line_end(&branch)?;
        if !reference.starts_with(b"refs/heads/") {
            return Err(GitError::InvalidReference);
        }
        ReferenceName::new(reference)?;
        let status = self.fresh_status(repo, width, false)?;
        let files = self.worktree_bytes(repo, &status).ok();
        Ok(RebaseProof {
            repository: repo.repository().clone(),
            original,
            branch,
            onto,
            instructions,
            status,
            files,
        })
    }
}

fn bounded(path: &Path) -> Result<Vec<u8>, GitError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(GitError::io)?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(GitError::io)?;
    if bytes.len() > 1_048_576 {
        return Err(GitError::ParsedLimitExceeded("rebase metadata"));
    }
    Ok(bytes)
}

fn rebase_path(repo: &TrustedRepository) -> Result<Option<PathBuf>, GitError> {
    let path = repo.repository().git_dir().join("rebase-merge");
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_dir() => Ok(Some(path)),
        Ok(_) => Err(GitError::SecurityPolicyViolation(
            "redirected rebase metadata",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(GitError::io(error)),
    }
}

fn single_url(values: &[Option<RemoteEndpoint>]) -> Option<String> {
    if let [Some(endpoint)] = values {
        Some(endpoint.as_str().to_owned())
    } else {
        None
    }
}

fn endpoint(value: Option<&str>) -> Result<RemoteEndpoint, GitError> {
    RemoteEndpoint::new(value.ok_or(GitError::InvalidArgument("remote endpoint"))?)
}

fn selected<'a>(snapshot: &'a RemoteSnapshot, name: &str) -> Result<&'a Remote, GitError> {
    remote_name(name)?;
    snapshot
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or(GitError::StaleSnapshot)
}

fn remote_name(name: &str) -> Result<(), GitError> {
    if name.is_empty()
        || name.len() > 64
        || !name.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(GitError::InvalidReference);
    }
    ReferenceName::new(format!("refs/remotes/{name}/branch"))?;
    Ok(())
}

fn branch_name(name: &str) -> Result<(), GitError> {
    if name.len() > 255 || matches!(name, "HEAD" | "@") || name.starts_with('-') {
        return Err(GitError::InvalidReference);
    }
    ReferenceName::new(format!("refs/heads/{name}"))?;
    Ok(())
}

fn failure(error: &GitError) -> RemoteNotice {
    match error {
        GitError::Remote(notice) => *notice,
        GitError::Cancelled => RemoteNotice::Cancelled,
        GitError::Timeout => RemoteNotice::Timeout,
        GitError::StaleSnapshot => RemoteNotice::Stale,
        GitError::InvalidArgument(_) | GitError::InvalidReference => RemoteNotice::Invalid,
        GitError::MergeDirty => RemoteNotice::Dirty,
        _ => RemoteNotice::Failed,
    }
}

fn sanitized(error: GitError) -> GitError {
    if matches!(error, GitError::CommandFailed(_)) {
        GitError::Remote(RemoteNotice::Failed)
    } else {
        error
    }
}
