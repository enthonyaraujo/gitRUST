use std::{
    path::{Path, PathBuf},
    thread,
};

use crossbeam_channel::Sender;
use gitrust_core::{
    github::{AccountId, Action, BrowserState, DevicePrompt, Notice, RepositoryKey, View},
    remotes::{RemoteAction, RemoteIntent, RemoteNotice},
    workspaces::Checkout,
};
use gitrust_providers::github::{Cancellation, DEVICE_URL};
use gitrust_storage::{GithubBindings, Store};

use super::Coordinator;
use crate::github_actor::{self, Effect, Job, Message};

struct CloneResult {
    destination: PathBuf,
    account: AccountId,
    parent: Option<RepositoryKey>,
}

#[derive(Default)]
pub(super) struct State {
    worker: Option<Sender<(Job, Cancellation)>>,
    cancellation: Option<Cancellation>,
    bindings: GithubBindings,
    before: Option<(PathBuf, u64)>,
    authorizing_remote: bool,
    cloned: Option<CloneResult>,
    git_write: bool,
    profile_read_failed: bool,
    inbox: Option<crate::browser_oauth::Inbox>,
    browser_url: Option<String>,
}

impl Coordinator {
    pub(super) fn initialize_github(&mut self, inbox: Option<crate::browser_oauth::Inbox>) {
        self.github.inbox = inbox;
        if let Some(store) = &self.store {
            if let Ok((device, browser, bindings)) = store.load_github() {
                self.view.github.client_id = device;
                self.view.github.browser_client_id = browser;
                self.github.bindings = bindings;
            } else {
                self.github.profile_read_failed = true;
                self.view.github.notice = Some(Notice::StorageUnavailable);
            }
        }
        if self.view.github.browser_client_id.is_empty()
            && let Some(id) = github_actor::configured_browser_client()
        {
            self.view.github.browser_client_id = id.as_str().into();
        }
    }

    pub(super) fn github_browser(&mut self, phase: BrowserState, url: Option<String>) {
        if self
            .github
            .cancellation
            .as_ref()
            .is_none_or(|cancel| cancel.check().is_err())
        {
            return;
        }
        self.view.github.browser_state = phase;
        self.github.browser_url = url;
        if phase == BrowserState::CallbackReceived {
            self.wake.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    pub(super) fn initialize_welcome(&mut self) {
        let completed = self.store.as_ref().map(Store::welcome_completed);
        self.view.offer_welcome = self.view.recent.paths.is_empty()
            && self.view.github.client_id.is_empty()
            && !self.view.storage_issue
            && matches!(completed, Some(Ok(false)));
        if self.view.github.client_id.is_empty()
            && let Some(id) = option_env!("GITRUST_GITHUB_CLIENT_ID")
                .filter(|id| gitrust_providers::github::ClientId::new(id).is_ok())
        {
            self.view.github.client_id = id.into();
        }
    }

    pub(super) fn complete_welcome(&mut self) {
        if std::mem::take(&mut self.view.offer_welcome)
            && self
                .store
                .as_ref()
                .is_some_and(|store| store.complete_welcome().is_err())
        {
            self.view.storage_issue = true;
        }
    }

    pub(super) fn shutdown_github(&mut self) {
        if let Some(cancel) = &self.github.cancellation {
            cancel.cancel();
        }
        self.github.worker.take();
    }

    pub(super) fn github_code(&mut self, code: DevicePrompt) {
        present_device_code(
            &mut self.view.github,
            self.github.cancellation.as_ref(),
            code,
            open_browser,
        );
    }

    pub(super) fn cancel_github(&mut self) {
        if let Some(cancel) = &self.github.cancellation {
            cancel.cancel();
        }
    }

    pub(super) fn sync_github_workspace(&mut self) {
        let path = self
            .view
            .active
            .as_ref()
            .map(|workspace| workspace.path.clone());
        let branch = self.view.active.as_ref().and_then(|workspace| {
            if let Checkout::Branch(name) = &workspace.checkout {
                Some(name.clone())
            } else {
                None
            }
        });
        let repo = self.view.remotes.as_ref().and_then(|view| {
            view.entries
                .iter()
                .find(|remote| remote.name == "origin")
                .and_then(|remote| remote.fetch.as_deref().and_then(RepositoryKey::from_remote))
                .or_else(|| {
                    view.entries
                        .iter()
                        .filter_map(|remote| remote.fetch.as_deref())
                        .find_map(RepositoryKey::from_remote)
                })
        });
        if self.view.github.workspace != path || self.view.github.workspace_repo != repo {
            self.view.github.checks = None;
        }
        self.view.github.workspace_account = path
            .as_ref()
            .and_then(|path| self.github.bindings.get(path).copied());
        self.view.github.workspace = path;
        self.view.github.workspace_branch = branch;
        self.view.github.workspace_revision = self
            .view
            .navigation
            .as_ref()
            .and_then(|nav| nav.head.clone());
        if let Some(repo) = &repo
            && !self.view.github.local_repositories.contains(repo)
        {
            let mut repositories = self.view.github.local_repositories.to_vec();
            repositories.push(repo.clone());
            self.view.github.local_repositories = repositories.into();
        }
        self.view.github.workspace_repo = repo;
    }

    pub(super) fn github_action(&mut self, action: Action) {
        match action {
            Action::Open(url) => open_browser(&url),
            Action::Cancel => self.cancel_github(),
            Action::ReopenBrowser => {
                if self.view.github.browser_state == BrowserState::AwaitingBrowserAuthorization
                    && let Some(url) = &self.github.browser_url
                    && !launch_browser(url)
                {
                    self.view.github.notice = Some(Notice::OAuthHandler);
                }
            }
            Action::BrowserSignIn { reconnect } => {
                if self.github.inbox.is_none() {
                    self.view.github.notice = Some(Notice::OAuthHandler);
                } else if github_actor::configured_browser_client()
                    .is_none_or(|id| id.as_str() != self.view.github.browser_client_id)
                {
                    self.view.github.notice = Some(Notice::OAuthConfiguration);
                } else {
                    self.submit_github(Job::Action(Action::BrowserSignIn { reconnect }));
                }
            }
            Action::Bind(path, account) => self.bind_github(&path, account),
            Action::Restore { .. } => self.submit_github(Job::Action(Action::Restore {
                device: self.view.github.client_id.clone(),
                browser: self.view.github.browser_client_id.clone(),
            })),
            Action::Create(plan)
                if plan.local.as_ref().is_some_and(|local| {
                    !self.valid_publish(local, RepositoryKey::new(&plan.owner, &plan.name).as_ref())
                }) =>
            {
                self.view.github.notice = Some(Notice::InvalidInput);
            }
            Action::ResumePublish {
                ref publish,
                ref repository,
            } if !self.valid_publish(publish, Some(repository)) => {
                self.view.github.notice = Some(Notice::InvalidInput);
            }
            Action::CheckoutPull { ref repository, .. } if !self.valid_checkout(repository) => {
                self.view.github.notice = Some(Notice::InvalidInput);
            }
            Action::SignIn { ref client_id, .. }
                if !self.view.github.client_id.is_empty()
                    && self.view.github.client_id != *client_id =>
            {
                self.view.github.notice = Some(Notice::InvalidClient);
            }
            action => self.submit_github(Job::Action(action)),
        }
    }

    fn valid_publish(
        &self,
        plan: &gitrust_core::github::Publish,
        repo: Option<&RepositoryKey>,
    ) -> bool {
        let Some(repo) = repo else {
            return false;
        };
        let url = repo.https();
        let view = &self.view.github;
        view.workspace.as_ref() == Some(&plan.workspace)
            && view.workspace_branch.as_ref() == Some(&plan.branch)
            && self
                .changes
                .status
                .as_ref()
                .is_some_and(|status| status.branch.oid.is_some())
            && self.pending.is_none()
            && view
                .workspace_account
                .is_none_or(|bound| view.active == Some(bound))
            && !plan.remote.starts_with('-')
            && gitrust_git::ReferenceName::new(format!("refs/remotes/{}/main", plan.remote)).is_ok()
            && self.view.remotes.as_ref().is_some_and(|remotes| {
                !remotes.entries.iter().any(|entry| {
                    entry.name == plan.remote
                        && (entry.fetch.as_deref() != Some(&url)
                            || entry.push.as_deref() != Some(&url))
                })
            })
    }

    fn valid_checkout(&self, repository: &RepositoryKey) -> bool {
        self.view.github.workspace_repo.as_ref() == Some(repository)
            && self
                .view
                .github
                .workspace_account
                .is_none_or(|account| self.view.github.active == Some(account))
            && self
                .view
                .remotes
                .as_ref()
                .is_some_and(|remote| !remote.worktree_dirty)
            && self.pending.is_none()
    }

    fn bind_github(&mut self, path: &Path, account: Option<AccountId>) {
        if self.view.github.workspace.as_deref() != Some(path) || self.github.cancellation.is_some()
        {
            return;
        }
        if let Some(id) = account {
            if !self
                .view
                .github
                .accounts
                .iter()
                .any(|account| account.id == id)
            {
                self.view.github.notice = Some(Notice::Reconnect);
                return;
            }
            self.github.bindings.insert(path.to_owned(), id);
        } else {
            self.github.bindings.remove(path);
        }
        self.save_github();
        self.sync_github_workspace();
    }

    fn save_github(&mut self) {
        if self.github.profile_read_failed
            || self.store.as_ref().is_none_or(|store| {
                store
                    .save_github(
                        &self.view.github.client_id,
                        &self.view.github.browser_client_id,
                        &self.github.bindings,
                    )
                    .is_err()
            })
        {
            self.view.github.notice = Some(Notice::StorageUnavailable);
        }
    }

    pub(super) fn submit_github(&mut self, job: Job) {
        if self.github.cancellation.is_some() {
            return;
        }
        if self.github.worker.is_none() {
            let (sender, incoming) = crossbeam_channel::bounded(1);
            let events = self.events.clone();
            let inbox = self.github.inbox.clone();
            if thread::Builder::new()
                .name("gitrust-github".into())
                .spawn(move || github_actor::run(&incoming, &events, inbox.as_ref()))
                .is_err()
            {
                self.view.github.notice = Some(Notice::Unavailable);
                return;
            }
            self.github.worker = Some(sender);
        }
        let cancel = Cancellation::default();
        let authorizing_remote = matches!(&job, Job::Authorize { .. });
        if self
            .github
            .worker
            .as_ref()
            .is_some_and(|worker| worker.try_send((job, cancel.clone())).is_ok())
        {
            self.github.before = self
                .view
                .github
                .workspace
                .clone()
                .zip(self.view.remotes.as_ref().map(|view| view.version));
            self.github.cancellation = Some(cancel);
            self.github.authorizing_remote = authorizing_remote;
            self.view.github.busy = true;
            self.view.github.notice = None;
        } else {
            self.view.github.notice = Some(Notice::Unavailable);
        }
    }

    pub(super) fn complete_github(&mut self, result: Message) {
        let cancelled = self
            .github
            .cancellation
            .take()
            .is_some_and(|flag| flag.check().is_err());
        let before = self.github.before.take();
        let authorizing_remote = std::mem::take(&mut self.github.authorizing_remote);
        let previous_client = self.view.github.client_id.clone();
        self.github.browser_url = None;
        let previous_view = std::mem::replace(&mut self.view.github, result.view);
        self.view.github.local_repositories = previous_view.local_repositories;
        self.view.github.workspace = previous_view.workspace;
        self.view.github.workspace_repo = previous_view.workspace_repo;
        if self.view.github.browser_client_id.is_empty() {
            self.view.github.browser_client_id = previous_view.browser_client_id;
        }
        self.sync_github_workspace();
        if self.view.github.client_id.is_empty() {
            self.view.github.client_id.clone_from(&previous_client);
        }
        if self.view.github.active.is_some() && !cancelled {
            self.complete_welcome();
        }
        if self.view.github.active.is_some()
            && (previous_client != self.view.github.client_id || previous_view.active.is_none())
        {
            self.save_github();
        }
        match result.effect {
            Effect::None => {}
            Effect::Open(url) => open_browser(&url),
            Effect::Disconnect(account) => {
                self.github.bindings.retain(|_, id| *id != account);
                self.save_github();
                self.sync_github_workspace();
            }
            Effect::Clone {
                destination,
                url,
                auth,
                parent,
                account,
            } if !cancelled && self.pending.is_none() && self.candidate.is_none() => {
                self.clone(&destination, &url, auth);
                if self.pending.is_some() {
                    self.github.cloned = Some(CloneResult {
                        destination,
                        account,
                        parent,
                    });
                } else {
                    self.view.github.notice = Some(Notice::LocalGitFailed);
                }
            }
            Effect::Remote { workspace, intent }
                if !cancelled
                    && self.view.github.workspace.as_ref() == Some(&workspace)
                    && self
                        .view
                        .remotes
                        .as_ref()
                        .is_some_and(|view| view.version == intent.version)
                    && self.pending.is_none() =>
            {
                self.on_remote(intent);
            }
            Effect::Mutate { workspace, action }
                if !cancelled
                    && self.pending.is_none()
                    && before.as_ref().map(|(path, version)| (path, *version))
                        == self
                            .view
                            .github
                            .workspace
                            .as_ref()
                            .zip(self.view.remotes.as_ref().map(|view| view.version))
                    && workspace
                        .as_ref()
                        .is_none_or(|path| self.view.github.workspace.as_ref() == Some(path)) =>
            {
                if let Some(view) = &self.view.remotes {
                    self.on_remote(RemoteIntent {
                        version: view.version,
                        action,
                    });
                    self.github.git_write = self.pending.is_some();
                }
            }
            _ => {
                self.view.github.notice = Some(if cancelled {
                    Notice::Cancelled
                } else {
                    Notice::LocalGitFailed
                });
            }
        }
        self.report_remote_authorization(authorizing_remote.then_some(before).flatten());
    }

    fn report_remote_authorization(&mut self, workspace: Option<(PathBuf, u64)>) {
        if let (Some((path, version)), Some(notice)) = (workspace, self.view.github.notice)
            && self.view.activity == gitrust_core::workspaces::Activity::Idle
            && self.view.github.workspace.as_ref() == Some(&path)
            && let Some(remotes) = self
                .view
                .remotes
                .as_mut()
                .filter(|view| view.version == version)
        {
            remotes.notice = Some(match notice {
                Notice::Cancelled => RemoteNotice::Cancelled,
                Notice::Timeout => RemoteNotice::Timeout,
                Notice::Offline => RemoteNotice::Offline,
                Notice::LocalGitFailed => RemoteNotice::Stale,
                _ => RemoteNotice::Authentication,
            });
        }
    }

    pub(super) fn github_clone_failed(&mut self) {
        if self.github.cloned.take().is_some() {
            self.view.github.notice = Some(Notice::LocalGitFailed);
        }
    }

    pub(super) fn verify_github_clone(&mut self, path: &Path, accepted: bool) {
        if let Some(clone) = self.github.cloned.take()
            && accepted
            && std::fs::canonicalize(&clone.destination).is_ok_and(|original| original == path)
        {
            self.github.bindings.insert(path.to_owned(), clone.account);
            self.save_github();
            self.github.cloned = Some(clone);
        }
    }

    pub(super) fn finish_github_clone(&mut self) {
        let Some(clone) = self.github.cloned.take() else {
            return;
        };
        if let Some(parent) = clone.parent
            && let Some(view) = &self.view.remotes
            && !view.entries.iter().any(|remote| remote.name == "upstream")
        {
            self.on_remote(RemoteIntent {
                version: view.version,
                action: RemoteAction::Add("upstream".into(), parent.https(), None),
            });
            self.github.git_write = self.pending.is_some();
        }
    }

    pub(super) fn complete_github_write(&mut self, outcome: Option<RemoteNotice>) {
        if std::mem::take(&mut self.github.git_write) {
            self.view.github.notice = Some(match outcome {
                Some(RemoteNotice::Pushed | RemoteNotice::UpToDate | RemoteNotice::Updated) => {
                    Notice::LocalGitComplete
                }
                Some(RemoteNotice::Uncertain) => Notice::UnknownMutation,
                _ => Notice::LocalGitFailed,
            });
        }
    }
}

fn present_device_code(
    view: &mut View,
    pending: Option<&Cancellation>,
    code: DevicePrompt,
    launch: impl FnOnce(&str),
) {
    if view.busy && view.prompt.is_none() && pending.is_some_and(|cancel| cancel.check().is_ok()) {
        view.prompt = Some(code);
        launch(DEVICE_URL);
    }
}

fn open_browser(url: &str) {
    if gitrust_providers::github::browser_url_allowed(url) {
        launch_browser(url);
    }
}

// The only query-bearing URL is generated from the private in-flight OAuth attempt.
pub(crate) fn launch_browser(url: &str) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    use std::process::{Command, Stdio};

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
    let mut command = {
        let Some(root) = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|root| root.is_absolute())
        else {
            return false;
        };
        Command::new(root.join("explorer.exe"))
    };
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    if let Ok(mut child) = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        let _ = thread::Builder::new().spawn(move || child.wait());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_official_authorization_page_once_per_active_prompt() {
        let code = DevicePrompt {
            code: "ABCD-EFGH".into(),
            expires_in: 300,
        };
        let mut view = View {
            busy: true,
            ..View::default()
        };
        let cancel = Cancellation::default();
        let mut opens = 0;
        present_device_code(&mut view, Some(&cancel), code.clone(), |url| {
            assert_eq!(url, "https://github.com/login/device");
            opens += 1;
        });
        assert_eq!(view.prompt.as_ref(), Some(&code));
        present_device_code(&mut view, Some(&cancel), code.clone(), |_| opens += 1);
        assert_eq!(opens, 1);

        view.prompt = None;
        cancel.cancel();
        present_device_code(&mut view, Some(&cancel), code.clone(), |_| opens += 1);
        present_device_code(&mut view, None, code, |_| opens += 1);
        assert!(view.prompt.is_none());
        assert_eq!(opens, 1);
    }
}
