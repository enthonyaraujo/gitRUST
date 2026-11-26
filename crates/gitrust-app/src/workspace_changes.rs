use std::sync::Arc;

use gitrust_core::{
    changes::{ChangesCommand, ChangesIntent, ChangesNotice, ChangesView, DiscardReview},
    workspaces::Activity,
};
use gitrust_git::{
    ChangePatch, GitError, Inspection, Mutation, MutationAction, ObjectId, RepositoryStatus,
    Request, TaskValue, change_rows,
};

use super::{Coordinator, Operation};

#[derive(Default)]
pub(super) struct ChangesState {
    identity: u64,
    version: u64,
    amend_head: Option<ObjectId>,
    review_id: u64,
    status: Option<Arc<RepositoryStatus>>,
    patch: Option<Arc<ChangePatch>>,
    review: Option<(u64, Vec<ChangePatch>)>,
    result: Option<ChangesNotice>,
}

impl Coordinator {
    pub(super) fn set_changes(&mut self, status: Option<RepositoryStatus>, switched: bool) {
        if switched {
            self.changes.result = None;
            self.changes.amend_head = None;
            self.changes.identity = self.changes.identity.wrapping_add(1);
        }
        self.changes.version = self.changes.version.wrapping_add(1);
        self.changes.patch = None;
        self.changes.review = None;
        let shared_rows = if !switched && self.changes.status.as_deref() == status.as_ref() {
            self.view.changes.as_ref().map(|view| view.rows.clone())
        } else {
            None
        };
        self.changes.status = status.map(Arc::new);
        let notice = self.changes.result.take().or_else(|| {
            self.view
                .changes
                .as_ref()
                .filter(|_| !switched)
                .and_then(|view| view.notice)
        });
        self.view.changes = self.changes.status.as_ref().and_then(|status| {
            let rows = shared_rows.or_else(|| change_rows(status).ok().map(Into::into))?;
            Some(ChangesView {
                identity: self.changes.identity,
                version: self.changes.version,
                rows,
                notice,
                ..ChangesView::default()
            })
        });
    }

    pub(super) fn on_changes(&mut self, intent: ChangesIntent) {
        if self.view.activity != Activity::Idle || self.pending.is_some() {
            return;
        }
        let Some(view) = self
            .view
            .changes
            .as_mut()
            .filter(|view| view.version == intent.version)
        else {
            return;
        };
        let Some(status) = self.changes.status.clone() else {
            return;
        };
        let Some(trust) = self
            .view
            .active
            .as_ref()
            .and_then(|active| self.trusted.get(&active.path))
            .cloned()
        else {
            return;
        };
        if !matches!(intent.command, ChangesCommand::Preview(_)) {
            view.notice = None;
        }
        let write = match intent.command {
            ChangesCommand::Preview(key) => {
                self.changes.patch = None;
                view.preview = None;
                return self.read_changes(Request::Inspect(trust, Inspection::Change(key)));
            }
            ChangesCommand::ReviewDiscard(keys) => {
                return self.read_changes(Request::Inspect(trust, Inspection::DiscardReview(keys)));
            }
            ChangesCommand::DismissDiscard => {
                self.changes.review = None;
                view.discard = None;
                return;
            }
            ChangesCommand::LoadAuthor | ChangesCommand::LoadAmend => {
                let amend = matches!(intent.command, ChangesCommand::LoadAmend);
                return self
                    .read_changes(Request::Inspect(trust, Inspection::CommitDraft { amend }));
            }
            ChangesCommand::Stage(keys) => MutationAction::Stage(keys),
            ChangesCommand::Unstage(keys) => MutationAction::Unstage(keys),
            ChangesCommand::Partial(key, selection) => {
                let Some(patch) = self
                    .changes
                    .patch
                    .as_ref()
                    .filter(|patch| *patch.key() == key)
                else {
                    view.notice = Some(ChangesNotice::Stale);
                    return;
                };
                MutationAction::Partial {
                    patch: patch.clone(),
                    selection,
                }
            }
            ChangesCommand::Discard(id) => {
                let Some((_, patches)) =
                    self.changes.review.take().filter(|(shown, _)| *shown == id)
                else {
                    view.notice = Some(ChangesNotice::Stale);
                    return;
                };
                view.discard = None;
                MutationAction::Discard(patches)
            }
            ChangesCommand::Commit(draft) => {
                if draft.amend
                    && (self.changes.amend_head.is_none()
                        || self.changes.amend_head != status.branch.oid)
                {
                    view.notice = Some(ChangesNotice::Stale);
                    return;
                }
                MutationAction::Commit(draft)
            }
        };
        let request = Request::Mutate(
            trust.clone(),
            Box::new(Mutation {
                expected: status,
                action: write,
            }),
        );
        self.start(request, Operation::ChangesWrite(trust), Activity::Idle);
        self.mark_changes_busy();
    }

    fn read_changes(&mut self, request: Request) {
        self.start(request, Operation::ChangesRead, Activity::Idle);
        self.mark_changes_busy();
    }

    fn mark_changes_busy(&mut self) {
        if let Some(view) = &mut self.view.changes {
            view.busy = self.pending.is_some();
        }
    }

    pub(super) fn complete_changes(
        &mut self,
        operation: Operation,
        result: Result<TaskValue, GitError>,
    ) {
        if let Operation::ChangesWrite(trust) = operation {
            self.changes.result = Some(match result {
                Ok(TaskValue::Changed(Some(_))) => ChangesNotice::Committed,
                Ok(TaskValue::Changed(None)) => ChangesNotice::Updated,
                Err(ref error) => failure(error),
                Ok(_) => ChangesNotice::Failed,
            });
            self.debounce.0 = None;
            self.inspect(trust, true);
            return;
        }
        self.view.activity = Activity::Idle;
        let Some(view) = &mut self.view.changes else {
            return;
        };
        view.busy = false;
        match result {
            Ok(TaskValue::Change(patch)) => {
                view.preview = Some(Arc::new(patch.display()));
                self.changes.patch = Some(Arc::new(patch));
            }
            Ok(TaskValue::DiscardReview(patches)) => {
                self.changes.review_id = self.changes.review_id.wrapping_add(1);
                let id = self.changes.review_id;
                view.discard = Some(Arc::new(DiscardReview {
                    id,
                    files: patches.iter().map(ChangePatch::display).collect(),
                }));
                self.changes.review = Some((id, patches));
            }
            Ok(TaskValue::CommitDraft(draft, head)) => {
                if draft.amend {
                    self.changes.amend_head = head;
                }
                view.draft = Some(Arc::new(draft));
            }
            Err(error) => view.notice = Some(failure(&error)),
            Ok(_) => view.notice = Some(ChangesNotice::Failed),
        }
    }
}

fn failure(error: &GitError) -> ChangesNotice {
    match error {
        GitError::StaleSnapshot | GitError::RepositoryNotTrusted => ChangesNotice::Stale,
        GitError::AuthorRequired => ChangesNotice::AuthorRequired,
        GitError::SigningFailed => ChangesNotice::SigningFailed,
        GitError::UnsupportedRepositoryFeature(_) | GitError::InvalidArgument(_) => {
            ChangesNotice::Unsupported
        }
        _ => ChangesNotice::Failed,
    }
}
