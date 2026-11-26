use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use crossbeam_channel::{Receiver, Sender};
use gitrust_core::{
    github::{Account, Action, Connection, DevicePrompt, Notice, RepositoryKey, View},
    remotes::{Auth, Credential, RemoteAction, RemoteIntent},
};
use gitrust_providers::github::{Cancellation, Client, ClientId};
use gitrust_storage::CredentialVault;

use crate::workspaces::WorkerEvent;

use gitrust_providers::github::Session;

pub(crate) enum Job {
    Action(Action),
    Authorize {
        account: u64,
        endpoint: String,
        workspace: PathBuf,
        intent: RemoteIntent,
    },
}

pub(crate) enum Effect {
    None,
    Open(String),
    Clone {
        destination: PathBuf,
        url: String,
        auth: Auth,
        parent: Option<RepositoryKey>,
        account: u64,
    },
    Remote {
        workspace: PathBuf,
        intent: RemoteIntent,
    },
    Mutate {
        workspace: Option<PathBuf>,
        action: RemoteAction,
    },
    Disconnect(u64),
}

pub(crate) struct Message {
    pub(crate) view: View,
    pub(crate) effect: Effect,
}

struct Saved {
    session: Session,
    state: Connection,
    has_vault: bool,
}

struct Worker {
    client: Client,
    vault: CredentialVault,
    client_id: Option<ClientId>,
    sessions: BTreeMap<u64, Saved>,
    view: View,
}

pub(crate) fn run(incoming: &Receiver<(Job, Cancellation)>, updates: &Sender<WorkerEvent>) {
    let mut worker = Worker {
        client: Client::default(),
        vault: CredentialVault,
        client_id: None,
        sessions: BTreeMap::new(),
        view: View::default(),
    };
    while let Ok((job, cancel)) = incoming.recv() {
        let old = worker.view.active;
        let affected = match &job {
            Job::Authorize { account, .. } => Some(*account),
            Job::Action(Action::SignIn { reconnect, .. }) => *reconnect,
            Job::Action(Action::Disconnect(id) | Action::Select(id)) => Some(*id),
            Job::Action(_) => old,
        };
        worker.view.notice = None;
        let effect = match worker.run(job, &cancel, |prompt| {
            let _ = updates.send(WorkerEvent::GithubCode(prompt));
        }) {
            Ok(effect) => effect,
            Err(error) => {
                worker.view.notice = Some(error);
                if error == Notice::Reconnect
                    && let Some(saved) = affected.and_then(|id| worker.sessions.get_mut(&id))
                {
                    saved.state = Connection::Reconnect;
                }
                Effect::None
            }
        };
        worker.view.prompt = None;
        if old != worker.view.active {
            worker.clear_query();
        }
        worker.view.accounts = worker
            .sessions
            .values()
            .map(|saved| Account {
                profile: saved.session.profile(),
                connection: saved.state,
            })
            .collect();
        if updates
            .send(WorkerEvent::Github(Box::new(Message {
                view: worker.view.clone(),
                effect,
            })))
            .is_err()
        {
            break;
        }
    }
}

impl Worker {
    fn clear_query(&mut self) {
        self.view.repositories = Arc::default();
        self.view.repository_page = None;
        self.view.organizations = Arc::default();
        self.view.pulls = Arc::default();
        self.view.pull_repository = None;
        self.view.pull_page = None;
        self.view.checks = None;
        self.view.check_scope = None;
        self.view.result = None;
        self.view.result_pull = None;
        self.view.review = None;
        self.view.avatar = None;
    }

    fn restore(&mut self, client: &str) -> Result<(), Notice> {
        let id = ClientId::new(client)?;
        if self.client_id.as_ref() != Some(&id) {
            self.sessions.clear();
            self.view.active = None;
            self.clear_query();
        }
        self.client_id = Some(id);
        client.clone_into(&mut self.view.client_id);
        for (account, data) in self
            .vault
            .load(client)
            .map_err(|_| Notice::StorageUnavailable)?
        {
            if let Ok(session) = Session::from_vault(account, &data) {
                self.sessions.entry(account).or_insert(Saved {
                    session,
                    state: Connection::Stored,
                    has_vault: true,
                });
            }
        }
        self.view.active = self
            .view
            .active
            .or_else(|| self.sessions.keys().next().copied());
        Ok(())
    }

    fn login(
        &mut self,
        client: &str,
        reconnect: Option<u64>,
        cancel: &Cancellation,
        prompt: impl FnOnce(DevicePrompt),
    ) -> Result<(), Notice> {
        let id = ClientId::new(client)?;
        let session = self.client.login(&id, cancel, prompt)?;
        let profile = session.profile();
        if reconnect.is_some_and(|old| old != profile.id) {
            return Err(Notice::InvalidInput);
        }
        if self.client_id.as_ref() != Some(&id) {
            self.sessions.clear();
            self.clear_query();
        }
        client.clone_into(&mut self.view.client_id);
        let stored = self
            .vault
            .store(id.as_str(), profile.id, &session.for_vault()?)
            .is_ok();
        let state = if stored {
            Connection::Stored
        } else {
            Connection::Temporary
        };
        let has_vault = stored
            || self
                .sessions
                .get(&profile.id)
                .is_some_and(|entry| entry.has_vault);
        self.sessions.insert(
            profile.id,
            Saved {
                session,
                state,
                has_vault,
            },
        );
        self.client_id = Some(id);
        self.view.active = Some(profile.id);
        if !stored {
            self.view.notice = Some(Notice::TemporarySession);
        }
        Ok(())
    }

    fn authorize(&mut self, account: u64, cancel: &Cancellation) -> Result<(), Notice> {
        let saved = self.sessions.get_mut(&account).ok_or(Notice::Reconnect)?;
        if saved.state == Connection::Reconnect {
            return Err(Notice::Reconnect);
        }
        if saved.session.needs_refresh() {
            let client = self.client_id.as_ref().ok_or(Notice::InvalidClient)?;
            match self.client.refresh(client, &saved.session, cancel) {
                Ok(session) => saved.session = session,
                Err(error) => {
                    if matches!(
                        error,
                        Notice::Reconnect | Notice::Expired | Notice::UnknownMutation
                    ) {
                        saved.state = Connection::Reconnect;
                    }
                    return Err(error);
                }
            }
            if saved.state == Connection::Stored
                && self
                    .vault
                    .store(client.as_str(), account, &saved.session.for_vault()?)
                    .is_err()
            {
                saved.state = Connection::Temporary;
                self.view.notice = Some(Notice::TemporarySession);
            }
        }
        Ok(())
    }

    fn disconnect(&mut self, account: u64) -> Result<(), Notice> {
        let entry = self.sessions.get(&account).ok_or(Notice::Reconnect)?;
        if entry.has_vault
            && let Some(client) = &self.client_id
        {
            self.vault
                .remove(client.as_str(), account)
                .map_err(|_| Notice::StorageUnavailable)?;
        }
        self.sessions.remove(&account);
        if self.view.active == Some(account) {
            self.view.active = self.sessions.keys().next().copied();
        }
        self.view.notice = Some(Notice::SignedOut);
        Ok(())
    }

    fn run(
        &mut self,
        job: Job,
        cancel: &Cancellation,
        show: impl FnOnce(DevicePrompt),
    ) -> Result<Effect, Notice> {
        cancel.check()?;
        let action = match job {
            Job::Authorize {
                account,
                endpoint,
                workspace,
                mut intent,
            } => {
                let repository =
                    RepositoryKey::from_https(&endpoint).ok_or(Notice::InvalidInput)?;
                self.authorize(account, cancel)?;
                let saved = self.sessions.get_mut(&account).ok_or(Notice::Reconnect)?;
                if let Err(error) =
                    self.client
                        .repository(saved.session.access(), &repository, cancel)
                {
                    if error == Notice::Reconnect {
                        saved.state = Connection::Reconnect;
                    }
                    return Err(error);
                }
                let auth = scoped(endpoint, saved.session.access());
                match &mut intent.action {
                    RemoteAction::Fetch(_, current)
                    | RemoteAction::Pull(_, _, current)
                    | RemoteAction::Push { auth: current, .. } => *current = auth,
                    _ => return Err(Notice::InvalidInput),
                }
                return Ok(Effect::Remote { workspace, intent });
            }
            Job::Action(action) => action,
        };
        match action {
            Action::Restore(client) => self.restore(&client)?,
            Action::SignIn {
                client_id,
                reconnect,
            } => self.login(&client_id, reconnect, cancel, show)?,
            Action::Select(id) => {
                if !self.sessions.contains_key(&id) {
                    return Err(Notice::Reconnect);
                }
                self.view.active = Some(id);
            }
            Action::Disconnect(id) => {
                self.disconnect(id)?;
                return Ok(Effect::Disconnect(id));
            }
            Action::Open(url) if gitrust_providers::github::browser_url_allowed(&url) => {
                return Ok(Effect::Open(url));
            }
            Action::Open(_) => return Err(Notice::InvalidInput),
            action => return self.account_action(action, cancel),
        }
        Ok(Effect::None)
    }

    #[allow(clippy::too_many_lines)]
    fn account_action(&mut self, action: Action, cancel: &Cancellation) -> Result<Effect, Notice> {
        let account = self.view.active.ok_or(Notice::Reconnect)?;
        self.authorize(account, cancel)?;
        let entry = self.sessions.get(&account).ok_or(Notice::Reconnect)?;
        let token = entry.session.access();
        let profile = entry.session.profile();
        let client = &self.client;
        match action {
            Action::Refresh => {
                let current = client.profile(token, cancel)?;
                if current.id != account {
                    return Err(Notice::Reconnect);
                }
                let mut organizations = Vec::new();
                for page in 1..=10 {
                    let result = client.installations(token, page, cancel)?;
                    organizations.extend(result.items);
                    if result.next.is_none() {
                        break;
                    }
                }
                self.view.organizations = organizations.into();
                let repositories = client.repositories(token, None, 1, cancel)?;
                self.view.repositories = repositories.items.into();
                self.view.repository_page = repositories.next;
                self.view.avatar = load_avatar(client, account, cancel);
            }
            Action::Repositories { installation, page } => {
                if installation.is_some_and(|id| {
                    !self
                        .view
                        .organizations
                        .iter()
                        .any(|org| org.installation == id)
                }) {
                    return Err(Notice::NoAccess);
                }
                let page = client.repositories(token, installation, page, cancel)?;
                self.view.repositories = page.items.into();
                self.view.repository_page = page.next;
            }
            Action::Clone {
                repository,
                parent,
                destination,
                ssh,
            } => {
                let repo = match client.repository(token, &repository, cancel) {
                    Ok(repo) => repo,
                    Err(Notice::Forbidden | Notice::NoAccess) => {
                        client.public_repository(&repository, cancel)?
                    }
                    Err(error) => return Err(error),
                };
                if parent.is_some() && parent != repo.parent {
                    return Err(Notice::InvalidInput);
                }
                let url = if ssh {
                    repo.key.ssh()
                } else {
                    repo.key.https()
                };
                let auth = if ssh || !repo.private {
                    Auth::default()
                } else {
                    scoped(url.clone(), token)
                };
                return Ok(Effect::Clone {
                    destination,
                    url,
                    auth,
                    parent: repo.parent.filter(|_| parent.is_some()),
                    account,
                });
            }
            Action::Create(plan) => {
                let organization = !profile.login.eq_ignore_ascii_case(&plan.owner);
                if organization
                    && !self
                        .view
                        .organizations
                        .iter()
                        .any(|org| org.login.eq_ignore_ascii_case(&plan.owner))
                {
                    return Err(Notice::NoAccess);
                }
                let created = client.create_repository(token, &plan, organization, cancel)?;
                self.view.result = Some(created.key.clone());
                self.view.notice = Some(Notice::Created);
                if let Some(publish) = plan.local {
                    return self.publish(token, &created.key, publish, cancel);
                }
            }
            Action::ResumePublish {
                repository,
                publish,
            } => return self.publish(token, &repository, publish, cancel),
            Action::Fork {
                repository,
                organization,
                name,
            } => {
                if organization.as_ref().is_some_and(|owner| {
                    !self
                        .view
                        .organizations
                        .iter()
                        .any(|org| org.login.eq_ignore_ascii_case(owner))
                }) {
                    return Err(Notice::NoAccess);
                }
                let expected =
                    RepositoryKey::new(organization.as_deref().unwrap_or(&profile.login), &name)
                        .ok_or(Notice::InvalidInput)?;
                let fork =
                    client.fork(token, &repository, organization.as_deref(), &name, cancel)?;
                if !fork.key.slug().eq_ignore_ascii_case(&expected.slug()) {
                    return Err(Notice::UnknownMutation);
                }
                self.view.result = Some(fork.key);
                self.view.notice = Some(Notice::ForkPending);
            }
            Action::Repository(repository) => {
                let repo = match client.repository(token, &repository, cancel) {
                    Ok(repo) => repo,
                    Err(Notice::Forbidden | Notice::NoAccess) => {
                        client.public_repository(&repository, cancel)?
                    }
                    Err(error) => return Err(error),
                };
                let key = repo.key.clone();
                self.view.repositories = vec![repo].into();
                self.view.repository_page = None;
                self.view.result = Some(key);
            }
            Action::SyncFork { repository, branch } => {
                let repo = client.repository(token, &repository, cancel)?;
                if !repo.fork || !repo.can_push {
                    return Err(Notice::Forbidden);
                }
                client.sync_fork(token, &repository, &branch, cancel)?;
                self.view.notice = Some(Notice::Created);
            }
            Action::Pulls {
                repository,
                page,
                state,
            } => {
                let page = client.pulls(token, &repository, page, state, cancel)?;
                self.view.pulls = page.items.into();
                self.view.pull_repository = Some(repository);
                self.view.pull_page = page.next;
                self.view.checks = None;
            }
            Action::CreatePull(pull) => {
                let repo = client.repository(token, &pull.source, cancel)?;
                if !repo.can_push {
                    return Err(Notice::Forbidden);
                }
                let created = client.create_pull(token, &pull, cancel)?;
                self.view.result_pull = Some((pull.destination, created.number));
                self.view.review = Some(created);
                self.view.notice = Some(Notice::Created);
            }
            Action::CheckoutPull {
                repository,
                number,
                local,
            } => {
                let pull = client.pull(token, &repository, number, cancel)?;
                let endpoint = repository.https();
                return Ok(Effect::Mutate {
                    workspace: None,
                    action: RemoteAction::CheckoutReview {
                        revision: pull.revision,
                        branch: local,
                        auth: scoped(endpoint.clone(), token),
                        endpoint,
                    },
                });
            }
            Action::Checks {
                repository,
                revision,
            } => {
                self.view.checks = None;
                self.view.workflows = false;
                self.view.checks =
                    Some(client.checks(token, &repository, &revision, cancel)?.into());
                self.view.check_scope = Some((repository, revision));
            }
            Action::Workflows {
                repository,
                revision,
            } => {
                self.view.checks = None;
                self.view.workflows = true;
                self.view.checks = Some(
                    client
                        .workflow_runs(token, &repository, &revision, cancel)?
                        .into(),
                );
                self.view.check_scope = Some((repository, revision));
            }
            _ => return Err(Notice::InvalidInput),
        }
        Ok(Effect::None)
    }

    fn publish(
        &self,
        token: &str,
        repo: &RepositoryKey,
        local: gitrust_core::github::Publish,
        cancel: &Cancellation,
    ) -> Result<Effect, Notice> {
        let confirmed = self
            .client
            .repository(token, repo, cancel)
            .map_err(|error| {
                if matches!(error, Notice::Forbidden | Notice::NoAccess) {
                    Notice::AwaitingInstallation
                } else {
                    error
                }
            })?;
        if !confirmed.can_push {
            return Err(Notice::AwaitingInstallation);
        }
        let url = confirmed.key.https();
        Ok(Effect::Mutate {
            workspace: Some(local.workspace),
            action: RemoteAction::Publish {
                remote: local.remote,
                branch: local.branch,
                auth: scoped(url.clone(), token),
                url,
            },
        })
    }
}

fn scoped(endpoint: String, token: &str) -> Auth {
    Auth {
        credential: Credential::ScopedToken {
            endpoint,
            secret: token.into(),
        },
        ..Auth::default()
    }
}

fn load_avatar(
    client: &Client,
    account: u64,
    cancel: &Cancellation,
) -> Option<std::sync::Arc<gitrust_core::github::Avatar>> {
    let bytes = client.avatar(account, cancel).ok()?;
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(128);
    limits.max_image_height = Some(128);
    limits.max_alloc = Some(256 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.into_rgba8();
    let avatar = gitrust_core::github::Avatar {
        width: rgba.width().try_into().ok()?,
        height: rgba.height().try_into().ok()?,
        rgba: rgba.into_raw(),
    };
    Some(std::sync::Arc::new(avatar))
}
