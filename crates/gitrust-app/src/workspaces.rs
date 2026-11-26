#[path = "workspace_changes.rs"]
mod changes;
#[path = "workspace_navigation.rs"]
mod navigation;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, after, bounded, never, select, unbounded};
use gitrust_core::workspaces::{
    Activity, ChangeCounts, Checkout, Preferences, ProvisionKind, RepositoryAction, RepositoryView,
    TransferProgress, Workspace, WorkspaceIssue,
};
use gitrust_git::{
    CloneRepository, CloneSource, CreateRepository, EngineOptions, EntryKind, FileChange,
    GitEngine, GitError, GitExecutable, GitTask, GitTaskId, HeadState, Inspection,
    RepositoryCandidate, RepositoryInfo, RepositoryStatus, Request, TaskValue, TrustedRepository,
};
use gitrust_storage::Store;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Default)]
pub(crate) struct ViewUpdate {
    pub(crate) view: Option<RepositoryView>,
    pub(crate) preferences: Option<Preferences>,
    pub(crate) directory: Option<PathBuf>,
}

pub(crate) struct Workspaces {
    commands: Option<Sender<RepositoryAction>>,
    updates: Arc<Mutex<ViewUpdate>>,
    worker: Option<JoinHandle<()>>,
}

impl Workspaces {
    pub(crate) fn start(ctx: &egui::Context) -> Self {
        Self::start_with(ctx, || Store::discover().ok())
    }

    fn start_with(
        ctx: &egui::Context,
        profile: impl FnOnce() -> Option<Store> + Send + 'static,
    ) -> Self {
        let (commands, incoming) = bounded(32);
        let updates = Arc::new(Mutex::new(ViewUpdate::default()));
        let mailbox = updates.clone();
        let mut worker = None;
        let wake = ctx.clone();
        if let Ok(thread) = thread::Builder::new()
            .name("gitrust-workspaces".into())
            .spawn(move || Coordinator::run(&incoming, profile(), mailbox, wake))
        {
            worker = Some(thread);
        } else if let Ok(mut update) = updates.lock() {
            update.view = Some(RepositoryView {
                activity: Activity::Idle,
                issue: Some(WorkspaceIssue::OperationFailed),
                ..RepositoryView::default()
            });
        }
        Self {
            commands: Some(commands),
            updates,
            worker,
        }
    }

    pub(crate) fn send(&self, action: RepositoryAction) {
        if let Some(commands) = &self.commands {
            let _ = commands.try_send(action);
        }
    }

    pub(crate) fn take_update(&self) -> Option<ViewUpdate> {
        self.updates
            .try_lock()
            .ok()
            .map(|mut view| std::mem::take(&mut *view))
    }
}

impl Drop for Workspaces {
    fn drop(&mut self) {
        drop(self.commands.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

enum WorkerEvent {
    Complete(GitTaskId),
    Folder(Option<PathBuf>, bool),
    Progress(u64, TransferProgress),
}

enum Operation {
    Identify(PathBuf),
    Inspect {
        trust: TrustedRepository,
        refresh: bool,
    },
    Provision(ProvisionKind),
    ChangesRead,
    ChangesWrite(TrustedRepository),
    NavigationRead(navigation::Read),
    NavigationWrite(TrustedRepository),
}

struct Pending {
    task: GitTask,
    operation: Operation,
}

#[derive(Default)]
struct Debounce(Option<(Instant, Instant)>);

impl Debounce {
    fn record(&mut self, now: Instant) {
        let first = self.0.map_or(now, |(first, _)| first);
        let deadline = (now + Duration::from_millis(300)).min(first + Duration::from_secs(2));
        self.0 = Some((first, deadline));
    }

    fn timer(&self, ready: bool) -> Receiver<Instant> {
        if let Some((_, deadline)) = self.0.filter(|_| ready) {
            after(deadline.saturating_duration_since(Instant::now()))
        } else {
            never()
        }
    }
}

struct Coordinator {
    engine: Option<GitEngine>,
    store: Option<Store>,
    view: RepositoryView,
    candidate: Option<RepositoryCandidate>,
    changes: changes::ChangesState,
    navigation: navigation::State,
    trusted: HashMap<PathBuf, TrustedRepository>,
    pending: Option<Pending>,
    events: Sender<WorkerEvent>,
    complete: mpsc::Sender<GitTaskId>,
    watched: Option<RecommendedWatcher>,
    watch_epoch: u64,
    watch_events: Sender<(u64, bool)>,
    debounce: Debounce,
    operation_epoch: u64,
    dialog_open: bool,
    mailbox: Arc<Mutex<ViewUpdate>>,
    wake: egui::Context,
}

impl Coordinator {
    fn run(
        commands: &Receiver<RepositoryAction>,
        store: Option<Store>,
        mailbox: Arc<Mutex<ViewUpdate>>,
        wake: egui::Context,
    ) {
        let (events, replies) = unbounded();
        let (watch_events, changes) = bounded(64);
        let (complete, completions) = mpsc::channel();
        let bridge = events.clone();
        let _ = thread::Builder::new()
            .name("gitrust-completions".into())
            .spawn(move || {
                while let Ok(id) = completions.recv() {
                    if bridge.send(WorkerEvent::Complete(id)).is_err() {
                        break;
                    }
                }
            });
        let mut service = Self {
            engine: None,
            store,
            view: RepositoryView::default(),
            candidate: None,
            changes: changes::ChangesState::default(),
            navigation: navigation::State::default(),
            trusted: HashMap::new(),
            pending: None,
            events,
            complete,
            watched: None,
            watch_epoch: 0,
            watch_events,
            debounce: Debounce::default(),
            operation_epoch: 0,
            dialog_open: false,
            mailbox,
            wake,
        };
        let preferences = service.load_profile();
        service.discover_git();
        service.publish(Some(preferences), None);
        loop {
            let ready = service.pending.is_none() && service.candidate.is_none();
            let timer = service.debounce.timer(ready);
            let previous = service.view.clone();
            let directory = select! {
                recv(commands) -> action => {
                    let Ok(action) = action else { break; };
                    service.on_action(action);
                    None
                }
                recv(replies) -> event => event.ok().and_then(|event| service.on_reply(event)),
                recv(changes) -> event => { if let Ok(event) = event { service.on_watch(event); } None }
                recv(timer) -> _ => { service.debounce.0 = None; service.refresh(); None }
            };
            if previous != service.view || directory.is_some() {
                service.publish(None, directory);
            }
        }
        service.watched.take();
        service.invalidate();
        if let Some(engine) = service.engine.take() {
            engine.shutdown();
        }
    }

    fn publish(&self, preferences: Option<Preferences>, directory: Option<PathBuf>) {
        if let Ok(mut update) = self.mailbox.lock() {
            update.view = Some(self.view.clone());
            if preferences.is_some() {
                update.preferences = preferences;
            }
            if directory.is_some() {
                update.directory = directory;
            }
        }
        self.wake.request_repaint();
    }

    fn load_profile(&mut self) -> Preferences {
        if let Some(store) = &self.store {
            match store.load_recent() {
                Ok(recent) => self.view.recent = recent,
                Err(_) => self.view.storage_issue = true,
            }
            if let Ok(prefs) = store.load_preferences() {
                return prefs;
            }
        }
        self.view.storage_issue = true;
        Preferences::default()
    }

    fn discover_git(&mut self) {
        self.engine = GitExecutable::discover()
            .and_then(|exe| {
                GitEngine::new(
                    exe,
                    EngineOptions {
                        timeout: Duration::from_secs(300),
                        ..EngineOptions::default()
                    },
                )
            })
            .ok();
        self.view.git_available = self.engine.is_some();
        self.view.issue = self
            .engine
            .is_none()
            .then_some(WorkspaceIssue::GitUnavailable);
        self.view.activity = Activity::Idle;
    }

    fn invalidate(&mut self) {
        self.operation_epoch = self.operation_epoch.wrapping_add(1);
        if let Some(pending) = self.pending.take() {
            pending.task.cancel();
        }
        if let Some(view) = &mut self.view.changes {
            view.busy = false;
        }
        if let Some(view) = &mut self.view.navigation {
            view.busy = false;
        }
        self.candidate = None;
        self.view.candidate = None;
        self.view.progress = None;
        self.view.destination = None;
        self.view.provisioned = None;
    }

    fn start(&mut self, request: Request, operation: Operation, phase: Activity) {
        self.invalidate();
        let result = self
            .engine
            .as_ref()
            .ok_or(GitError::GitNotFound)
            .and_then(|engine| engine.submit_notifying(request, self.complete.clone()));
        match result {
            Ok(task) => {
                self.pending = Some(Pending { task, operation });
                self.view.issue = None;
                self.view.activity = phase;
            }
            Err(error) => self.issue(issue(&error)),
        }
    }

    fn issue(&mut self, issue: WorkspaceIssue) {
        self.view.activity = Activity::Idle;
        self.view.progress = None;
        self.view.destination = None;
        self.view.provisioned = None;
        self.view.issue = Some(issue);
    }

    fn open(&mut self, path: PathBuf) {
        self.start(
            Request::Identify(path.clone()),
            Operation::Identify(path),
            Activity::Identifying,
        );
    }

    fn inspect(&mut self, trust: TrustedRepository, refresh: bool) {
        self.start(
            Request::Inspect(trust.clone(), Inspection::Workspace),
            Operation::Inspect { trust, refresh },
            Activity::Inspecting,
        );
    }

    fn refresh(&mut self) {
        if self.pending.is_some() || self.candidate.is_some() {
            return;
        }
        if let Some(active) = &self.view.active
            && let Some(trust) = self.trusted.get(&active.path)
        {
            self.inspect(trust.clone(), true);
        }
    }

    fn on_action(&mut self, action: RepositoryAction) {
        if matches!(
            self.pending.as_ref().map(|job| &job.operation),
            Some(Operation::ChangesWrite(_) | Operation::NavigationWrite(_))
        ) && !matches!(
            action,
            RepositoryAction::Cancel | RepositoryAction::SavePreferences(_)
        ) {
            return;
        }
        match action {
            RepositoryAction::Changes(intent) => self.on_changes(intent),
            RepositoryAction::Navigate(intent) => self.on_navigation(intent),
            RepositoryAction::Browse => self.folder_dialog(false),
            RepositoryAction::BrowseDestination => self.folder_dialog(true),
            RepositoryAction::RetryGit if self.engine.is_none() => self.discover_git(),
            RepositoryAction::RetryGit => {}
            RepositoryAction::Open(path) => self.open(path),
            RepositoryAction::Authorize { path, accepted } => self.authorize(&path, accepted),
            RepositoryAction::Forget(path) => self.forget(&path),
            RepositoryAction::Reveal(path) => {
                if reveal(&path).is_err() {
                    self.view.issue = Some(WorkspaceIssue::FolderActionFailed);
                }
            }
            RepositoryAction::Refresh => self.refresh(),
            RepositoryAction::Focus => {
                if self.view.active.is_some() {
                    self.debounce.record(Instant::now());
                }
            }
            RepositoryAction::Create {
                destination,
                branch,
            } => {
                let request = std::path::absolute(&destination)
                    .map_err(GitError::Io)
                    .and_then(|path| CreateRepository::new(path, branch));
                match request {
                    Ok(request) => {
                        self.start(
                            Request::Create(request),
                            Operation::Provision(ProvisionKind::Create),
                            Activity::Creating,
                        );
                        if self.pending.is_some() {
                            self.view.destination = Some(destination);
                        }
                    }
                    Err(error) => self.issue(issue(&error)),
                }
            }
            RepositoryAction::Clone { destination, url } => self.clone(&destination, &url),
            RepositoryAction::Cancel => {
                if let Some(job) = &self.pending {
                    job.task.cancel();
                    self.view.activity = Activity::Cancelling;
                }
            }
            RepositoryAction::SavePreferences(prefs) => {
                if self
                    .store
                    .as_ref()
                    .is_none_or(|store| store.save_preferences(prefs).is_err())
                {
                    self.view.storage_issue = true;
                }
            }
        }
    }

    fn folder_dialog(&mut self, for_destination: bool) {
        if self.dialog_open {
            return;
        }
        let events = self.events.clone();
        match thread::Builder::new()
            .name("gitrust-folder-dialog".into())
            .spawn(move || {
                let folder = rfd::FileDialog::new().pick_folder();
                let _ = events.send(WorkerEvent::Folder(folder, for_destination));
            }) {
            Ok(_) => self.dialog_open = true,
            Err(_) => self.view.issue = Some(WorkspaceIssue::FolderActionFailed),
        }
    }

    fn clone(&mut self, destination: &Path, url: &str) {
        let request = CloneSource::new(url).and_then(|source| {
            let path = std::path::absolute(destination).map_err(GitError::Io)?;
            CloneRepository::new(path, source)
        });
        match request {
            Ok(request) => {
                let (tx, rx) = mpsc::sync_channel(2);
                self.start(
                    Request::Clone(request.with_progress(tx)),
                    Operation::Provision(ProvisionKind::Clone),
                    Activity::Cloning,
                );
                if self.pending.is_some() {
                    self.view.destination = Some(destination.to_owned());
                }
                let events = self.events.clone();
                let epoch = self.operation_epoch;
                let _ = thread::Builder::new()
                    .name("gitrust-clone-progress".into())
                    .spawn(move || {
                        while let Ok(progress) = rx.recv() {
                            if events.send(WorkerEvent::Progress(epoch, progress)).is_err() {
                                break;
                            }
                            // Coalesce progress reports from fast transfers.
                            thread::sleep(Duration::from_millis(80));
                        }
                    });
            }
            Err(error) => self.issue(issue(&error)),
        }
    }

    fn on_reply(&mut self, event: WorkerEvent) -> Option<PathBuf> {
        match event {
            WorkerEvent::Complete(id) => self.complete(id),
            WorkerEvent::Folder(path, for_destination) => {
                self.dialog_open = false;
                if for_destination {
                    return path;
                }
                if let Some(path) = path {
                    self.open(path);
                }
            }
            WorkerEvent::Progress(epoch, progress) => {
                if epoch == self.operation_epoch && self.view.activity == Activity::Cloning {
                    self.view.progress = Some(progress);
                }
            }
        }
        None
    }

    fn complete(&mut self, id: GitTaskId) {
        if !self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.task.id() == id)
        {
            return;
        }
        let Some(pending) = self.pending.take() else {
            return;
        };
        let Some(report) = pending.task.try_take() else {
            self.issue(WorkspaceIssue::OperationFailed);
            return;
        };
        self.view.progress = None;
        self.view.destination = None;
        if matches!(
            pending.operation,
            Operation::ChangesRead | Operation::ChangesWrite(_)
        ) {
            self.complete_changes(pending.operation, report.result);
            return;
        }
        if matches!(
            pending.operation,
            Operation::NavigationRead(_) | Operation::NavigationWrite(_)
        ) {
            self.complete_navigation(pending.operation, report.result);
            return;
        }
        match report.result {
            Ok(TaskValue::Identified(candidate)) => self.offer(candidate, false),
            Ok(TaskValue::Provisioned(candidate)) => {
                if let Operation::Provision(kind) = pending.operation {
                    self.view.provisioned = Some(kind);
                    self.offer(candidate, true);
                }
            }
            Ok(TaskValue::Workspace(info, status)) => {
                if let Operation::Inspect { trust, refresh } = pending.operation {
                    self.opened(info, status, trust, refresh);
                }
            }
            Err(error) => self.failed(pending.operation, &error),
            Ok(_) => self.issue(WorkspaceIssue::OperationFailed),
        }
    }

    fn offer(&mut self, candidate: RepositoryCandidate, new: bool) {
        let path = repo_path(&candidate);
        if !new
            && let Some(trust) = self.trusted.get(path)
            && trust.repository() == &candidate
        {
            self.inspect(trust.clone(), false);
            return;
        }
        self.view.candidate = Some(path.to_owned());
        self.candidate = Some(candidate);
        self.view.activity = Activity::Trust;
    }

    fn authorize(&mut self, path: &Path, accepted: bool) {
        if self.view.candidate.as_deref() != Some(path) {
            return;
        }
        self.view.candidate = None;
        if let Some(candidate) = self.candidate.take() {
            if accepted {
                self.inspect(candidate.authorize_local_metadata(), false);
            } else {
                self.issue(WorkspaceIssue::Security);
            }
        }
    }

    fn opened(
        &mut self,
        info: RepositoryInfo,
        status: Option<RepositoryStatus>,
        trust: TrustedRepository,
        refresh: bool,
    ) {
        let path = repo_path(&info.repository).to_owned();
        let changed_workspace = self
            .view
            .active
            .as_ref()
            .is_none_or(|active| active.path != path)
            || self
                .trusted
                .get(&path)
                .is_none_or(|current| current != &trust);
        let workspace = Workspace {
            path: path.clone(),
            checkout: checkout(info.head),
            has_remote: info.has_remote,
            changes: status.as_ref().map(counts),
        };
        self.view.unavailable.retain(|entry| entry != &path);
        self.view.active = Some(workspace);
        self.set_changes(status, changed_workspace);
        self.trusted.insert(path.clone(), trust.clone());
        self.view.activity = Activity::Idle;
        self.view.issue = None;
        if !refresh {
            self.view.recent.open(path);
            self.save_recent();
        }
        if changed_workspace {
            self.watch(&info.repository);
        }
        self.load_navigation(trust, changed_workspace);
    }

    fn failed(&mut self, operation: Operation, error: &GitError) {
        tracing::debug!(%error, "workspace inspection ended with an error");
        let problem = issue(error);
        let path = match operation {
            Operation::Identify(path) => Some(path),
            Operation::Inspect { trust, .. } => Some(repo_path(trust.repository()).to_owned()),
            Operation::Provision(_)
            | Operation::ChangesRead
            | Operation::ChangesWrite(_)
            | Operation::NavigationRead(_)
            | Operation::NavigationWrite(_) => None,
        };
        if matches!(
            problem,
            WorkspaceIssue::Unavailable
                | WorkspaceIssue::NotRepository
                | WorkspaceIssue::PermissionDenied
        ) && let Some(path) = path
        {
            if self.view.recent.paths.contains(&path) && !self.view.unavailable.contains(&path) {
                self.view.unavailable.push(path.clone());
            }
            self.trusted.remove(&path);
            if self
                .view
                .active
                .as_ref()
                .is_some_and(|workspace| workspace.path == path)
            {
                self.view.active = None;
                self.set_changes(None, true);
                self.clear_navigation();
                self.unwatch();
            }
        }
        self.issue(problem);
    }

    fn forget(&mut self, path: &Path) {
        self.view.recent.forget(path);
        self.view.unavailable.retain(|entry| entry != path);
        self.trusted.remove(path);
        let active = self
            .view
            .active
            .as_ref()
            .is_some_and(|workspace| workspace.path == path);
        let pending_for_path =
            self.pending
                .as_ref()
                .is_some_and(|pending| match &pending.operation {
                    Operation::Identify(target) => target == path,
                    Operation::Inspect { trust, .. } => repo_path(trust.repository()) == path,
                    Operation::Provision(_)
                    | Operation::ChangesRead
                    | Operation::ChangesWrite(_)
                    | Operation::NavigationRead(_)
                    | Operation::NavigationWrite(_) => false,
                });
        if pending_for_path || self.view.candidate.as_deref() == Some(path) {
            self.invalidate();
            self.view.activity = Activity::Idle;
        }
        if active {
            self.view.active = None;
            self.set_changes(None, true);
            self.clear_navigation();
            self.unwatch();
        }
        self.save_recent();
    }

    fn save_recent(&mut self) {
        if self
            .store
            .as_ref()
            .is_none_or(|store| store.save_recent(&self.view.recent).is_err())
        {
            self.view.storage_issue = true;
        }
    }

    fn unwatch(&mut self) {
        self.watched = None;
        self.watch_epoch = self.watch_epoch.wrapping_add(1);
        self.view.watching = false;
        self.debounce.0 = None;
    }

    fn watch(&mut self, candidate: &RepositoryCandidate) {
        self.unwatch();
        let epoch = self.watch_epoch;
        let events = self.watch_events.clone();
        let watcher =
            notify::recommended_watcher(
                move |result: notify::Result<notify::Event>| match result {
                    Ok(event) if !matches!(event.kind, notify::EventKind::Access(_)) => {
                        let _ = events.try_send((epoch, false));
                    }
                    Err(_) => {
                        let _ = events.try_send((epoch, true));
                    }
                    Ok(_) => {}
                },
            );
        if let Ok(mut watcher) = watcher {
            let root = repo_path(candidate);
            let common = candidate.identity().common_dir();
            if watcher.watch(root, RecursiveMode::Recursive).is_ok()
                && (common.starts_with(root)
                    || watcher.watch(common, RecursiveMode::Recursive).is_ok())
            {
                self.watched = Some(watcher);
                self.view.watching = true;
            }
        }
    }

    fn on_watch(&mut self, (epoch, failed): (u64, bool)) {
        if epoch != self.watch_epoch {
            return;
        }
        if failed {
            self.unwatch();
        }
        if self.view.active.is_some() {
            self.debounce.record(Instant::now());
        }
    }
}

fn repo_path(candidate: &RepositoryCandidate) -> &Path {
    candidate.worktree().unwrap_or_else(|| candidate.git_dir())
}

fn issue(error: &GitError) -> WorkspaceIssue {
    match error {
        GitError::GitNotFound
        | GitError::UnsupportedGitVersion(_)
        | GitError::InvalidExecutable => WorkspaceIssue::GitUnavailable,
        GitError::InvalidRepository => WorkspaceIssue::NotRepository,
        GitError::Io(error) if error.kind() == std::io::ErrorKind::NotFound => {
            WorkspaceIssue::Unavailable
        }
        GitError::PermissionDenied => WorkspaceIssue::PermissionDenied,
        GitError::SecurityPolicyViolation(_)
        | GitError::RepositoryNotTrusted
        | GitError::UnsupportedRepositoryFeature(_) => WorkspaceIssue::Security,
        GitError::InvalidArgument(_) | GitError::InvalidReference => WorkspaceIssue::InvalidInput,
        GitError::DestinationExists => WorkspaceIssue::DestinationExists,
        GitError::Cancelled => WorkspaceIssue::Cancelled,
        _ => WorkspaceIssue::OperationFailed,
    }
}

fn checkout(head: HeadState) -> Checkout {
    fn name(reference: &gitrust_git::ReferenceName) -> String {
        let bytes = reference.as_bytes();
        String::from_utf8_lossy(bytes.strip_prefix(b"refs/heads/").unwrap_or(bytes)).into_owned()
    }
    match head {
        HeadState::Attached(reference, _) => Checkout::Branch(name(&reference)),
        HeadState::Unborn(reference) => Checkout::Unborn(name(&reference)),
        HeadState::Detached(id) => Checkout::Detached(id.abbreviated().to_owned()),
    }
}

fn counts(status: &RepositoryStatus) -> ChangeCounts {
    let mut counts = ChangeCounts::default();
    for entry in &status.entries {
        match entry.kind {
            EntryKind::Untracked => counts.untracked += 1,
            EntryKind::Unmerged { .. } => counts.conflicted += 1,
            _ => {
                counts.staged += usize::from(
                    entry
                        .index
                        .is_some_and(|change| change != FileChange::Unchanged),
                );
                counts.unstaged += usize::from(
                    entry
                        .worktree
                        .is_some_and(|change| change != FileChange::Unchanged),
                );
            }
        }
    }
    counts
}

fn reveal(path: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    let path = std::fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(std::io::ErrorKind::NotADirectory.into());
    }
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("/usr/bin/gio");
        command.args(["open", "--"]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/open");
        command.arg("--");
        command
    };
    #[cfg(windows)]
    let mut command = Command::new(
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|root| root.is_absolute())
            .ok_or(std::io::ErrorKind::NotFound)?
            .join("explorer.exe"),
    );
    let status = command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("native folder opener failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_coalesces_and_has_a_maximum_delay() {
        let mut debounce = Debounce::default();
        let start = Instant::now();
        debounce.record(start);
        assert_eq!(
            debounce.0,
            Some((start, start + Duration::from_millis(300)))
        );
        debounce.record(start + Duration::from_millis(250));
        assert_eq!(
            debounce.0,
            Some((start, start + Duration::from_millis(550)))
        );
        debounce.record(start + Duration::from_millis(1900));
        assert_eq!(debounce.0, Some((start, start + Duration::from_secs(2))));
    }
}

#[cfg(test)]
mod integration_tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn until(service: &Workspaces, check: impl Fn(&RepositoryView) -> bool) -> RepositoryView {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(view) = service.take_update().and_then(|update| update.view)
                && check(&view)
            {
                return view;
            }
            assert!(Instant::now() < end, "expected coordinator transition");
            thread::sleep(Duration::from_millis(15));
        }
    }

    fn open(service: &Workspaces, input: &Path, expected: &Path, accept: bool) {
        service.send(RepositoryAction::Open(input.to_owned()));
        until(service, |view| {
            view.activity == Activity::Trust && view.candidate.as_deref() == Some(expected)
        });
        service.send(RepositoryAction::Authorize {
            path: expected.to_owned(),
            accepted: accept,
        });
    }

    fn prepare(service: &Workspaces, path: &Path) -> RepositoryView {
        service.send(RepositoryAction::Create {
            destination: path.to_owned(),
            branch: "main".into(),
        });
        let view = until(service, |view| view.activity == Activity::Trust);
        let candidate = view.candidate.expect("a prepared candidate");
        service.send(RepositoryAction::Authorize {
            path: candidate.clone(),
            accepted: true,
        });
        until(service, |view| {
            view.active
                .as_ref()
                .is_some_and(|current| current.path == candidate)
        })
    }

    #[test]
    fn explicit_trust_switching_refresh_and_restart_use_real_repository_state() {
        let fixture = tempfile::tempdir().expect("isolated profile");
        let base = fixture.path().canonicalize().expect("native path");
        let profile = base.join("profile");
        let make_service = || {
            let profile = profile.clone();
            Workspaces::start_with(&egui::Context::default(), move || {
                Some(Store::at(&profile, &profile))
            })
        };
        let service = make_service();
        until(&service, |view| view.git_available);
        let first = base.join("first");
        let second = base.join("second");
        let first_view = prepare(&service, &first);
        assert!(
            matches!(first_view.active.expect("opened workspace").checkout, Checkout::Unborn(ref branch) if branch == "main")
        );
        prepare(&service, &second);

        // Both actions run through the same queue. A superseded Git result must
        // never replace the latest selection.
        service.send(RepositoryAction::Open(second.clone()));
        service.send(RepositoryAction::Open(first.clone()));
        let view = until(&service, |v| {
            v.active
                .as_ref()
                .is_some_and(|current| current.path == first)
                && v.activity == Activity::Idle
        });
        assert_eq!(view.recent.paths, [first.clone(), second.clone()]);
        std::fs::write(first.join("external"), b"real change")
            .expect("external working tree event");
        if !view.watching {
            service.send(RepositoryAction::Focus);
        }
        until(&service, |view| {
            view.active
                .as_ref()
                .and_then(|active| active.changes)
                .is_some_and(|c| c.untracked == 1)
        });

        let preferences = Preferences {
            scale_percent: 125,
            ..Preferences::default()
        };
        service.send(RepositoryAction::SavePreferences(preferences));
        drop(service);
        let store = Store::at(&profile, &profile);
        assert_eq!(
            store.load_preferences().expect("saved profile"),
            preferences
        );
        let service = make_service();
        let view = until(&service, |v| v.git_available);
        assert!(view.active.is_none());
        assert_eq!(view.recent.last.as_deref(), Some(first.as_path()));
        open(&service, &first, &first, false);
        assert!(
            until(&service, |view| view.issue
                == Some(WorkspaceIssue::Security))
            .active
            .is_none()
        );
        open(&service, &first, &first, true);
        until(&service, |view| view.active.is_some());
        service.send(RepositoryAction::Forget(first.clone()));
        until(&service, |view| {
            view.active.is_none() && !view.recent.paths.contains(&first)
        });
        assert!(first.join("external").exists());
        drop(service);
        std::fs::rename(&second, base.join("moved")).expect("repository moved externally");
        let service = make_service();
        until(&service, |view| view.git_available);
        service.send(RepositoryAction::Open(second.clone()));
        until(&service, |view| {
            view.unavailable.contains(&second) && view.issue == Some(WorkspaceIssue::Unavailable)
        });
        let ordinary = base.join("ordinary-directory");
        std::fs::create_dir(&ordinary).expect("directory without repository metadata");
        service.send(RepositoryAction::Open(ordinary));
        until(&service, |view| {
            view.issue == Some(WorkspaceIssue::NotRepository)
        });
    }
}
