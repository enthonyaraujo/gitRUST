use std::sync::Arc;

use gitrust_core::{
    navigation::{
        BranchBase, Details, HistoryFile, HistoryRow, Merge, NavigationCommand, NavigationIntent,
        NavigationNotice, Pagination, Person, Reference, Revision, SignatureKind,
    },
    workspaces::Activity,
};
use gitrust_git::{
    AbortProof, BranchSource, CommitInfo, CommitSignature, GitError, HeadState, HistoryPage,
    Inspection, MergeState, Mutation, MutationAction, NavigationOutcome, NavigationSnapshot,
    ObjectId, RefMutation, RefTip, ReferenceName, RepoPath, Request, Signature, TaskValue,
    TrustedRepository,
};

use super::{Coordinator, Operation};

const PAGE_SIZE: u16 = 80;
const HISTORY_LIMIT: u32 = 8_000;

pub(super) enum Read {
    Summary,
    Page,
    Details,
    Patch,
}

#[derive(Default)]
pub(super) struct State {
    version: u64,
    identity: u64,
    offset: Option<u32>,
    lanes: Vec<Option<String>>,
    tips: Arc<[ObjectId]>,
    abort: Option<Arc<AbortProof>>,
    message: Option<NavigationNotice>,
}

impl Coordinator {
    pub(super) fn clear_navigation(&mut self) {
        self.navigation.identity = self.navigation.identity.wrapping_add(1);
        self.navigation.abort = None;
        self.navigation.offset = None;
        self.navigation.lanes.clear();
        self.navigation.tips = Arc::default();
        self.navigation.message = None;
        self.view.navigation = None;
    }

    pub(super) fn load_navigation(&mut self, trust: TrustedRepository, switched: bool) {
        if switched {
            self.clear_navigation();
        }
        self.read_navigation(trust, Inspection::Navigation, Read::Summary);
    }

    fn read_navigation(&mut self, trust: TrustedRepository, inspection: Inspection, read: Read) {
        self.start(
            Request::Inspect(trust, inspection),
            Operation::NavigationRead(read),
            Activity::Idle,
        );
        if let Some(view) = &mut self.view.navigation {
            view.busy = self.pending.is_some();
        }
    }

    pub(super) fn on_navigation(&mut self, intent: NavigationIntent) {
        if self.view.activity != Activity::Idle || self.pending.is_some() {
            return;
        }
        let Some(view) = self
            .view
            .navigation
            .as_mut()
            .filter(|view| view.version == intent.version)
        else {
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
        let command = match intent.command {
            NavigationCommand::UseAllRefs(all) => {
                view.all_refs = all;
                view.root = anchor(view.head.clone(), &self.navigation.tips, all);
                reset_history(view, &mut self.navigation);
                return;
            }
            NavigationCommand::More => {
                let (Some(root), Some(offset)) = (&view.root, self.navigation.offset) else {
                    return;
                };
                ObjectId::new(root).and_then(|root| {
                    HistoryPage::new(offset, PAGE_SIZE).map(|page| {
                        let request = if view.all_refs {
                            Inspection::RelatedHistory(root, self.navigation.tips.clone(), page)
                        } else {
                            Inspection::AnchoredHistory(root, page)
                        };
                        (request, Read::Page)
                    })
                })
            }
            NavigationCommand::Details(id, parent) => {
                view.patch = None;
                pair(&id, parent.as_deref())
                    .map(|(id, parent)| (Inspection::CommitDetails(id, parent), Read::Details))
            }
            NavigationCommand::Patch(id, parent, path) => {
                view.patch = None;
                pair(&id, parent.as_deref()).and_then(|(id, parent)| {
                    RepoPath::new(path)
                        .map(|path| (Inspection::CommitPatch(id, parent, path), Read::Patch))
                })
            }
            write => {
                self.navigate_write(trust, write);
                return;
            }
        };
        match command {
            Ok((inspection, read)) => self.read_navigation(trust, inspection, read),
            Err(error) => view.notice = Some(failure(&error)),
        }
    }

    fn navigate_write(&mut self, trust: TrustedRepository, write: NavigationCommand) {
        let Some(status) = self.changes.status.clone() else {
            return;
        };
        let command = match write {
            NavigationCommand::Create(name, base) => {
                source(base).map(|base| RefMutation::Create(name, base))
            }
            NavigationCommand::Switch(branch) => tip(&branch).map(RefMutation::Switch),
            NavigationCommand::Rename(branch, name) => {
                tip(&branch).map(|branch| RefMutation::Rename(branch, name))
            }
            NavigationCommand::Delete(branch) => tip(&branch).map(RefMutation::Delete),
            NavigationCommand::Merge(branch, author) => {
                tip(&branch).map(|branch| RefMutation::Merge(branch, author))
            }
            NavigationCommand::Finish(source, draft) => {
                ObjectId::new(&source).map(|source| RefMutation::Finish(source, draft))
            }
            NavigationCommand::Abort => self
                .navigation
                .abort
                .clone()
                .map(RefMutation::Abort)
                .ok_or(GitError::StaleSnapshot),
            _ => return,
        };
        let command = match command {
            Ok(command) => command,
            Err(error) => {
                if let Some(view) = &mut self.view.navigation {
                    view.notice = Some(failure(&error));
                }
                return;
            }
        };
        let request = Request::Mutate(
            trust.clone(),
            Box::new(Mutation {
                expected: status,
                action: MutationAction::Navigate(Box::new(command)),
            }),
        );
        self.start(request, Operation::NavigationWrite(trust), Activity::Idle);
        if let Some(view) = &mut self.view.navigation {
            view.busy = self.pending.is_some();
            view.notice = None;
        }
    }

    pub(super) fn complete_navigation(
        &mut self,
        operation: Operation,
        outcome: Result<TaskValue, GitError>,
    ) {
        if let Operation::NavigationWrite(trust) = operation {
            self.navigation.message = Some(match outcome {
                Ok(TaskValue::NavigationResult(result)) => {
                    self.navigation.abort = result.abort;
                    notice(result.outcome)
                }
                Err(ref error) => failure(error),
                _ => NavigationNotice::Failed,
            });
            self.debounce.0 = None;
            self.inspect(trust, true);
            return;
        }
        self.view.activity = Activity::Idle;
        if let Ok(TaskValue::Navigation(summary)) = outcome {
            self.update_summary(summary);
            return;
        }
        let Some(view) = &mut self.view.navigation else {
            return;
        };
        view.busy = false;
        match outcome {
            Ok(TaskValue::History(page))
                if page.root.as_ref().map(ObjectId::as_str) == view.root.as_deref() =>
            {
                let rows = Arc::make_mut(&mut view.history);
                rows.extend(
                    page.commits
                        .into_iter()
                        .map(|commit| graph_row(&mut self.navigation.lanes, &commit)),
                );
                self.navigation.offset = page.next_offset.filter(|&offset| offset < HISTORY_LIMIT);
                view.pagination = match (self.navigation.offset, page.next_offset) {
                    (Some(_), _) => Pagination::More,
                    (None, Some(_)) => Pagination::Limit,
                    (None, None) => Pagination::End,
                };
            }
            Ok(TaskValue::CommitDetails(detail)) => {
                view.details = Some(Arc::new(Details {
                    revision: revision(&detail.commit),
                    committer: person(&detail.commit.committer),
                    message: String::from_utf8_lossy(&detail.commit.message).into_owned(),
                    signature: detail.commit.signing.map(|signature| match signature {
                        CommitSignature::OpenPgp => SignatureKind::OpenPgp,
                        CommitSignature::Ssh => SignatureKind::Ssh,
                        CommitSignature::Other => SignatureKind::Other,
                    }),
                    parent: detail.parent.map(|id| id.as_str().to_owned()),
                    files: detail
                        .files
                        .into_iter()
                        .map(|file| HistoryFile {
                            path: file.path,
                            previous: file.previous,
                            lines: file.lines,
                        })
                        .collect(),
                }));
            }
            Ok(TaskValue::Change(patch)) => view.patch = Some(Arc::new(patch.display())),
            Err(ref error) => {
                view.notice = Some(failure(error));
                if matches!(operation, Operation::NavigationRead(Read::Page)) {
                    view.pagination = Pagination::End;
                }
            }
            _ => view.notice = Some(NavigationNotice::Failed),
        }
    }

    fn update_summary(&mut self, summary: NavigationSnapshot) {
        self.navigation.version = self.navigation.version.wrapping_add(1);
        let head = match &summary.refs.head {
            HeadState::Unborn(_) => None,
            HeadState::Attached(_, id) | HeadState::Detached(id) => Some(id.as_str().to_owned()),
        };
        let tips: Arc<[_]> = summary
            .refs
            .branches
            .iter()
            .filter(|branch| !branch.symbolic)
            .map(|branch| branch.id.clone())
            .collect();
        let mut view = self.view.navigation.take().unwrap_or_default();
        let root = anchor(head.clone(), &tips, view.all_refs);
        view.head = head;
        if view.root != root || (view.all_refs && tips != self.navigation.tips) {
            view.root = root;
            reset_history(&mut view, &mut self.navigation);
        }
        self.navigation.tips = tips;
        view.version = self.navigation.version;
        view.identity = self.navigation.identity;
        view.busy = false;
        view.refs = summary
            .refs
            .branches
            .into_iter()
            .filter(|branch| !branch.symbolic)
            .map(|branch| Reference {
                label: String::from_utf8_lossy(
                    branch
                        .name
                        .as_bytes()
                        .strip_prefix(b"refs/heads/")
                        .or_else(|| branch.name.as_bytes().strip_prefix(b"refs/remotes/"))
                        .unwrap_or(branch.name.as_bytes()),
                )
                .into_owned(),
                name: Arc::from(branch.name.as_bytes()),
                commit: branch.id.as_str().to_owned(),
                upstream: branch.upstream.map(|name| {
                    String::from_utf8_lossy(
                        name.as_bytes()
                            .strip_prefix(b"refs/remotes/")
                            .unwrap_or(name.as_bytes()),
                    )
                    .into_owned()
                }),
                divergence: branch.divergence,
                remote: branch.remote_tracking,
                current: branch.current,
                occupied: branch.occupied,
            })
            .collect();
        view.author = summary.author;
        view.merge = match summary.merge {
            MergeState::Active(source, unresolved) => Merge::Active {
                source: source.as_str().to_owned(),
                unresolved,
                can_abort: self.navigation.abort.is_some(),
            },
            MergeState::Other => Merge::Incompatible,
            MergeState::None => {
                self.navigation.abort = None;
                Merge::None
            }
        };
        view.notice = self.navigation.message.take().or(view.notice);
        self.view.navigation = Some(view);
    }
}

fn anchor(head: Option<String>, tips: &[ObjectId], all: bool) -> Option<String> {
    head.or_else(|| {
        all.then(|| tips.first().map(|tip| tip.as_str().to_owned()))
            .flatten()
    })
}

fn reset_history(view: &mut gitrust_core::navigation::NavigationView, state: &mut State) {
    view.history = Arc::default();
    state.offset = view.root.as_ref().map(|_| 0);
    view.pagination = if state.offset.is_some() {
        Pagination::Initial
    } else {
        Pagination::End
    };
    view.details = None;
    view.patch = None;
    state.lanes.clear();
}

fn pair(id: &str, parent: Option<&str>) -> Result<(ObjectId, Option<ObjectId>), GitError> {
    Ok((ObjectId::new(id)?, parent.map(ObjectId::new).transpose()?))
}

fn tip(reference: &Reference) -> Result<RefTip, GitError> {
    Ok(RefTip {
        name: ReferenceName::new(&reference.name)?,
        id: ObjectId::new(&reference.commit)?,
    })
}

fn source(base: BranchBase) -> Result<BranchSource, GitError> {
    match base {
        BranchBase::Head => Ok(BranchSource::Head),
        BranchBase::Ref(reference) => tip(&reference).map(BranchSource::Reference),
        BranchBase::Commit(id) => ObjectId::new(&id).map(BranchSource::Commit),
    }
}

fn revision(commit: &CommitInfo) -> Revision {
    Revision {
        id: commit.id.as_str().to_owned(),
        subject: String::from_utf8_lossy(&commit.subject).into_owned(),
        author: person(&commit.author),
        parents: commit
            .parents
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
    }
}

fn person(value: &Signature) -> Person {
    let seconds = value
        .unix_seconds
        .saturating_add(i64::from(value.offset_minutes) * 60);
    let day = seconds.div_euclid(86400);
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let day_of_year = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_position = (5 * day_of_year + 2) / 153;
    let date = day_of_year - (153 * month_position + 2) / 5 + 1;
    let month = if month_position < 10 {
        month_position + 3
    } else {
        month_position - 9
    };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let hour = seconds.rem_euclid(86400) / 3600;
    let minute = seconds.rem_euclid(3600) / 60;
    let zone = value.offset_minutes.unsigned_abs();
    let sign = if value.offset_minutes < 0 { '-' } else { '+' };
    Person {
        name: String::from_utf8_lossy(&value.name).into_owned(),
        email: String::from_utf8_lossy(&value.email).into_owned(),
        time: format!(
            "{year:04}-{month:02}-{date:02} {hour:02}:{minute:02} {sign}{:02}:{:02}",
            zone / 60,
            zone % 60
        ),
    }
}

fn graph_row(lanes: &mut Vec<Option<String>>, commit: &CommitInfo) -> HistoryRow {
    let revision = revision(commit);
    let find = |lanes: &[Option<String>], id: &str| {
        lanes.iter().position(|slot| slot.as_deref() == Some(id))
    };
    let node = find(lanes, &revision.id).unwrap_or_else(|| empty_lane(lanes));
    lanes[node] = None;
    let mut edges: Vec<_> = lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| lane.is_some())
        .map(|(i, _)| (i, i))
        .collect();
    for (index, parent) in revision.parents.iter().enumerate() {
        let lane = find(lanes, parent)
            .unwrap_or_else(|| if index == 0 { node } else { empty_lane(lanes) });
        lanes[lane] = Some(parent.clone());
        edges.push((node, lane));
    }
    while lanes.last().is_some_and(Option::is_none) {
        lanes.pop();
    }
    HistoryRow {
        revision,
        node,
        edges,
    }
}

fn empty_lane(lanes: &mut Vec<Option<String>>) -> usize {
    if let Some(lane) = lanes.iter().position(Option::is_none) {
        lane
    } else {
        lanes.push(None);
        lanes.len() - 1
    }
}

fn notice(outcome: NavigationOutcome) -> NavigationNotice {
    match outcome {
        NavigationOutcome::BranchUpdated => NavigationNotice::BranchUpdated,
        NavigationOutcome::Integrated => NavigationNotice::Integrated,
        NavigationOutcome::FastForward => NavigationNotice::FastForward,
        NavigationOutcome::MergePending => NavigationNotice::MergePending,
        NavigationOutcome::MergeCommitted => NavigationNotice::MergeCommitted,
        NavigationOutcome::MergeAborted => NavigationNotice::MergeAborted,
    }
}

fn failure(error: &GitError) -> NavigationNotice {
    match error {
        GitError::StaleSnapshot => NavigationNotice::Stale,
        GitError::InvalidReference | GitError::InvalidArgument(_) => NavigationNotice::InvalidRef,
        GitError::BranchOccupied => NavigationNotice::Occupied,
        GitError::BranchUnmerged => NavigationNotice::Unmerged,
        GitError::BranchSwitchBlocked => NavigationNotice::SwitchBlocked,
        GitError::MergeDirty => NavigationNotice::Dirty,
        GitError::AuthorRequired => NavigationNotice::AuthorRequired,
        GitError::SigningFailed => NavigationNotice::SigningFailed,
        GitError::UnsupportedRepositoryFeature(_) => NavigationNotice::Incompatible,
        GitError::Cancelled => NavigationNotice::Cancelled,
        _ => NavigationNotice::Failed,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    fn commit(label: &str, parents: &[&str]) -> CommitInfo {
        let identity = Signature {
            name: b"Fixture".to_vec(),
            email: b"fixture@example.invalid".to_vec(),
            unix_seconds: 0,
            offset_minutes: -180,
        };
        let object = |label: &str| ObjectId::new(&label.repeat(40)).expect("fixture object");
        CommitInfo {
            id: object(label),
            parents: parents.iter().map(|label| object(label)).collect(),
            subject: b"Commit".to_vec(),
            message: b"Commit\n".to_vec(),
            author: identity.clone(),
            committer: identity,
            signing: None,
        }
    }

    #[test]
    fn graph_keeps_lanes_across_pages_until_real_parents_converge() {
        let mut lanes = Vec::new();
        let merge = graph_row(&mut lanes, &commit("f", &["e", "d"]));
        assert_eq!(merge.edges, [(0, 0), (0, 1)]);
        let first = graph_row(&mut lanes, &commit("e", &["a"]));
        assert_eq!(first.node, 0);
        assert_eq!(first.edges, [(1, 1), (0, 0)]);
        let first_on_next_page = graph_row(&mut lanes, &commit("d", &["a"]));
        assert_eq!(first_on_next_page.node, 1);
        assert_eq!(first_on_next_page.edges, [(0, 0), (1, 0)]);
        let root = graph_row(&mut lanes, &commit("a", &[]));
        assert_eq!(root.node, 0);
        assert_eq!(root.edges, []);
        assert_eq!(lanes, []);
        assert_eq!(root.revision.author.time, "1969-12-31 21:00 -03:00");
    }
}
