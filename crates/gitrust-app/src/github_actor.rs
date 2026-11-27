use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use crossbeam_channel::{Receiver, Sender};
use gitrust_core::{
    github::{
        Account, AccountId, Action, AuthMethod, BrowserState, Connection, Notice, RepositoryKey,
        View,
    },
    remotes::{Auth, Credential, RemoteAction, RemoteIntent},
};
use gitrust_providers::github::{BrowserAttempt, Cancellation, Client, ClientId};
use gitrust_storage::CredentialVault;

use crate::workspaces::WorkerEvent;

use gitrust_providers::github::Session;

pub(crate) enum Job {
    Action(Action),
    Authorize {
        account: AccountId,
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
        account: AccountId,
    },
    Remote {
        workspace: PathBuf,
        intent: RemoteIntent,
    },
    Mutate {
        workspace: Option<PathBuf>,
        action: RemoteAction,
    },
    Disconnect(AccountId),
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
    sessions: BTreeMap<AccountId, Saved>,
    browser_client: Option<ClientId>,
    view: View,
}

pub(crate) fn configured_browser_client() -> Option<ClientId> {
    std::env::var("GITRUST_GITHUB_OAUTH_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("GITRUST_GITHUB_OAUTH_CLIENT_ID").map(str::to_owned))
        .and_then(|value| ClientId::new(&value).ok())
}

pub(crate) fn run(
    incoming: &Receiver<(Job, Cancellation)>,
    updates: &Sender<WorkerEvent>,
    inbox: Option<&crate::browser_oauth::Inbox>,
) {
    let mut worker = Worker {
        client: Client::default(),
        vault: CredentialVault,
        client_id: None,
        browser_client: None,
        sessions: BTreeMap::new(),
        view: View::default(),
    };
    while let Ok((job, cancel)) = incoming.recv() {
        let old = worker.view.active;
        let affected = match &job {
            Job::Authorize { account, .. } => Some(*account),
            Job::Action(Action::SignIn { reconnect, .. } | Action::BrowserSignIn { reconnect }) => {
                *reconnect
            }
            Job::Action(Action::Disconnect(id) | Action::Select(id)) => Some(*id),
            Job::Action(_) => old,
        };
        worker.view.notice = None;
        let browser = matches!(job, Job::Action(Action::BrowserSignIn { .. }));
        worker.view.browser_state = BrowserState::Idle;
        let effect = match worker.run(job, &cancel, updates, inbox) {
            Ok(effect) => {
                if browser {
                    worker.view.browser_state = BrowserState::Authenticated;
                }
                effect
            }
            Err(error) => {
                worker.view.notice = Some(error);
                if browser {
                    worker.view.browser_state = match error {
                        Notice::Cancelled => BrowserState::Cancelled,
                        Notice::Expired => BrowserState::Expired,
                        _ => BrowserState::Failed,
                    };
                }
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
        if browser
            && worker.view.browser_state == BrowserState::Authenticated
            && let Some(account) = worker.view.active
        {
            worker.view.avatar = load_avatar(&worker.client, account.number, &cancel);
        }
        worker.view.accounts = worker
            .sessions
            .iter()
            .map(|(id, saved)| Account {
                id: *id,
                scopes: saved.session.scopes().into(),
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

    fn restore(&mut self, client: &str, method: AuthMethod) -> Result<(), Notice> {
        if client.is_empty() {
            return Ok(());
        }
        let id = ClientId::new(client)?;
        let records = self
            .vault
            .load(client, method)
            .map_err(|_| Notice::StorageUnavailable)?;
        self.set_client(&id, method);
        for (number, data) in records {
            let parsed = match method {
                AuthMethod::Device => Session::from_vault(number, &data),
                AuthMethod::Browser => Session::from_browser_vault(number, &id, &data),
            };
            if let Ok(session) = parsed {
                self.sessions
                    .entry(AccountId { number, method })
                    .or_insert(Saved {
                        session,
                        state: Connection::Stored,
                        has_vault: true,
                    });
            }
        }
        if self.view.active.is_none() {
            self.view.active = self.sessions.keys().next().copied();
        }
        Ok(())
    }

    fn set_client(&mut self, id: &ClientId, method: AuthMethod) {
        let (current, public) = match method {
            AuthMethod::Device => (&mut self.client_id, &mut self.view.client_id),
            AuthMethod::Browser => (&mut self.browser_client, &mut self.view.browser_client_id),
        };
        if current.as_ref() == Some(id) {
            return;
        }
        *current = Some(id.clone());
        id.as_str().clone_into(public);
        self.sessions.retain(|key, _| key.method != method);
        if self.view.active.is_some_and(|key| key.method == method) {
            self.view.active = None;
            self.clear_query();
        }
    }

    fn save_session(
        &mut self,
        client: &ClientId,
        method: AuthMethod,
        reconnect: Option<AccountId>,
        session: Session,
        cancel: &Cancellation,
    ) -> Result<(), Notice> {
        let key = AccountId {
            method,
            number: session.profile().id,
        };
        if reconnect.is_some_and(|old| old != key) {
            return Err(Notice::WrongAccount);
        }
        cancel.check()?;
        let stored = self
            .vault
            .store(client.as_str(), method, key.number, &session.for_vault()?)
            .is_ok();
        if !stored && method == AuthMethod::Browser {
            return Err(Notice::StorageUnavailable);
        }
        self.set_client(client, method);
        let state = if stored {
            Connection::Stored
        } else {
            Connection::Temporary
        };
        let has_vault = stored || self.sessions.get(&key).is_some_and(|entry| entry.has_vault);
        self.sessions.insert(
            key,
            Saved {
                session,
                state,
                has_vault,
            },
        );
        self.view.active = Some(key);
        self.view.notice = Some(if stored {
            Notice::SignedIn
        } else {
            Notice::TemporarySession
        });
        Ok(())
    }

    fn browser_login(
        &mut self,
        reconnect: Option<AccountId>,
        cancel: &Cancellation,
        events: &Sender<WorkerEvent>,
        inbox: Option<&crate::browser_oauth::Inbox>,
    ) -> Result<(), Notice> {
        let notify = |phase, url| {
            let _ = events.send(WorkerEvent::GithubBrowser(phase, url));
        };
        notify(BrowserState::Starting, None);
        let inbox = inbox.ok_or(Notice::OAuthHandler)?;
        let client = configured_browser_client().ok_or(Notice::OAuthConfiguration)?;
        let secret = self
            .vault
            .oauth_client_secret(client.as_str())
            .map_err(|_| Notice::OAuthConfiguration)?;
        cancel.check()?;
        crate::browser_oauth::register()?;
        let attempt = BrowserAttempt::new()?;
        let url = attempt.authorize_url(&client);
        let (attempt, callback) = inbox.wait_for(attempt, cancel, || {
            if !crate::workspaces::launch_browser(&url) {
                return Err(Notice::OAuthHandler);
            }
            notify(BrowserState::AwaitingBrowserAuthorization, Some(url));
            Ok(())
        })?;
        notify(BrowserState::CallbackReceived, None);
        let session =
            self.client
                .exchange_browser(&client, &secret, attempt, callback, cancel, |phase| {
                    notify(phase, None);
                })?;
        self.save_session(&client, AuthMethod::Browser, reconnect, session, cancel)
    }

    fn authorize(&mut self, account: AccountId, cancel: &Cancellation) -> Result<(), Notice> {
        let saved = self.sessions.get_mut(&account).ok_or(Notice::Reconnect)?;
        if saved.state == Connection::Reconnect {
            return Err(Notice::Reconnect);
        }
        if saved.session.needs_refresh() {
            let client = match account.method {
                AuthMethod::Device => self.client_id.as_ref(),
                AuthMethod::Browser => self.browser_client.as_ref(),
            }
            .ok_or(Notice::InvalidClient)?;
            let refreshed = match account.method {
                AuthMethod::Device => self.client.refresh(client, &saved.session, cancel),
                AuthMethod::Browser => {
                    let secret = self
                        .vault
                        .oauth_client_secret(client.as_str())
                        .map_err(|_| Notice::OAuthConfiguration)?;
                    self.client
                        .refresh_browser(client, &secret, &saved.session, cancel)
                }
            };
            match refreshed {
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
                    .store(
                        client.as_str(),
                        account.method,
                        account.number,
                        &saved.session.for_vault()?,
                    )
                    .is_err()
            {
                saved.state = Connection::Temporary;
                self.view.notice = Some(Notice::TemporarySession);
            }
        }
        saved.session.check_access()?;
        Ok(())
    }

    fn disconnect(&mut self, account: AccountId) -> Result<(), Notice> {
        let entry = self.sessions.get(&account).ok_or(Notice::Reconnect)?;
        let client = match account.method {
            AuthMethod::Device => &self.client_id,
            AuthMethod::Browser => &self.browser_client,
        };
        if entry.has_vault
            && let Some(client) = client
        {
            self.vault
                .remove(client.as_str(), account.method, account.number)
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
        events: &Sender<WorkerEvent>,
        inbox: Option<&crate::browser_oauth::Inbox>,
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
                let auth = scoped(endpoint, account.method, saved.session.access());
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
            Action::Restore { device, browser } => {
                let browser = self.restore(&browser, AuthMethod::Browser);
                let device = self.restore(&device, AuthMethod::Device);
                browser.and(device)?;
            }
            Action::BrowserSignIn { reconnect } => {
                self.browser_login(reconnect, cancel, events, inbox)?;
            }
            Action::SignIn {
                client_id,
                reconnect,
            } => {
                let id = ClientId::new(&client_id)?;
                let session = self.client.login(&id, cancel, |prompt| {
                    let _ = events.send(WorkerEvent::GithubCode(prompt));
                })?;
                self.save_session(&id, AuthMethod::Device, reconnect, session, cancel)?;
            }
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
        let device = account.method == AuthMethod::Device;
        let profile = entry.session.profile();
        let client = &self.client;
        match action {
            Action::Refresh => {
                let current = client.profile(token, cancel)?;
                if current.id != account.number {
                    return Err(Notice::Reconnect);
                }
                let mut organizations = Vec::new();
                for page in (1..=10).filter(|_| device) {
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
                self.view.avatar = load_avatar(client, account.number, cancel);
            }
            Action::Repositories { installation, page } => {
                if installation.is_some_and(|id| {
                    !device
                        || !self
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
                    scoped(url.clone(), account.method, token)
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
                if device
                    && organization
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
                    return self.publish(token, device, &created.key, publish, cancel);
                }
            }
            Action::ResumePublish {
                repository,
                publish,
            } => return self.publish(token, device, &repository, publish, cancel),
            Action::Fork {
                repository,
                organization,
                name,
            } => {
                if device
                    && organization.as_ref().is_some_and(|owner| {
                        !self
                            .view
                            .organizations
                            .iter()
                            .any(|org| org.login.eq_ignore_ascii_case(owner))
                    })
                {
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
                        auth: scoped(endpoint.clone(), account.method, token),
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
        device: bool,
        repo: &RepositoryKey,
        local: gitrust_core::github::Publish,
        cancel: &Cancellation,
    ) -> Result<Effect, Notice> {
        let confirmed = self
            .client
            .repository(token, repo, cancel)
            .map_err(|error| {
                if device && matches!(error, Notice::Forbidden | Notice::NoAccess) {
                    Notice::AwaitingInstallation
                } else {
                    error
                }
            })?;
        if !confirmed.can_push {
            return Err(if device {
                Notice::AwaitingInstallation
            } else {
                Notice::Forbidden
            });
        }
        let url = confirmed.key.https();
        Ok(Effect::Mutate {
            workspace: Some(local.workspace),
            action: RemoteAction::Publish {
                remote: local.remote,
                branch: local.branch,
                auth: scoped(
                    url.clone(),
                    if device {
                        AuthMethod::Device
                    } else {
                        AuthMethod::Browser
                    },
                    token,
                ),
                url,
            },
        })
    }
}

fn scoped(endpoint: String, method: AuthMethod, token: &str) -> Auth {
    Auth {
        credential: Credential::ScopedToken {
            method,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_rejects_a_different_account_or_method_before_persistence() {
        let mut worker = Worker {
            client: Client::default(),
            vault: CredentialVault,
            client_id: None,
            browser_client: None,
            sessions: BTreeMap::new(),
            view: View::default(),
        };
        let client = ClientId::new("fixtureOAuthClient").expect("public fixture");
        let token = br#"{"account":42,"login":"fixture","access":"fixture-access","browser_client":"fixtureOAuthClient","scopes":["repo","read:user"]}"#;
        for expected in [
            AccountId {
                method: AuthMethod::Browser,
                number: 9,
            },
            AccountId {
                method: AuthMethod::Device,
                number: 42,
            },
        ] {
            let session = Session::from_browser_vault(42, &client, token).expect("test account");
            assert_eq!(
                worker.save_session(
                    &client,
                    AuthMethod::Browser,
                    Some(expected),
                    session,
                    &Cancellation::default(),
                ),
                Err(Notice::WrongAccount)
            );
            assert!(worker.sessions.is_empty());
            assert!(worker.view.active.is_none());
        }
    }
}
