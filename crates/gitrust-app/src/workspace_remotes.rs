use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
    time::{SystemTime, UNIX_EPOCH},
};

use gitrust_core::{
    navigation::Merge,
    remotes::{Credential, RebaseState, RemoteAction, RemoteIntent, RemoteNotice, RemotesView},
    workspaces::Activity,
};
use gitrust_git::{
    GitError, RebaseProof, RemoteRequest, RemoteSnapshot, Request, TaskValue, TrustedRepository,
};

use super::{Coordinator, Operation};

#[derive(Default)]
pub(super) struct State {
    version: u64,
    snapshot: Option<RemoteSnapshot>,
    message: Option<RemoteNotice>,
    last_fetch: BTreeMap<String, u64>,
    rebase: Option<Arc<RebaseProof>>,
}

impl Coordinator {
    pub(super) fn clear_remotes(&mut self) {
        self.view.remotes = None;
        self.remotes = State {
            version: self.remotes.version.wrapping_add(1),
            ..State::default()
        };
        self.sync_github_workspace();
    }

    pub(super) fn update_remotes(&mut self, snapshot: RemoteSnapshot) {
        self.remotes.version = self.remotes.version.wrapping_add(1);
        if !snapshot.rebase {
            self.remotes.rebase = None;
            if self.remotes.message == Some(RemoteNotice::RebasePending) {
                self.remotes.message = None;
            }
        }
        if self.remotes.message == Some(RemoteNotice::MergePending)
            && self
                .view
                .navigation
                .as_ref()
                .is_some_and(|view| matches!(view.merge, Merge::None))
        {
            self.remotes.message = None;
        }
        self.remotes
            .last_fetch
            .retain(|name, _| snapshot.entries.iter().any(|entry| entry.name == *name));
        let view = RemotesView {
            version: self.remotes.version,
            entries: Arc::from(snapshot.entries.clone()),
            upstream: snapshot.upstream.clone(),
            current: snapshot.current.clone(),
            ahead_behind: snapshot.divergence,
            worktree_dirty: snapshot.worktree_dirty,
            preference: snapshot.preference,
            last_fetch: self
                .remotes
                .last_fetch
                .iter()
                .map(|(name, time)| (name.clone(), *time))
                .collect(),
            busy: false,
            notice: self.remotes.message,
            rebase: if self.remotes.rebase.is_some() {
                RebaseState::Recoverable
            } else if snapshot.rebase {
                RebaseState::Unverified
            } else {
                RebaseState::None
            },
        };
        self.remotes.snapshot = Some(snapshot);
        self.view.remotes = Some(view);
    }

    pub(super) fn on_remote(&mut self, intent: RemoteIntent) {
        if self.pending.is_some()
            || self.view.activity != Activity::Idle
            || self
                .view
                .remotes
                .as_ref()
                .is_none_or(|view| view.version != intent.version)
        {
            return;
        }
        let Some(trust) = self
            .view
            .active
            .as_ref()
            .and_then(|workspace| self.trusted.get(&workspace.path))
            .cloned()
        else {
            return;
        };
        let Some(expected) = self.remotes.snapshot.clone() else {
            return;
        };
        let linked = match &intent.action {
            RemoteAction::Fetch(names, auth)
                if matches!(auth.credential, Credential::LinkedAccount) =>
            {
                Some((names.first().filter(|_| names.len() == 1), false))
            }
            RemoteAction::Pull(name, _, auth)
                if matches!(auth.credential, Credential::LinkedAccount) =>
            {
                Some((Some(name), false))
            }
            RemoteAction::Push { remote, auth, .. }
                if matches!(auth.credential, Credential::LinkedAccount) =>
            {
                Some((Some(remote), true))
            }
            _ => None,
        };
        if let Some((remote, push)) = linked {
            let endpoint = remote
                .and_then(|name| expected.entries.iter().find(|remote| &remote.name == name))
                .and_then(|remote| {
                    if push {
                        remote.push.clone()
                    } else {
                        remote.fetch.clone()
                    }
                });
            if let (Some(account), Some(endpoint), Some(workspace)) = (
                self.view.github.workspace_account,
                endpoint
                    .filter(|url| gitrust_core::github::RepositoryKey::from_https(url).is_some()),
                self.view.github.workspace.clone(),
            ) {
                self.submit_github(crate::github_actor::Job::Authorize {
                    account,
                    endpoint,
                    workspace,
                    intent,
                });
            } else if let Some(view) = &mut self.view.remotes {
                view.notice = Some(RemoteNotice::Authentication);
            }
            return;
        }
        let (send, receive) = mpsc::sync_channel(2);
        let request = RemoteRequest {
            expected,
            action: intent.action,
            status: self.changes.status.clone(),
            bridge: std::env::current_exe().ok(),
            progress: Some(send),
            rebase: self.remotes.rebase.clone(),
        };
        self.remotes.message = None;
        self.start(
            Request::Remote(trust.clone(), Box::new(request)),
            Operation::RemoteWrite(trust),
            Activity::Idle,
        );
        if let Some(view) = &mut self.view.remotes {
            view.busy = self.pending.is_some();
            view.notice = None;
        }
        if self.pending.is_some() {
            self.transfer_progress(receive);
        }
    }

    pub(super) fn complete_remote(
        &mut self,
        trust: TrustedRepository,
        outcome: Result<TaskValue, GitError>,
    ) {
        self.remotes.message = Some(match outcome {
            Ok(TaskValue::RemoteResult(result)) => {
                if let Some(proof) = result.merge {
                    self.navigation.abort = Some(proof);
                }
                if let Some(proof) = result.rebase {
                    self.remotes.rebase = Some(proof);
                }
                let time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |time| time.as_secs());
                for name in result.fetched {
                    self.remotes.last_fetch.insert(name, time);
                }
                result.notice
            }
            Err(GitError::Remote(notice)) => notice,
            Err(GitError::Cancelled) => RemoteNotice::Cancelled,
            Err(GitError::Timeout) => RemoteNotice::Timeout,
            Err(GitError::StaleSnapshot) => RemoteNotice::Stale,
            Err(GitError::MergeDirty) => RemoteNotice::Dirty,
            Err(
                GitError::InvalidArgument(_)
                | GitError::InvalidReference
                | GitError::DestinationExists,
            ) => RemoteNotice::Invalid,
            _ => RemoteNotice::Failed,
        });
        self.complete_github_write(self.remotes.message);
        self.debounce.0 = None;
        self.inspect(trust, true);
    }
}
