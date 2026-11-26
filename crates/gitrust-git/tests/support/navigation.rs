use super::*;
use gitrust_core::changes::{ChangeKey, ChangeSide, CommitAuthor, CommitDraft};
use gitrust_git::{
    AbortProof, BranchList, BranchSource, MergeState, Mutation, MutationAction, NavigationOutcome,
    RefMutation, RefTip,
};
use std::sync::Arc;

fn branch_list(engine: &GitEngine, repo: &TrustedRepository) -> BranchList {
    let TaskValue::Branches(list) = inspect(engine, repo, Inspection::Branches) else {
        panic!("branch list required");
    };
    list
}

fn branch(engine: &GitEngine, repo: &TrustedRepository, name: &str) -> RefTip {
    let reference = branch_list(engine, repo)
        .branches
        .into_iter()
        .find(|branch| branch.name.as_bytes() == name.as_bytes())
        .expect("branch exists");
    RefTip {
        name: reference.name,
        id: reference.id,
    }
}

fn run(
    engine: &GitEngine,
    repo: &TrustedRepository,
    command: RefMutation,
) -> Result<TaskValue, GitError> {
    let expected = Arc::new(status(engine, repo, false));
    mutate(engine, repo, command, expected)
}

fn mutate(
    engine: &GitEngine,
    repo: &TrustedRepository,
    command: RefMutation,
    expected: Arc<RepositoryStatus>,
) -> Result<TaskValue, GitError> {
    submit(
        engine,
        Request::Mutate(
            repo.clone(),
            Box::new(Mutation {
                expected,
                action: MutationAction::Navigate(Box::new(command)),
            }),
        ),
    )
    .result
}

fn author() -> CommitAuthor {
    CommitAuthor {
        name: "Stage Five".into(),
        email: "five@example.invalid".into(),
    }
}

fn outcome(
    value: Result<TaskValue, GitError>,
    expected: NavigationOutcome,
) -> Option<Arc<AbortProof>> {
    let TaskValue::NavigationResult(result) = value.expect("navigation operation") else {
        panic!("expected outcome");
    };
    assert_eq!(result.outcome, expected);
    result.abort
}

#[test]
fn branch_lifecycle_validates_names_and_preserves_local_work() {
    let fixture = Fixture::new(&[]);
    fixture.write("tracked", "base");
    let base = fixture.commit("base");
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    for invalid in ["--force", "@{-1}", "a..b", "HEAD", "bad name"] {
        assert!(matches!(
            run(
                &service,
                &repo,
                RefMutation::Create(invalid.into(), BranchSource::Head)
            ),
            Err(GitError::InvalidReference)
        ));
    }
    outcome(
        run(
            &service,
            &repo,
            RefMutation::Create("topic".into(), BranchSource::Commit(base.clone())),
        ),
        NavigationOutcome::BranchUpdated,
    );
    assert!(
        run(
            &service,
            &repo,
            RefMutation::Create("topic".into(), BranchSource::Head)
        )
        .is_err()
    );
    let topic = branch(&service, &repo, "refs/heads/topic");
    assert_eq!(topic.id, base);
    fixture.write("tracked", "staged");
    fixture.git(["add", "--", "tracked"]);
    fixture.write("tracked", "unstaged");
    outcome(
        run(&service, &repo, RefMutation::Switch(topic)),
        NavigationOutcome::BranchUpdated,
    );
    assert_eq!(fixture.git(["show", ":tracked"]).stdout, b"staged");
    assert_eq!(
        fs::read(fixture.path().join("tracked")).expect("worktree"),
        b"unstaged"
    );
    let topic = branch(&service, &repo, "refs/heads/topic");
    assert!(matches!(
        run(&service, &repo, RefMutation::Delete(topic.clone())),
        Err(GitError::BranchOccupied)
    ));
    outcome(
        run(
            &service,
            &repo,
            RefMutation::Rename(topic, "renamed".into()),
        ),
        NavigationOutcome::BranchUpdated,
    );
    assert_eq!(
        fixture.git(["symbolic-ref", "--short", "HEAD"]).stdout,
        b"renamed\n"
    );
    outcome(
        run(
            &service,
            &repo,
            RefMutation::Switch(branch(&service, &repo, "refs/heads/main")),
        ),
        NavigationOutcome::BranchUpdated,
    );
    outcome(
        run(
            &service,
            &repo,
            RefMutation::Delete(branch(&service, &repo, "refs/heads/renamed")),
        ),
        NavigationOutcome::BranchUpdated,
    );
}

#[test]
fn unsafe_checkout_and_linked_worktree_mutations_are_blocked() {
    let fixture = Fixture::new(&[]);
    fixture.write("tracked", "base");
    fixture.commit("base");
    fixture.git(["checkout", "-b", "topic"]);
    fixture.write("tracked", "different");
    fixture.commit("topic");
    fixture.git(["checkout", "main"]);
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    fixture.write("tracked", "local");
    let topic = branch(&service, &repo, "refs/heads/topic");
    assert!(matches!(
        run(&service, &repo, RefMutation::Switch(topic.clone())),
        Err(GitError::BranchSwitchBlocked)
    ));
    assert!(matches!(
        run(&service, &repo, RefMutation::Delete(topic.clone())),
        Err(GitError::BranchUnmerged)
    ));
    assert_eq!(
        fs::read(fixture.path().join("tracked")).expect("worktree"),
        b"local"
    );
    let sibling = tempfile::tempdir().expect("linked worktree parent");
    let path = sibling.path().join("tree");
    fixture.git([
        OsStr::new("worktree"),
        OsStr::new("add"),
        path.as_os_str(),
        OsStr::new("topic"),
    ]);
    assert!(matches!(
        run(&service, &repo, RefMutation::Delete(topic.clone())),
        Err(GitError::BranchOccupied)
    ));
    assert!(matches!(
        run(&service, &repo, RefMutation::Rename(topic, "moved".into())),
        Err(GitError::BranchOccupied)
    ));
}

#[test]
fn tracking_refs_and_anchored_pages_survive_a_head_change() {
    let fixture = Fixture::new(&[]);
    let root = fixture.commit("root");
    let middle = fixture.commit("middle");
    let latest = fixture.commit("latest");
    fixture.git(["remote", "add", "origin", "https://example.invalid/repo"]);
    fixture.git(["update-ref", "refs/remotes/origin/topic", latest.as_str()]);
    fixture.git(["branch", "--set-upstream-to=origin/topic", "main"]);
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let remote = branch(&service, &repo, "refs/remotes/origin/topic");
    outcome(
        run(
            &service,
            &repo,
            RefMutation::Create("tracking".into(), BranchSource::Reference(remote)),
        ),
        NavigationOutcome::BranchUpdated,
    );
    let branches = branch_list(&service, &repo);
    let tracking = branches
        .branches
        .iter()
        .find(|branch| branch.name.as_bytes() == b"refs/heads/tracking")
        .expect("tracking branch");
    assert_eq!(tracking.divergence, Some((0, 0)));
    assert_eq!(
        tracking.upstream.as_ref().map(ReferenceName::as_bytes),
        Some(b"refs/remotes/origin/topic".as_slice())
    );
    fixture.commit("outside snapshot");
    let TaskValue::History(page) = inspect(
        &service,
        &repo,
        Inspection::AnchoredHistory(latest.clone(), HistoryPage::new(1, 2).expect("page")),
    ) else {
        panic!("expected page");
    };
    assert_eq!(page.root, Some(latest));
    assert_eq!(
        page.commits
            .into_iter()
            .map(|commit| commit.id)
            .collect::<Vec<_>>(),
        [middle, root.clone()]
    );
    fixture.git(["checkout", "--detach", root.as_str()]);
    assert!(matches!(
        branch_list(&service, &repo).head,
        HeadState::Detached(_)
    ));
}

#[test]
fn root_comparison_and_unborn_history_are_supported() {
    let fixture = Fixture::new(&[]);
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let TaskValue::History(page) = inspect(
        &service,
        &repo,
        Inspection::History(HistoryPage::new(0, 10).expect("page")),
    ) else {
        panic!("expected page");
    };
    assert!(page.root.is_none());
    fixture.write("example", "one\ntwo\n");
    let root = fixture.commit("root");
    let TaskValue::CommitDetails(details) = inspect(
        &service,
        &repo,
        Inspection::CommitDetails(root.clone(), None),
    ) else {
        panic!("expected details");
    };
    assert!(details.parent.is_none());
    assert_eq!(details.files[0].lines, Some((2, 0)));
    let TaskValue::Change(patch) = inspect(
        &service,
        &repo,
        Inspection::CommitPatch(root, None, RepoPath::new("example".into()).expect("path")),
    ) else {
        panic!("expected patch");
    };
    assert!(patch.display().text.contains("+two"));
}

#[test]
fn fast_forward_three_way_and_parent_comparisons() {
    let fixture = Fixture::new(&[]);
    fixture.commit("base");
    fixture.git(["checkout", "-b", "topic"]);
    fixture.write("topic-file", "topic\n");
    let topic = fixture.commit("topic");
    fixture.git(["checkout", "main"]);
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let topic_ref = branch(&service, &repo, "refs/heads/topic");
    outcome(
        run(
            &service,
            &repo,
            RefMutation::Merge(topic_ref.clone(), author()),
        ),
        NavigationOutcome::FastForward,
    );
    assert_eq!(fixture.oid("HEAD"), topic);
    outcome(
        run(&service, &repo, RefMutation::Merge(topic_ref, author())),
        NavigationOutcome::Integrated,
    );
    fixture.git(["checkout", "-b", "side"]);
    fixture.write("side-file", "side\n");
    let side = fixture.commit("side");
    fixture.git(["checkout", "main"]);
    fixture.write("main-file", "main\n");
    let main = fixture.commit("main");
    let side_ref = branch(&service, &repo, "refs/heads/side");
    outcome(
        run(&service, &repo, RefMutation::Merge(side_ref, author())),
        NavigationOutcome::MergePending,
    );
    let draft = CommitDraft {
        title: "Merge side".into(),
        author: author(),
        ..CommitDraft::default()
    };
    outcome(
        run(&service, &repo, RefMutation::Finish(side.clone(), draft)),
        NavigationOutcome::MergeCommitted,
    );
    let head = fixture.oid("HEAD");
    for (parent, changed) in [(main.clone(), "side-file"), (side.clone(), "main-file")] {
        let TaskValue::CommitDetails(details) = inspect(
            &service,
            &repo,
            Inspection::CommitDetails(head.clone(), Some(parent)),
        ) else {
            panic!("expected details");
        };
        assert_eq!(details.commit.parents, [main.clone(), side.clone()]);
        assert_eq!(details.files.len(), 1);
        assert_eq!(details.files[0].path, Path::new(changed));
    }
}

#[test]
fn conflict_abort_protects_subsequent_bytes_and_resolved_index_can_finish() {
    let fixture = Fixture::new(&[]);
    fixture.write("shared", "base\n");
    fixture.commit("base");
    fixture.git(["checkout", "-b", "side"]);
    fixture.write("shared", "side\n");
    let side = fixture.commit("side");
    fixture.git(["checkout", "main"]);
    fixture.write("shared", "main\n");
    let main = fixture.commit("main");
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let tip = branch(&service, &repo, "refs/heads/side");
    let abort = outcome(
        run(&service, &repo, RefMutation::Merge(tip.clone(), author())),
        NavigationOutcome::MergePending,
    )
    .expect("abort receipt");
    outcome(
        run(&service, &repo, RefMutation::Abort(abort)),
        NavigationOutcome::MergeAborted,
    );
    assert_eq!(fixture.oid("HEAD"), main);
    assert_eq!(
        fs::read(fixture.path().join("shared")).expect("aborted content"),
        b"main\n"
    );
    let abort = outcome(
        run(&service, &repo, RefMutation::Merge(tip, author())),
        NavigationOutcome::MergePending,
    )
    .expect("abort receipt");
    let TaskValue::Navigation(snapshot) = inspect(&service, &repo, Inspection::Navigation) else {
        panic!("expected merge state");
    };
    assert_eq!(snapshot.merge, MergeState::Active(side.clone(), 1));
    fixture.write("shared", "manual resolution\n");
    assert!(matches!(
        run(&service, &repo, RefMutation::Abort(abort)),
        Err(GitError::StaleSnapshot)
    ));
    assert_eq!(
        fs::read(fixture.path().join("shared")).expect("resolution preserved"),
        b"manual resolution\n"
    );
    let expected = Arc::new(status(&service, &repo, false));
    let staged = submit(
        &service,
        Request::Mutate(
            repo.clone(),
            Box::new(Mutation {
                expected,
                action: MutationAction::Stage(vec![ChangeKey {
                    path: "shared".into(),
                    side: ChangeSide::Conflict,
                }]),
            }),
        ),
    );
    assert!(matches!(staged.result, Ok(TaskValue::Changed(None))));
    let draft = CommitDraft {
        title: "Resolve shared file".into(),
        author: author(),
        ..CommitDraft::default()
    };
    outcome(
        run(&service, &repo, RefMutation::Finish(side, draft)),
        NavigationOutcome::MergeCommitted,
    );
    assert_eq!(
        fixture.git(["show", "HEAD:shared"]).stdout,
        b"manual resolution\n"
    );
}

#[test]
fn mutation_refuses_stale_status_and_timed_out_writes() {
    let fixture = Fixture::new(&[]);
    fixture.commit("base");
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let expected = Arc::new(status(&service, &repo, false));
    fixture.write("local", "preserve");
    assert!(matches!(
        mutate(
            &service,
            &repo,
            RefMutation::Create("stale".into(), BranchSource::Head),
            expected
        ),
        Err(GitError::StaleSnapshot)
    ));
    let timed = engine(EngineOptions {
        timeout: std::time::Duration::from_nanos(1),
        ..EngineOptions::default()
    });
    let expected = Arc::new(status(&service, &repo, false));
    assert!(matches!(
        mutate(
            &timed,
            &repo,
            RefMutation::Create("timed".into(), BranchSource::Head),
            expected
        ),
        Err(GitError::Timeout)
    ));
    assert_eq!(
        fixture
            .git(["for-each-ref", "--format=%(refname)", "refs/heads/"])
            .stdout,
        b"refs/heads/main\n"
    );
}

#[test]
fn checkout_and_merge_do_not_overwrite_ignored_files() {
    let fixture = Fixture::new(&[]);
    fixture.write(".gitignore", "ignored\n");
    fixture.commit("base");
    fixture.git(["checkout", "-b", "topic"]);
    fixture.write("ignored", "tracked on topic");
    fixture.git(["add", "-f", "--", "ignored"]);
    fixture.commit("topic");
    fixture.git(["checkout", "main"]);
    fixture.write("ignored", "preserve the ignored copy");
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let topic = branch(&service, &repo, "refs/heads/topic");
    assert!(matches!(
        run(&service, &repo, RefMutation::Switch(topic.clone())),
        Err(GitError::BranchSwitchBlocked)
    ));
    assert!(run(&service, &repo, RefMutation::Merge(topic, author())).is_err());
    assert_eq!(
        fs::read(fixture.path().join("ignored")).expect("untracked data"),
        b"preserve the ignored copy"
    );
    assert_eq!(
        fixture.git(["symbolic-ref", "--short", "HEAD"]).stdout,
        b"main\n"
    );
}

#[test]
fn branch_comparison_counts_divergence_and_rejects_moved_tips() {
    let fixture = Fixture::new(&[]);
    fixture.commit("base");
    fixture.git(["switch", "-c", "topic"]);
    fixture.write("topic", "topic");
    fixture.commit("on topic");
    fixture.git(["switch", "main"]);
    fixture.write("main", "main");
    let head = fixture.commit("on main");
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let topic = branch(&service, &repo, "refs/heads/topic");
    assert!(matches!(
        inspect(
            &service,
            &repo,
            Inspection::CompareBranches(head.clone(), topic.clone())
        ),
        TaskValue::BranchComparison(1, 1)
    ));
    fixture.git(["branch", "-f", "topic", "HEAD"]);
    assert!(matches!(
        submit(
            &service,
            Request::Inspect(repo, Inspection::CompareBranches(head, topic))
        )
        .result,
        Err(GitError::StaleSnapshot)
    ));
}

#[test]
fn squash_stages_changes_without_rewriting_the_target_or_creating_merge_state() {
    let fixture = Fixture::new(&[]);
    let base = fixture.commit("base");
    fixture.git(["switch", "-c", "topic"]);
    fixture.write("topic", "from topic");
    fixture.commit("on topic");
    fixture.git(["switch", "main"]);
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    let topic = branch(&service, &repo, "refs/heads/topic");
    fixture.write("local", "keep");
    assert!(matches!(
        run(
            &service,
            &repo,
            RefMutation::Squash(topic.clone(), author())
        ),
        Err(GitError::MergeDirty)
    ));
    assert_eq!(
        fs::read(fixture.path().join("local")).expect("local file"),
        b"keep"
    );
    fs::remove_file(fixture.path().join("local")).expect("remove fixture file");
    outcome(
        run(&service, &repo, RefMutation::Squash(topic, author())),
        NavigationOutcome::SquashStaged,
    );
    assert_eq!(fixture.oid("HEAD"), base);
    assert!(!fixture.path().join(".git/MERGE_HEAD").exists());
    assert_eq!(fixture.git(["show", ":topic"]).stdout, b"from topic");
}

#[test]
fn stash_preserves_index_worktree_and_untracked_content_with_ignored_files_in_place() {
    let fixture = Fixture::new(&[]);
    fixture.write("tracked", "base");
    fixture.write(".gitignore", "ignored\n");
    fixture.commit("base");
    fixture.git(["config", "--local", "user.name", "Fixture"]);
    fixture.git(["config", "--local", "user.email", "fixture@example.invalid"]);
    fixture.write("tracked", "staged");
    fixture.git(["add", "--", "tracked"]);
    fixture.write("tracked", "unstaged");
    fixture.write("new file", "untracked");
    fixture.write("ignored", "ignored contents");
    let service = engine(EngineOptions::default());
    let repo = trust(&service, fixture.path());
    outcome(
        run(&service, &repo, RefMutation::Stash),
        NavigationOutcome::Stashed,
    );
    let TaskValue::Stashes(entries) = inspect(&service, &repo, Inspection::Stashes) else {
        panic!("expected saved stash");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, fixture.oid("refs/stash").as_str());
    assert_eq!(
        String::from_utf8_lossy(&fixture.git(["status", "--porcelain"]).stdout),
        ""
    );
    assert_eq!(
        fixture.git(["show", "stash@{0}:tracked"]).stdout,
        b"unstaged"
    );
    assert_eq!(
        fixture.git(["show", "stash@{0}^2:tracked"]).stdout,
        b"staged"
    );
    assert_eq!(
        fixture.git(["show", "stash@{0}^3:new file"]).stdout,
        b"untracked"
    );
    assert_eq!(
        fs::read(fixture.path().join("ignored")).expect("ignored file"),
        b"ignored contents"
    );
}
