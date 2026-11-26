#[path = "provision.rs"]
mod provision;
#[path = "write.rs"]
mod write;

use std::{
    collections::VecDeque,
    ffi::OsString,
    fmt,
    path::Path,
    sync::{
        Arc, Condvar, Mutex, MutexGuard, Weak,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    BranchList, DiffTarget, GitError, GitExecutable, HeadState, History, HistoryPage, Inspection,
    ObjectId, OperationSafetyClass, ProcessOutput, RepositoryCandidate, RepositoryDiff,
    RepositoryIdentity, RepositoryInfo, RepositoryStatus, Request, TrustedRepository,
    command::{Builtin, GitCommandSpec},
    history::{commits, object, terminated_lines},
    model::{path_from_bytes, strip_line_end},
    process::{self, Control, ProcessLimits},
    repository::validate_config,
};

/// Limits for one background executor.
#[derive(Clone, Copy, Debug)]
pub struct EngineOptions {
    /// Maximum running tasks.
    pub workers: usize,
    /// Maximum waiting tasks, excluding running work.
    pub queue_capacity: usize,
    /// End-to-end active task deadline. Queue time is excluded.
    pub timeout: Duration,
    /// Maximum stdout bytes from an inspection command.
    pub stdout_limit: usize,
    /// Maximum stderr bytes from an inspection command.
    pub stderr_limit: usize,
}

impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            workers: 2,
            queue_capacity: 64,
            timeout: Duration::from_secs(30),
            stdout_limit: 32 * 1024 * 1024,
            stderr_limit: 256 * 1024,
        }
    }
}

/// An ID unique to a task in this application process.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GitTaskId(u64);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Observable lifetime of a background task.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitTaskState {
    /// Waiting for a worker and repository access.
    Queued,
    /// A worker owns the task.
    Running,
    /// A result is available.
    Completed,
    /// An error is available.
    Failed,
    /// Cancelled before or during execution.
    Cancelled,
}

/// The typed result of a supported request.
#[derive(Debug)]
pub enum TaskValue {
    /// Identified but not authorized candidate.
    Identified(RepositoryCandidate),
    /// Created and validated workspace, still awaiting user authorization.
    Provisioned(RepositoryCandidate),
    /// Repository information.
    Info(RepositoryInfo),
    /// A single preflight followed by HEAD and optional worktree inspection.
    Workspace(RepositoryInfo, Option<RepositoryStatus>),
    /// Status snapshot.
    Status(RepositoryStatus),
    /// Parsed two-way diff.
    Diff(RepositoryDiff),
    /// A worker-prepared file patch.
    Change(crate::ChangePatch),
    /// Exact patches approved together for discard.
    DiscardReview(Vec<crate::ChangePatch>),
    /// Local commit prefill.
    CommitDraft(gitrust_core::changes::CommitDraft, Option<ObjectId>),
    /// A local mutation completed. A commit includes its object ID.
    Changed(Option<ObjectId>),
    /// History page.
    History(History),
    /// Branch snapshot.
    Branches(BranchList),
}

/// Terminal outcome, including successful process diagnostics if requested by
/// the caller. Repository content can be sensitive and should not be logged.
pub struct TaskReport {
    /// Submitted task.
    pub id: GitTaskId,
    /// Time since submission.
    pub elapsed: Duration,
    /// Typed value or failure.
    pub result: Result<TaskValue, GitError>,
    /// Git output for dispatched inspection commands. Private policy preflight
    /// and internal object-validation commands are omitted.
    pub processes: Vec<ProcessOutput>,
}

impl fmt::Debug for TaskReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskReport")
            .field("id", &self.id)
            .field("elapsed", &self.elapsed)
            .field("error", &self.result.as_ref().err())
            .field("processes", &self.processes)
            .finish_non_exhaustive()
    }
}

/// A receiver and cancellation handle for a task.
pub struct GitTask {
    id: GitTaskId,
    signal: Arc<Signal>,
    scheduler: Weak<Scheduler>,
    receiver: mpsc::Receiver<TaskReport>,
}

impl fmt::Debug for GitTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitTask")
            .field("id", &self.id)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl GitTask {
    /// Stable identity for correlating the result.
    #[must_use]
    pub const fn id(&self) -> GitTaskId {
        self.id
    }

    /// Current task state.
    #[must_use]
    pub fn state(&self) -> GitTaskState {
        match self.signal.state.load(Ordering::Acquire) {
            0 => GitTaskState::Queued,
            1 => GitTaskState::Running,
            2 => GitTaskState::Completed,
            3 => GitTaskState::Failed,
            _ => GitTaskState::Cancelled,
        }
    }

    /// Remove a queued task immediately or cancel an active read.
    pub fn cancel(&self) {
        self.signal.cancelled.store(true, Ordering::Release);
        if let Some(scheduler) = self.scheduler.upgrade() {
            scheduler.cancel_queued(self.id);
        }
    }

    /// Take the result once it is available, without blocking.
    #[must_use]
    pub fn try_take(&self) -> Option<TaskReport> {
        self.receiver.try_recv().ok()
    }

    /// Wait for delivery. Use from tests or a non-GUI thread only.
    ///
    /// # Errors
    /// Returns an executor error if the worker ends before delivery.
    pub fn wait(self) -> Result<TaskReport, GitError> {
        self.receiver.recv().map_err(|_| GitError::ExecutorClosed)
    }
}

/// Bounded worker pool. Idle workers sleep until a task is eligible.
pub struct GitEngine {
    scheduler: Arc<Scheduler>,
    workers: Vec<JoinHandle<()>>,
}

impl fmt::Debug for GitEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitEngine")
            .field("workers", &self.workers.len())
            .field("queue_capacity", &self.scheduler.capacity)
            .finish_non_exhaustive()
    }
}

impl GitEngine {
    /// Start workers with a previously verified Git binary.
    ///
    /// # Errors
    /// Rejects invalid limits or returns a worker initialization error.
    pub fn new(executable: GitExecutable, options: EngineOptions) -> Result<Self, GitError> {
        if !(1..=32).contains(&options.workers)
            || !(1..=4096).contains(&options.queue_capacity)
            || options.timeout.is_zero()
            || options.timeout > Duration::from_secs(300)
            || !(1..=128 * 1024 * 1024).contains(&options.stdout_limit)
            || !(1..=16 * 1024 * 1024).contains(&options.stderr_limit)
        {
            return Err(GitError::InvalidArgument("executor limits"));
        }
        let scheduler = Arc::new(Scheduler {
            inner: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            shutdown: AtomicBool::new(false),
            capacity: options.queue_capacity,
        });
        let executable = Arc::new(executable);
        let mut engine = Self {
            scheduler,
            workers: Vec::with_capacity(options.workers),
        };
        for index in 0..options.workers {
            let scheduler = Arc::clone(&engine.scheduler);
            let executable = Arc::clone(&executable);
            let worker = thread::Builder::new()
                .name(format!("gitrust-worker-{index}"))
                .spawn(move || worker_loop(&scheduler, &executable, options))
                .map_err(GitError::SpawnFailed)?;
            engine.workers.push(worker);
        }
        Ok(engine)
    }

    /// Enqueue work without waiting for a worker.
    ///
    /// # Errors
    /// Returns a queue-full or executor-closed error.
    pub fn submit(&self, request: Request) -> Result<GitTask, GitError> {
        self.enqueue(request, None)
    }

    /// Enqueue work and publish its ID after delivering the terminal report.
    /// A dispatcher can wait on a shared notification channel and wake the UI.
    ///
    /// # Errors
    /// Returns a queue-full or executor-closed error.
    pub fn submit_notifying(
        &self,
        request: Request,
        completion: mpsc::Sender<GitTaskId>,
    ) -> Result<GitTask, GitError> {
        self.enqueue(request, Some(completion))
    }

    fn enqueue(
        &self,
        request: Request,
        completion: Option<mpsc::Sender<GitTaskId>>,
    ) -> Result<GitTask, GitError> {
        let mut queue = self.scheduler.lock();
        if self.scheduler.shutdown.load(Ordering::Acquire) {
            return Err(GitError::ExecutorClosed);
        }
        if queue.pending.len() >= self.scheduler.capacity {
            return Err(GitError::QueueFull);
        }
        let id = NEXT_ID
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map(GitTaskId)
            .map_err(|_| GitError::ExecutorClosed)?;
        let (sender, receiver) = mpsc::channel();
        let signal = Arc::new(Signal::default());
        let access = Access {
            identity: match &request {
                Request::Identify(_) | Request::Create(_) | Request::Clone(_) => None,
                Request::Inspect(repository, _) | Request::Mutate(repository, _) => {
                    Some(repository.repository().identity().clone())
                }
            },
            class: request.safety_class(),
        };
        queue.pending.push_back(Job {
            id,
            request,
            access,
            signal: Arc::clone(&signal),
            sender,
            completion,
            submitted: Instant::now(),
        });
        self.scheduler.wake.notify_all();
        Ok(GitTask {
            id,
            signal,
            scheduler: Arc::downgrade(&self.scheduler),
            receiver,
        })
    }

    /// Cancel tasks, terminate active read subprocesses and join workers.
    /// Like dropping the executor, call this from a non-GUI thread if active.
    pub fn shutdown(self) {
        drop(self);
    }
}

impl Drop for GitEngine {
    fn drop(&mut self) {
        self.scheduler.close();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[derive(Default)]
struct Signal {
    state: AtomicU8,
    cancelled: AtomicBool,
}

#[derive(Clone)]
struct Access {
    identity: Option<RepositoryIdentity>,
    class: OperationSafetyClass,
}

impl Access {
    fn conflicts(&self, other: &Self) -> bool {
        self.identity.is_some()
            && self.identity == other.identity
            && (self.class != OperationSafetyClass::ReadOnly
                || other.class != OperationSafetyClass::ReadOnly)
    }
}

struct Job {
    id: GitTaskId,
    request: Request,
    access: Access,
    signal: Arc<Signal>,
    sender: mpsc::Sender<TaskReport>,
    completion: Option<mpsc::Sender<GitTaskId>>,
    submitted: Instant,
}

impl Job {
    fn finish(self, result: Result<TaskValue, GitError>, processes: Vec<ProcessOutput>) {
        let state = match &result {
            Ok(_) => GitTaskState::Completed,
            Err(GitError::Cancelled) => GitTaskState::Cancelled,
            Err(_) => GitTaskState::Failed,
        };
        let _ = self.sender.send(TaskReport {
            id: self.id,
            elapsed: self.submitted.elapsed(),
            result,
            processes,
        });
        // Publish the terminal state only after the report is in the channel.
        self.signal.state.store(state as u8, Ordering::Release);
        if let Some(completion) = self.completion {
            let _ = completion.send(self.id);
        }
    }
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<Job>,
    active: Vec<(GitTaskId, Access)>,
}

impl Queue {
    fn first_eligible(&self) -> Option<usize> {
        self.pending.iter().enumerate().position(|(index, job)| {
            !self
                .active
                .iter()
                .any(|(_, active)| job.access.conflicts(active))
                && !self
                    .pending
                    .iter()
                    .take(index)
                    .any(|earlier| job.access.conflicts(&earlier.access))
        })
    }
}

struct Scheduler {
    inner: Mutex<Queue>,
    wake: Condvar,
    shutdown: AtomicBool,
    capacity: usize,
}

impl Scheduler {
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn next(&self) -> Option<Job> {
        let mut queue = self.lock();
        loop {
            if self.shutdown.load(Ordering::Acquire) {
                return None;
            }
            if let Some(index) = queue.first_eligible() {
                let job = queue.pending.remove(index)?;
                job.signal
                    .state
                    .store(GitTaskState::Running as u8, Ordering::Release);
                queue.active.push((job.id, job.access.clone()));
                return Some(job);
            }
            queue = self
                .wake
                .wait(queue)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn release(&self, id: GitTaskId) {
        let mut queue = self.lock();
        queue.active.retain(|(active, _)| *active != id);
        self.wake.notify_all();
    }

    fn cancel_queued(&self, id: GitTaskId) {
        let mut queue = self.lock();
        if let Some(index) = queue.pending.iter().position(|job| job.id == id)
            && let Some(job) = queue.pending.remove(index)
        {
            job.finish(Err(GitError::Cancelled), Vec::new());
            self.wake.notify_all();
        }
    }

    fn close(&self) {
        let mut queue = self.lock();
        self.shutdown.store(true, Ordering::Release);
        for job in queue.pending.drain(..) {
            job.finish(Err(GitError::Cancelled), Vec::new());
        }
        self.wake.notify_all();
    }
}

fn worker_loop(scheduler: &Scheduler, executable: &GitExecutable, options: EngineOptions) {
    while let Some(job) = scheduler.next() {
        let control = Control {
            deadline: Instant::now() + options.timeout,
            cancelled: &job.signal.cancelled,
            shutdown: &scheduler.shutdown,
        };
        let mut runner = Runner {
            executable,
            control,
            limits: ProcessLimits {
                stdout: options.stdout_limit,
                stderr: options.stderr_limit,
            },
            outputs: Vec::new(),
        };
        let result = runner.execute(&job.request).and_then(|value| {
            if !matches!(value, TaskValue::Provisioned(_) | TaskValue::Changed(_)) {
                control.check()?;
            }
            Ok(value)
        });
        scheduler.release(job.id);
        let outputs = runner.outputs;
        job.finish(result, outputs);
    }
}

struct Runner<'a> {
    executable: &'a GitExecutable,
    control: Control<'a>,
    limits: ProcessLimits,
    outputs: Vec<ProcessOutput>,
}

impl Runner<'_> {
    fn execute(&mut self, request: &Request) -> Result<TaskValue, GitError> {
        self.control.check()?;
        match request {
            Request::Identify(path) => {
                RepositoryCandidate::discover(path, self.control).map(TaskValue::Identified)
            }
            Request::Inspect(repository, operation) => self.inspect(repository, operation),
            Request::Mutate(repository, operation) => self.mutate(repository, operation),
            Request::Create(request) => {
                self.provision(&request.path, provision::Operation::Init(&request.branch))
            }
            Request::Clone(request) => {
                self.provision(&request.path, provision::Operation::Clone(request))
            }
        }
    }

    fn inspect(
        &mut self,
        trusted: &TrustedRepository,
        operation: &Inspection,
    ) -> Result<TaskValue, GitError> {
        let (format, width, has_remote) = self.prepare(trusted)?;
        let repo = trusted.repository();
        let location = repo.location();
        let is_bare = repo.worktree().is_none();
        let head = || self.head(trusted, width);
        match operation {
            Inspection::Info | Inspection::Workspace => {
                let info = RepositoryInfo {
                    has_remote,
                    repository: repo.clone(),
                    head: head()?,
                    object_format: format.into(),
                };
                if matches!(operation, Inspection::Info) {
                    return Ok(TaskValue::Info(info));
                }
                let status = if is_bare {
                    None
                } else {
                    Some(self.worktree_status(trusted, width, false)?)
                };
                Ok(TaskValue::Workspace(info, status))
            }
            Inspection::Status { include_ignored } => {
                if is_bare {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "status in a bare repository",
                    ));
                }
                self.worktree_status(trusted, width, *include_ignored)
                    .map(TaskValue::Status)
            }
            Inspection::Diff { target, path } => {
                if is_bare && !matches!(target, DiffTarget::Between { .. }) {
                    return Err(GitError::UnsupportedRepositoryFeature(
                        "index or worktree diff in a bare repository",
                    ));
                }
                self.diff(location, target, path.as_ref(), width)
                    .map(TaskValue::Diff)
            }
            Inspection::Change(key) => self.preview(trusted, key, width).map(TaskValue::Change),
            Inspection::DiscardReview(keys) => self
                .review_discard(trusted, keys, width)
                .map(TaskValue::DiscardReview),
            Inspection::CommitDraft { amend } => self
                .draft(trusted, width, *amend)
                .map(|(draft, head)| TaskValue::CommitDraft(draft, head)),
            Inspection::History(page) => self
                .history(location, head()?, *page, width)
                .map(TaskValue::History),
            Inspection::Branches => {
                let head = head()?;
                let spec = GitCommandSpec::new(Builtin::ForEachRef).args([
                    "--format=%(refname)%00%(objectname)%00%(upstream)%00",
                    "--",
                    "refs/heads/",
                    "refs/remotes/",
                ]);
                let data = self.record(location, spec, &[], false)?;
                BranchList::parse(head, data, width).map(TaskValue::Branches)
            }
        }
    }

    fn worktree_status(
        &mut self,
        trusted: &TrustedRepository,
        width: usize,
        include_ignored: bool,
    ) -> Result<RepositoryStatus, GitError> {
        let spec = status_spec(include_ignored);
        let symbolic = trusted.head_reference()?;
        let data = self.record(trusted.repository().location(), spec, &[], false)?;
        RepositoryStatus::parse_with_head(data, symbolic.as_ref(), Some(width))
    }

    fn prepare(
        &self,
        trusted: &TrustedRepository,
    ) -> Result<(&'static str, usize, bool), GitError> {
        let repo = trusted.repository();
        let location = repo.location();
        let mut has_remote = false;
        for path in trusted.preflight(self.control)? {
            let spec = GitCommandSpec::new(Builtin::Config)
                .arg("--file")
                .arg(git_path(&path))
                .args(["--no-includes", "--null", "--list"]);
            let raw = self.run(
                None,
                spec,
                &[],
                ProcessLimits {
                    stdout: 2 * 1024 * 1024,
                    stderr: 16 * 1024,
                },
            )?;
            if !raw.status.success() {
                return Err(GitError::SecurityPolicyViolation(
                    "invalid local repository configuration",
                ));
            }
            has_remote |= validate_config(&raw.stdout)?;
        }
        let spec = GitCommandSpec::new(Builtin::RevParse)
            .args(["--is-bare-repository", "--show-object-format"]);
        let identity = checked(self.run(Some(location), spec, &[], self.limits)?, false)?;
        let mut fields = terminated_lines(&identity.stdout)?;
        let (is_bare, format, width) = match (fields.next(), fields.next(), fields.next()) {
            (Some(is_bare @ (b"true" | b"false")), Some(format), None) => {
                let width = match format {
                    b"sha1" => 40,
                    b"sha256" => 64,
                    _ => return Err(GitError::UnsupportedRepositoryFeature("object format")),
                };
                (
                    is_bare == b"true",
                    if width == 40 { "sha1" } else { "sha256" },
                    width,
                )
            }
            _ => return Err(GitError::InvalidRepository),
        };
        if is_bare != repo.worktree().is_none() {
            return Err(GitError::SecurityPolicyViolation(
                "unexpected repository mode",
            ));
        }
        Ok((format, width, has_remote))
    }

    fn diff(
        &mut self,
        location: &Path,
        target: &DiffTarget,
        path: Option<&crate::RepoPath>,
        width: usize,
    ) -> Result<RepositoryDiff, GitError> {
        let spec = self.diff_spec(location, target, path, width, false)?;
        let data = self.record(location, spec, &[], true)?;
        RepositoryDiff::parse(data)
    }

    fn diff_spec(
        &self,
        location: &Path,
        target: &DiffTarget,
        path: Option<&crate::RepoPath>,
        width: usize,
        untracked: bool,
    ) -> Result<GitCommandSpec, GitError> {
        let mut spec = GitCommandSpec::new(Builtin::Diff)
            .args([
                "--raw",
                "-z",
                "--patch",
                "--full-index",
                "--no-color",
                "--no-ext-diff",
                "--no-textconv",
                "--no-relative",
                "--ignore-submodules=all",
                "--find-renames",
                "--diff-filter=u",
                "--no-indent-heuristic",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--diff-algorithm=myers",
                "--exit-code",
            ])
            .arg(format!("--abbrev={width}"));
        if untracked {
            let path = path.ok_or(GitError::InvalidArgument("untracked file"))?;
            return Ok(spec
                .args(["--no-index", "--", process::NULL_PATH])
                .arg(git_path(path.as_path())));
        }
        spec = match target {
            DiffTarget::Worktree => spec.arg("--ours"),
            DiffTarget::Index => spec.arg("--cached"),
            DiffTarget::Between { old, new } => {
                self.require_commit(location, old, width)?;
                self.require_commit(location, new, width)?;
                spec.args(["--end-of-options", old.as_str(), new.as_str()])
            }
        };
        spec = spec.arg("--");
        if let Some(path) = path {
            spec = spec.arg(git_path(path.as_path()));
        }
        Ok(spec)
    }

    fn head(&self, repository: &TrustedRepository, width: usize) -> Result<HeadState, GitError> {
        let cwd = repository.repository().location();
        let reference = repository.head_reference()?;
        let out = self.run(
            Some(cwd),
            GitCommandSpec::new(Builtin::RevParse).args([
                "--verify",
                "--end-of-options",
                "HEAD^{commit}",
            ]),
            &[],
            self.limits,
        )?;
        if out.status.success() {
            let id = object(strip_line_end(&out.stdout)?, width)?;
            return Ok(match reference {
                Some(name) => HeadState::Attached(name, id),
                None => HeadState::Detached(id),
            });
        }
        if let Some(name) = reference
            && name.as_bytes().starts_with(b"refs/heads/")
        {
            let resolved = self.run(
                Some(cwd),
                GitCommandSpec::new(Builtin::ShowRef)
                    .args(["--exists", "--"])
                    .arg(path_from_bytes(name.as_bytes())?.into_os_string()),
                &[],
                self.limits,
            )?;
            if resolved.status.code() == Some(2) {
                return Ok(HeadState::Unborn(name));
            }
        }
        Err(GitError::InvalidReference)
    }

    fn require_commit(&self, cwd: &Path, object: &ObjectId, width: usize) -> Result<(), GitError> {
        if object.as_str().len() != width {
            return Err(GitError::InvalidReference);
        }
        let out = self.run(
            Some(cwd),
            GitCommandSpec::new(Builtin::CatFile).args(["-t", "--", object.as_str()]),
            &[],
            self.limits,
        )?;
        if !out.status.success() || strip_line_end(&out.stdout)? != b"commit" {
            return Err(GitError::InvalidReference);
        }
        Ok(())
    }

    fn history(
        &mut self,
        cwd: &Path,
        head: HeadState,
        page: HistoryPage,
        width: usize,
    ) -> Result<History, GitError> {
        let root = match head {
            HeadState::Attached(_, id) | HeadState::Detached(id) => id,
            HeadState::Unborn(_) => {
                return Ok(History {
                    root: None,
                    commits: Vec::new(),
                    next_offset: None,
                });
            }
        };
        let spec = GitCommandSpec::new(Builtin::RevList)
            .arg(format!("--max-count={}", page.limit + 1))
            .arg(format!("--skip={}", page.skip))
            .args(["--end-of-options", root.as_str(), "--"]);
        let data = self.record(cwd, spec, &[], false)?;
        let limit = usize::from(page.limit);
        let mut ids = terminated_lines(data)?
            .take(limit + 2)
            .map(|line| object(line, width))
            .collect::<Result<Vec<_>, _>>()?;
        if ids.len() > limit + 1 {
            return Err(GitError::MalformedOutput("unbounded history page"));
        }
        let next_offset = (ids.len() > limit).then_some(page.skip + u32::from(page.limit));
        ids.truncate(limit);
        let parsed = if ids.is_empty() {
            Vec::new()
        } else {
            let mut input = Vec::with_capacity(ids.len() * (width + 1));
            for id in &ids {
                input.extend(id.as_str().as_bytes());
                input.push(b'\n');
            }
            let data = self.record(
                cwd,
                GitCommandSpec::new(Builtin::CatFile).arg("--batch"),
                &input,
                false,
            )?;
            commits(data, ids)?
        };
        Ok(History {
            root: Some(root),
            commits: parsed,
            next_offset,
        })
    }

    fn record(
        &mut self,
        cwd: &Path,
        spec: GitCommandSpec,
        input: &[u8],
        diff: bool,
    ) -> Result<&[u8], GitError> {
        let output = checked(self.run(Some(cwd), spec, input, self.limits)?, diff)?;
        self.outputs.push(output);
        self.outputs
            .last()
            .map(|output| output.stdout.as_slice())
            .ok_or(GitError::MalformedOutput("missing process result"))
    }

    fn run(
        &self,
        location: Option<&Path>,
        spec: GitCommandSpec,
        input: &[u8],
        limits: ProcessLimits,
    ) -> Result<ProcessOutput, GitError> {
        self.run_policy(location, spec, input, limits, process::Policy::Local, &[])
    }

    fn run_policy(
        &self,
        location: Option<&Path>,
        spec: GitCommandSpec,
        input: &[u8],
        limits: ProcessLimits,
        policy: process::Policy<'_>,
        overrides: &[OsString],
    ) -> Result<ProcessOutput, GitError> {
        let mut args = [
            "--no-pager",
            "--no-optional-locks",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "core.protectHFS=true",
            "-c",
            "core.protectNTFS=true",
            "-c",
            "core.quotePath=true",
            "-c",
            "protocol.allow=never",
        ]
        .map(OsString::from)
        .to_vec();
        let cwd = self
            .executable
            .path()
            .parent()
            .ok_or(GitError::InvalidExecutable)?;
        if let Some(path) = location {
            args.push("-C".into());
            args.push(git_path(path));
        }
        args.extend_from_slice(overrides);
        args.extend(spec.args);
        process::run_with_policy(
            self.executable.path(),
            cwd,
            &args,
            input,
            limits,
            self.control,
            policy,
        )
    }
}

fn status_spec(include_ignored: bool) -> GitCommandSpec {
    GitCommandSpec::new(Builtin::Status)
        .args([
            "--porcelain=v2",
            "-z",
            "--branch",
            "--ahead-behind",
            "--untracked-files=all",
            "--ignore-submodules=all",
            "--find-renames",
        ])
        .arg(if include_ignored {
            "--ignored=matching"
        } else {
            "--no-ignored"
        })
}

fn checked(output: ProcessOutput, diff: bool) -> Result<ProcessOutput, GitError> {
    if output.status.success() || (diff && output.status.code() == Some(1)) {
        Ok(output)
    } else if output
        .stderr
        .windows(b"detected dubious ownership".len())
        .any(|word| word == b"detected dubious ownership")
    {
        Err(GitError::RepositoryNotTrusted)
    } else {
        Err(GitError::CommandFailed(Box::new(output)))
    }
}

#[cfg(not(windows))]
fn git_path(path: &Path) -> OsString {
    path.as_os_str().into()
}

#[cfg(windows)]
fn git_path(path: &Path) -> OsString {
    use std::{
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Component, Prefix},
    };
    if let Some(Component::Prefix(prefix)) = path.components().next()
        && matches!(prefix.kind(), Prefix::VerbatimDisk(_))
    {
        let wide: Vec<u16> = path.as_os_str().encode_wide().skip(4).collect();
        return OsString::from_wide(&wide);
    }
    path.as_os_str().into()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn identity(path: &Path) -> RepositoryIdentity {
        let git = path.join(".git");
        for dir in ["objects", "refs"] {
            std::fs::create_dir_all(git.join(dir)).expect("fixture metadata layout");
        }
        std::fs::write(git.join("HEAD"), b"ref: refs/heads/main\n").expect("fixture HEAD");
        std::fs::write(git.join("config"), b"").expect("fixture config");
        let signal = AtomicBool::new(false);
        RepositoryCandidate::discover(
            path,
            Control {
                deadline: Instant::now() + Duration::from_secs(3),
                cancelled: &signal,
                shutdown: &signal,
            },
        )
        .expect("fixture identity")
        .identity()
        .clone()
    }

    fn scheduled(
        id: u64,
        repo: RepositoryIdentity,
        class: OperationSafetyClass,
    ) -> (Job, mpsc::Receiver<TaskReport>) {
        let (sender, receiver) = mpsc::channel();
        (
            Job {
                id: GitTaskId(id),
                request: Request::Identify(repo.common_dir().to_owned()),
                access: Access {
                    identity: Some(repo),
                    class,
                },
                signal: Arc::default(),
                sender,
                completion: None,
                submitted: Instant::now(),
            },
            receiver,
        )
    }

    #[test]
    fn pending_writer_blocks_later_reads_only_for_its_repository() {
        use OperationSafetyClass::{DestructiveWrite, ReadOnly, SerializedWrite};
        let primary = tempfile::tempdir().expect("primary directory");
        let secondary = tempfile::tempdir().expect("secondary directory");
        let primary = identity(primary.path());
        let secondary = identity(secondary.path());
        let (writer, _) = scheduled(1, primary.clone(), SerializedWrite);
        let (reader, _) = scheduled(2, primary.clone(), ReadOnly);
        let (other_repo, _) = scheduled(3, secondary, DestructiveWrite);
        let active_reader = Access {
            identity: Some(primary),
            class: ReadOnly,
        };
        let mut queue = Queue {
            pending: VecDeque::from([writer, reader, other_repo]),
            active: vec![(GitTaskId(0), active_reader.clone())],
        };
        assert_eq!(queue.first_eligible(), Some(2));
        queue.active.clear();
        assert_eq!(queue.first_eligible(), Some(0));
        let writer = queue.pending.pop_front().expect("writer admitted");
        queue.active.push((writer.id, writer.access));
        assert_eq!(queue.first_eligible(), Some(1));
        queue.active.clear();
        queue.active.push((GitTaskId(0), active_reader));
        assert_eq!(queue.first_eligible(), Some(0));
    }

    #[test]
    fn cancel_queued_removes_immediately_and_close_drains() {
        let directory = tempfile::tempdir().expect("fixture");
        let identity = identity(directory.path());
        let (first, first_result) = scheduled(1, identity.clone(), OperationSafetyClass::ReadOnly);
        let (next, next_result) = scheduled(2, identity, OperationSafetyClass::ReadOnly);
        let state = Arc::clone(&first.signal);
        let scheduler = Scheduler {
            inner: Mutex::new(Queue {
                pending: VecDeque::from([first, next]),
                active: Vec::new(),
            }),
            wake: Condvar::new(),
            shutdown: AtomicBool::new(false),
            capacity: 2,
        };
        scheduler.cancel_queued(GitTaskId(1));
        assert!(matches!(
            first_result
                .try_recv()
                .expect("queued cancellation report")
                .result,
            Err(GitError::Cancelled)
        ));
        assert_eq!(
            state.state.load(Ordering::Acquire),
            GitTaskState::Cancelled as u8
        );
        assert_eq!(scheduler.lock().pending.len(), 1);
        scheduler.close();
        assert!(matches!(
            next_result.try_recv().expect("shutdown report").result,
            Err(GitError::Cancelled)
        ));
        assert!(scheduler.next().is_none());
    }

    #[test]
    fn bounded_queue_rejects_new_work_until_cancellation_frees_a_slot() {
        let engine = GitEngine {
            scheduler: Arc::new(Scheduler {
                inner: Mutex::new(Queue::default()),
                wake: Condvar::new(),
                shutdown: AtomicBool::new(false),
                capacity: 1,
            }),
            workers: Vec::new(),
        };
        let task = engine
            .submit(Request::Identify("first".into()))
            .expect("free slot");
        assert!(matches!(
            engine.submit(Request::Identify("next".into())),
            Err(GitError::QueueFull)
        ));
        task.cancel();
        assert!(matches!(
            task.try_take().expect("cancellation delivered").result,
            Err(GitError::Cancelled)
        ));
        let pending = engine
            .submit(Request::Identify("replacement".into()))
            .expect("slot freed");
        engine.shutdown();
        assert!(matches!(
            pending.wait().expect("shutdown delivered").result,
            Err(GitError::Cancelled)
        ));
    }

    #[test]
    fn shutdown_wakes_an_idle_scheduler() {
        let scheduler = Scheduler {
            inner: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            shutdown: AtomicBool::new(false),
            capacity: 2,
        };
        thread::scope(|scope| {
            let idle = scope.spawn(|| scheduler.next().is_none());
            scheduler.close();
            assert!(idle.join().expect("idle worker released"));
        });
    }
}
