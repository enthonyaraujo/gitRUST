//! Real Git fixtures. Every mutation is confined to an isolated temporary root.
#![allow(clippy::expect_used)]

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::OnceLock,
};

use gitrust_git::{
    DiffLineKind, DiffTarget, EngineOptions, EntryKind, FileChange, GitEngine, GitError,
    GitExecutable, GitTaskState, HeadState, HistoryPage, Inspection, ObjectId, OutputStream,
    ReferenceName, RepoPath, RepositoryCandidate, RepositoryDiff, RepositoryStatus, Request,
    TaskReport, TaskValue, TrustedRepository,
};
use tempfile::TempDir;

fn executable() -> &'static GitExecutable {
    static GIT: OnceLock<GitExecutable> = OnceLock::new();
    GIT.get_or_init(|| {
        GitExecutable::discover().expect("native Git 2.45+ is required for integration tests")
    })
}

fn engine(options: EngineOptions) -> GitEngine {
    GitEngine::new(executable().clone(), options).expect("start executor")
}

fn fixture_command(path: &Path) -> Command {
    let mut command = Command::new(executable().path());
    command
        .current_dir(path)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .args([
            "-c",
            "user.name=Stage Two",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.autocrlf=false",
            "-c",
            "commit.gpgSign=false",
        ]);
    #[cfg(windows)]
    for var in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(var) {
            command.env(var, value);
        }
    }
    command
}

struct Fixture(TempDir);

impl Fixture {
    fn new(arguments: &[&str]) -> Self {
        let root = Self(
            tempfile::Builder::new()
                .prefix("gitRUST fixtures á ")
                .tempdir()
                .expect("temporary repository"),
        );
        let mut init = vec!["init", "--quiet", "--initial-branch=main"];
        init.extend_from_slice(arguments);
        root.git(init);
        root
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn git(&self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Output {
        let output = fixture_command(self.path())
            .args(args)
            .output()
            .expect("fixture Git starts");
        assert!(
            output.status.success(),
            "fixture command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn write(&self, name: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
        fs::write(self.path().join(name), contents).expect("fixture write");
    }

    fn commit(&self, description: &str) -> ObjectId {
        self.git(["add", "--all", "--"]);
        self.git(["commit", "--quiet", "--allow-empty", "-m", description]);
        self.oid("HEAD")
    }

    fn oid(&self, reference: &str) -> ObjectId {
        let output = self.git(["rev-parse", "--verify", "--end-of-options", reference]);
        ObjectId::new(
            std::str::from_utf8(&output.stdout)
                .expect("ASCII Git ID")
                .trim(),
        )
        .expect("complete object ID")
    }
}

fn submit(engine: &GitEngine, request: Request) -> TaskReport {
    let task = engine.submit(request).expect("enqueue");
    let id = task.id();
    let report = task.wait().expect("receive");
    assert_eq!(report.id, id);
    report
}

fn identify(engine: &GitEngine, path: &Path) -> RepositoryCandidate {
    let Ok(TaskValue::Identified(candidate)) =
        submit(engine, Request::Identify(path.to_owned())).result
    else {
        panic!("fixture not identified");
    };
    candidate
}

fn trust(engine: &GitEngine, path: &Path) -> TrustedRepository {
    identify(engine, path).authorize_local_metadata()
}

fn inspect(engine: &GitEngine, repo: &TrustedRepository, operation: Inspection) -> TaskValue {
    submit(engine, Request::Inspect(repo.clone(), operation))
        .result
        .expect("Git inspection succeeds")
}

fn status(engine: &GitEngine, repo: &TrustedRepository, include_ignored: bool) -> RepositoryStatus {
    let TaskValue::Status(value) = inspect(engine, repo, Inspection::Status { include_ignored })
    else {
        panic!("expected status");
    };
    value
}

fn diff(
    engine: &GitEngine,
    repo: &TrustedRepository,
    target: DiffTarget,
    path: Option<RepoPath>,
) -> RepositoryDiff {
    let report = submit(
        engine,
        Request::Inspect(repo.clone(), Inspection::Diff { target, path }),
    );
    let TaskValue::Diff(value) = report.result.expect("diff succeeds") else {
        panic!("expected diff");
    };
    assert_eq!(
        report
            .processes
            .first()
            .expect("raw diff available")
            .status
            .code(),
        Some(i32::from(!value.files.is_empty()))
    );
    value
}

fn key_files(repo: &TrustedRepository) -> [PathBuf; 4] {
    let candidate = repo.repository();
    let common = candidate.identity().common_dir();
    [
        candidate.git_dir().join("index"),
        candidate.git_dir().join("HEAD"),
        common.join("config"),
        common.join("refs/heads/main"),
    ]
}

#[test]
fn empty_clean_and_dirty_status_preserves_index_config_refs_and_files() {
    let fixture = Fixture::new(&[]);
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let TaskValue::Info(unborn) = inspect(&engine, &repo, Inspection::Info) else {
        panic!("expected info");
    };
    assert!(
        matches!(unborn.head, HeadState::Unborn(branch) if branch.as_bytes() == b"refs/heads/main")
    );
    assert!(status(&engine, &repo, false).branch.oid.is_none());
    assert_eq!(diff(&engine, &repo, DiffTarget::Index, None).files, []);
    let TaskValue::History(empty) = inspect(
        &engine,
        &repo,
        Inspection::History(HistoryPage::new(0, 10).expect("valid page")),
    ) else {
        panic!("expected history");
    };
    assert_eq!(empty.commits, []);

    fixture.write("tracked", "base\n");
    fixture.write("removed", "removed\n");
    fixture.write(".gitignore", "*.ignored\n");
    let commit = fixture.commit("first");
    assert_eq!(status(&engine, &repo, false).entries, []);
    fixture.write("tracked", "staged\n");
    fixture.write("new file", "added\n");
    fixture.git(["add", "--", "tracked", "new file"]);
    fixture.write("tracked", "working\n");
    fs::remove_file(fixture.path().join("removed")).expect("fixture deletion");
    fixture.write("untracked", "local\n");
    fixture.write("local.ignored", "ignored\n");

    let files = key_files(&repo);
    let contents = files
        .each_ref()
        .map(|path| fs::read(path).expect("snapshot metadata"));
    let changes = status(&engine, &repo, true);
    assert_eq!(changes.branch.oid, Some(commit));
    assert!(
        changes
            .entries
            .iter()
            .any(|entry| entry.path.as_bytes() == b"tracked"
                && entry.index == Some(FileChange::Modified)
                && entry.worktree == Some(FileChange::Modified))
    );
    assert!(
        changes
            .entries
            .iter()
            .any(|entry| entry.path.as_bytes() == b"removed"
                && entry.worktree == Some(FileChange::Deleted))
    );
    assert!(changes.entries.iter().any(
        |entry| entry.path.as_bytes() == b"new file" && entry.index == Some(FileChange::Added)
    ));
    assert!(
        changes
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Untracked)
    );
    assert!(
        changes
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Ignored)
    );
    assert!(
        !status(&engine, &repo, false)
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Ignored)
    );
    assert_eq!(
        files
            .each_ref()
            .map(|path| fs::read(path).expect("verify metadata")),
        contents
    );
    assert_eq!(
        fs::read(fixture.path().join("tracked")).expect("worktree file"),
        b"working\n"
    );
    assert!(!repo.repository().git_dir().join("index.lock").exists());
}

#[test]
fn staged_and_worktree_patches_have_binary_rename_mode_line_and_literal_path_metadata() {
    let fixture = Fixture::new(&[]);
    fixture.write("before.txt", "a\nb\nc\n");
    fixture.write("text", b"first\nold\r\nlast");
    fixture.write("binary", [0, 1, 2]);
    fixture.write("-dash", "first\n");
    let first = fixture.commit("base");
    fixture.git(["mv", "--", "before.txt", "after name.txt"]);
    fixture.write("binary", [0, 1, 3]);
    fixture.write("text", b"first\nnew\r\nfinal");
    fixture.git(["update-index", "--chmod=+x", "--", "-dash"]);
    fixture.write("-dash", "second\n");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let staged = diff(&engine, &repo, DiffTarget::Index, None);
    assert!(staged.files.iter().any(|file| {
        file.path.as_bytes() == b"after name.txt"
            && file
                .from
                .as_ref()
                .is_some_and(|from| from.as_bytes() == b"before.txt")
            && file.change == FileChange::Renamed
            && file.score == Some(100)
    }));
    assert!(
        staged
            .files
            .iter()
            .any(|file| file.path.as_bytes() == b"-dash" && file.modes == [0o100_644, 0o100_755])
    );
    assert!(status(&engine, &repo, false).entries.iter().any(|entry|
        matches!(&entry.kind, EntryKind::RenamedOrCopied { from, similarity: 100 } if from.as_bytes() == b"before.txt")));
    let unstaged = diff(&engine, &repo, DiffTarget::Worktree, None);
    assert!(
        unstaged
            .files
            .iter()
            .any(|file| file.path.as_bytes() == b"binary" && file.binary)
    );
    let text = unstaged
        .files
        .iter()
        .find(|file| file.path.as_bytes() == b"text")
        .expect("text diff");
    let hunk = text.hunks.first().expect("one hunk");
    assert_eq!(hunk.old, (1, 3));
    assert!(
        hunk.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Context && line.new_line == Some(1))
    );
    assert!(
        hunk.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Removed
                && line.old_line == Some(2)
                && line.content == b"old\r")
    );
    assert!(
        hunk.lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Added
                && line.content == b"final"
                && !line.has_newline)
    );
    let path = RepoPath::new(PathBuf::from("-dash")).expect("literal path");
    let one = diff(&engine, &repo, DiffTarget::Worktree, Some(path));
    assert_eq!(one.files.len(), 1);
    assert_eq!(one.files[0].path.as_bytes(), b"-dash");
    let mut lines = one.files[0].hunks.iter().flat_map(|hunk| &hunk.lines);
    assert!(lines.any(|line| line.kind == DiffLineKind::Added && line.content == b"second"));
    let final_id = fixture.commit("edited");
    let between = diff(
        &engine,
        &repo,
        DiffTarget::Between {
            old: first,
            new: final_id,
        },
        None,
    );
    assert!(between.files.len() >= 3);
    let missing = ObjectId::new(&"f".repeat(40)).expect("full ID");
    let invalid = submit(
        &engine,
        Request::Inspect(
            repo,
            Inspection::Diff {
                target: DiffTarget::Between {
                    old: missing.clone(),
                    new: missing,
                },
                path: None,
            },
        ),
    );
    assert!(matches!(invalid.result, Err(GitError::InvalidReference)));
}

#[test]
fn finds_branches_merge_parents_upstream_pagination_and_detached_head() {
    let fixture = Fixture::new(&[]);
    let base = fixture.commit("ação\n\nbody\u{1e}with\u{1f}separators");
    fixture.git(["switch", "--quiet", "-c", "topic"]);
    fixture.write("topic", "topic");
    let topic = fixture.commit("topic");
    fixture.git(["switch", "--quiet", "main"]);
    fixture.write("main", "main");
    let main = fixture.commit("main");
    fixture.git(["merge", "--quiet", "--no-ff", "-m", "merge", "topic"]);
    let merged = fixture.oid("HEAD");
    fixture.git(["remote", "add", "local", "https://example.invalid/ignored"]);
    fixture.git(["update-ref", "refs/remotes/local/main", base.as_str()]);
    fixture.git(["branch", "--set-upstream-to=local/main", "main"]);
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let TaskValue::Branches(branches) = inspect(&engine, &repo, Inspection::Branches) else {
        panic!("expected branches");
    };
    assert!(matches!(branches.head, HeadState::Attached(_, ref id) if id == &merged));
    assert!(branches.branches.iter().any(|branch| {
        branch.current
            && branch
                .upstream
                .as_ref()
                .is_some_and(|name| name.as_bytes() == b"refs/remotes/local/main")
    }));
    assert!(
        branches
            .branches
            .iter()
            .any(|branch| branch.remote_tracking && branch.id == base)
    );
    assert_eq!(
        status(&engine, &repo, false).branch.divergence,
        Some((3, 0))
    );
    let page = |skip, size| {
        let TaskValue::History(history) = inspect(
            &engine,
            &repo,
            Inspection::History(HistoryPage::new(skip, size).expect("valid page")),
        ) else {
            panic!("expected page");
        };
        history
    };
    let first = page(0, 1);
    assert_eq!(first.root, Some(merged));
    assert_eq!(first.next_offset, Some(1));
    assert_eq!(first.commits[0].parents, [main, topic]);
    assert_eq!(first.commits[0].author.name, b"Stage Two");
    assert_eq!(first.commits[0].committer.email, b"fixture@example.invalid");
    let next = page(1, 10);
    let original = next
        .commits
        .iter()
        .find(|commit| commit.id == base)
        .expect("root commit");
    assert_eq!(original.subject, "ação".as_bytes());
    assert!(original.message.ends_with(b"body\x1ewith\x1fseparators\n"));
    fixture.git(["switch", "--quiet", "--detach", base.as_str()]);
    let TaskValue::Branches(detached) = inspect(&engine, &repo, Inspection::Branches) else {
        panic!("expected branches");
    };
    assert_eq!(detached.head, HeadState::Detached(base));
    assert!(detached.branches.iter().all(|branch| !branch.current));
    assert!(status(&engine, &repo, false).branch.branch.is_none());
}

#[test]
fn identifies_bare_and_linked_worktrees_with_shared_identity() {
    let fixture = Fixture::new(&[]);
    let base = fixture.commit("initial");
    let nested = fixture.path().join("sub dir");
    fs::create_dir(&nested).expect("fixture subdirectory");
    let sibling = tempfile::tempdir().expect("linked worktree parent");
    let linked = sibling.path().join("linked á");
    fixture.git([
        OsStr::new("worktree"),
        OsStr::new("add"),
        OsStr::new("--quiet"),
        OsStr::new("-b"),
        OsStr::new("linked"),
        linked.as_os_str(),
    ]);
    let engine = engine(EngineOptions::default());
    let primary = identify(&engine, &nested);
    let secondary = identify(&engine, &linked);
    assert_eq!(primary.identity(), secondary.identity());
    assert_ne!(primary.git_dir(), secondary.git_dir());
    let TaskValue::Info(info) = inspect(
        &engine,
        &secondary.authorize_local_metadata(),
        Inspection::Info,
    ) else {
        panic!("expected worktree info");
    };
    assert!(
        matches!(info.head, HeadState::Attached(name, id) if name.as_bytes() == b"refs/heads/linked" && id == base)
    );
    let bare_fixture = Fixture::new(&["--bare"]);
    let bare_repo = trust(&engine, bare_fixture.path());
    assert!(bare_repo.repository().worktree().is_none());
    assert!(
        matches!(inspect(&engine, &bare_repo, Inspection::Info), TaskValue::Info(info)
        if matches!(info.head, HeadState::Unborn(_)))
    );
    assert!(matches!(
        submit(
            &engine,
            Request::Inspect(
                bare_repo,
                Inspection::Status {
                    include_ignored: false
                }
            )
        )
        .result,
        Err(GitError::UnsupportedRepositoryFeature(_))
    ));
}

#[test]
fn sha256_supports_status_refs_history_and_comparison() {
    let fixture = Fixture::new(&["--object-format=sha256"]);
    fixture.write("file", "before\n");
    let before = fixture.commit("before");
    fixture.write("file", "after\n");
    let after = fixture.commit("after");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let TaskValue::Info(info) = inspect(&engine, &repo, Inspection::Info) else {
        panic!("expected info");
    };
    assert_eq!(info.object_format, "sha256");
    assert_eq!(
        status(&engine, &repo, false).branch.oid,
        Some(after.clone())
    );
    let TaskValue::Branches(branches) = inspect(&engine, &repo, Inspection::Branches) else {
        panic!("expected branches");
    };
    assert_eq!(branches.branches[0].id.as_str().len(), 64);
    let data = diff(
        &engine,
        &repo,
        DiffTarget::Between {
            old: before,
            new: after,
        },
        None,
    );
    assert_eq!(data.files[0].objects[0].as_str().len(), 64);
    let TaskValue::History(history) = inspect(
        &engine,
        &repo,
        Inspection::History(HistoryPage::new(0, 2).expect("valid page")),
    ) else {
        panic!("expected history");
    };
    let first = &history.commits[0];
    assert_eq!(first.id.as_str().len(), 64);
    assert_eq!(first.id.abbreviated().len(), 12);
    assert_eq!(first.parents[0].as_str().len(), 64);
}

#[test]
fn conflict_reports_index_stages_without_a_combined_patch() {
    let fixture = Fixture::new(&[]);
    fixture.write("shared", "base\n");
    fixture.commit("base");
    fixture.git(["switch", "--quiet", "-c", "side"]);
    fixture.write("shared", "side\n");
    fixture.commit("side");
    fixture.git(["switch", "--quiet", "main"]);
    fixture.write("shared", "main\n");
    fixture.commit("main");
    let conflict = fixture_command(fixture.path())
        .args(["merge", "--no-edit", "side"])
        .output()
        .expect("fixture merge");
    assert_eq!(conflict.status.code(), Some(1));
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let status = status(&engine, &repo, false);
    assert!(status.entries.iter().any(|entry| matches!(&entry.kind, EntryKind::Unmerged { modes, objects }
        if modes.iter().all(|&mode| mode == 0o100_644) && objects.iter().all(|id| !id.as_str().starts_with("0000")))));
    assert_eq!(diff(&engine, &repo, DiffTarget::Index, None).files, []);
    assert_eq!(
        diff(&engine, &repo, DiffTarget::Worktree, None).files[0]
            .path
            .as_bytes(),
        b"shared"
    );
}

#[cfg(unix)]
#[test]
fn unix_paths_preserve_native_bytes_and_literal_pathspecs() {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let fixture = Fixture::new(&[]);
    // macOS rejects the invalid UTF-8 filename on the runner's filesystem.
    // The decoder still has to preserve that byte independently of the host.
    let synthetic =
        RepositoryStatus::parse(b"# branch.oid (initial)\0# branch.head main\0? byte\xff\0")
            .expect("byte-oriented status entry");
    assert_eq!(
        synthetic.entries[0]
            .path
            .to_path_buf()
            .expect("native byte path")
            .as_os_str()
            .as_bytes(),
        b"byte\xff"
    );
    let native_name = if cfg!(target_os = "macos") {
        b"a tab\tand\nbyte".as_slice()
    } else {
        b"a tab\tand\nbyte\xff"
    };
    let weird = PathBuf::from(std::ffi::OsString::from_vec(native_name.to_vec()));
    let magic = Path::new(":(glob)*");
    fixture.write(&weird, b"one\n");
    fixture.write(magic, b"one\n");
    fixture.commit("base");
    fixture.write(&weird, b"two\n");
    fixture.write(magic, b"two\n");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let status = status(&engine, &repo, false);
    assert!(
        status
            .entries
            .iter()
            .any(|entry| entry.path.to_path_buf().expect("native path") == weird)
    );
    let one = diff(
        &engine,
        &repo,
        DiffTarget::Worktree,
        Some(RepoPath::new(weird.clone()).expect("native literal")),
    );
    assert_eq!(one.files[0].path.as_bytes(), weird.as_os_str().as_bytes());
    let one = diff(
        &engine,
        &repo,
        DiffTarget::Worktree,
        Some(RepoPath::new(magic.into()).expect("magic is literal")),
    );
    assert_eq!(one.files.len(), 1);
    assert_eq!(one.files[0].path.as_bytes(), b":(glob)*");

    let link = tempfile::tempdir().expect("sibling");
    let alias = link.path().join("symlink");
    std::os::unix::fs::symlink(fixture.path(), &alias).expect("symlink fixture");
    assert_eq!(
        identify(&engine, &alias).identity(),
        repo.repository().identity()
    );
}

#[test]
fn policy_refuses_unapproved_metadata_and_keeps_existing_locks() {
    let fixture = Fixture::new(&[]);
    fixture.commit("base");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let marker = fixture.path().join("must-not-be-created");
    for key in [
        "core.fsmonitor",
        "core.hooksPath",
        "diff.external",
        "diff.custom.command",
        "diff.custom.textconv",
        "filter.custom.clean",
        "filter.custom.smudge",
        "filter.custom.process",
        "include.path",
        "includeIf.gitdir:unused.path",
        "core.worktree",
        "alias.status",
        "pager.status",
        "submodule.other.update",
    ] {
        fixture.git([
            OsStr::new("config"),
            OsStr::new("--local"),
            OsStr::new(key),
            marker.as_os_str(),
        ]);
        let report = submit(
            &engine,
            Request::Inspect(
                repo.clone(),
                Inspection::Status {
                    include_ignored: false,
                },
            ),
        );
        assert!(
            matches!(report.result, Err(GitError::SecurityPolicyViolation(_))),
            "blocked {key}: {report:?}"
        );
        assert!(report.processes.is_empty());
        assert!(!marker.exists());
        fixture.git([
            "config",
            "--file",
            ".git/config",
            "--no-includes",
            "--unset-all",
            key,
        ]);
    }
    fixture.git(["config", "--local", "remote.origin.promisor", "true"]);
    let result = submit(&engine, Request::Inspect(repo.clone(), Inspection::Info)).result;
    assert!(matches!(
        result,
        Err(GitError::UnsupportedRepositoryFeature("partial clone"))
    ));
    fixture.git([
        "config",
        "--file",
        ".git/config",
        "--unset-all",
        "remote.origin.promisor",
    ]);
    fixture.write(".git/objects/info/alternates", "absent\n");
    assert!(matches!(
        submit(&engine, Request::Inspect(repo.clone(), Inspection::Info)).result,
        Err(GitError::UnsupportedRepositoryFeature(_))
    ));
    fs::remove_file(fixture.path().join(".git/objects/info/alternates"))
        .expect("remove fixture alternate");
    fixture.write(".git/index.lock", b"existing lock");
    assert!(matches!(
        submit(&engine, Request::Inspect(repo, Inspection::Info)).result,
        Err(GitError::RepositoryLocked)
    ));
    assert_eq!(
        fs::read(fixture.path().join(".git/index.lock")).expect("preserved lock"),
        b"existing lock"
    );
}

#[test]
fn bounded_output_malformed_frames_and_argument_validation_are_explicit() {
    let fixture = Fixture::new(&[]);
    fixture.write("a".repeat(80), "untracked");
    let engine = engine(EngineOptions {
        stdout_limit: 64,
        ..EngineOptions::default()
    });
    let repo = trust(&engine, fixture.path());
    let task = engine
        .submit(Request::Inspect(
            repo,
            Inspection::Status {
                include_ignored: false,
            },
        ))
        .expect("enqueue");
    assert!(matches!(
        task.state(),
        GitTaskState::Queued | GitTaskState::Running | GitTaskState::Failed
    ));
    let report = task.wait().expect("receive");
    assert!(matches!(
        report.result,
        Err(GitError::OutputLimitExceeded(OutputStream::Stdout))
    ));
    assert!(ObjectId::new("HEAD~2").is_err());
    assert!(ReferenceName::new("refs/heads/new@{2}").is_err());
    assert!(ReferenceName::new("refs/heads/tab\t").is_err());
    assert!(RepoPath::new("../escape".into()).is_err());
    assert!(RepoPath::new(fixture.path().to_owned()).is_err());
    assert!(HistoryPage::new(0, 501).is_err());
    assert!(
        RepositoryStatus::parse(b"# branch.oid (initial)\0# branch.head main\0? cut mid-path")
            .is_err()
    );
    assert!(
        RepositoryStatus::parse(b"# branch.oid (initial)\0# branch.head main\0x future\0").is_err()
    );
    assert!(RepositoryDiff::parse(b":raw\0unterminated patch").is_err());
}

#[test]
fn simultaneous_requests_return_their_own_structured_results() {
    let fixture = Fixture::new(&[]);
    let id = fixture.commit("base");
    let engine = engine(EngineOptions {
        workers: 4,
        queue_capacity: 24,
        ..EngineOptions::default()
    });
    let repo = trust(&engine, fixture.path());
    let tasks: Vec<_> = (0..20)
        .map(|_| {
            engine
                .submit(Request::Inspect(
                    repo.clone(),
                    Inspection::Status {
                        include_ignored: false,
                    },
                ))
                .expect("enqueue")
        })
        .collect();
    let mut ids = std::collections::HashSet::new();
    for task in tasks {
        let id_task = task.id();
        assert!(ids.insert(id_task));
        let report = task.wait().expect("result delivered");
        assert_eq!(report.id, id_task);
        assert!(
            matches!(report.result, Ok(TaskValue::Status(status)) if status.branch.oid == Some(id.clone()))
        );
    }
    engine.shutdown();
}

#[test]
#[ignore = "subprocess fixture with an isolated environment"]
fn fixture_ambient_git_settings() {
    assert!(std::env::var_os("GIT_DIR").is_some());
    let fixture = Fixture::new(&[]);
    let id = fixture.commit("true repository");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    assert_eq!(status(&engine, &repo, false).branch.oid, Some(id));
}

#[test]
fn git_environment_isolated_and_worktree_git_file_is_never_used_as_executable() {
    let output = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--ignored",
            "--exact",
            "fixture_ambient_git_settings",
            "--nocapture",
        ])
        .env("GIT_DIR", "unused-local-fixture")
        .env("GIT_WORK_TREE", "unused-local-fixture")
        .env("GIT_INDEX_FILE", "unused-local-fixture")
        .env("GIT_COMMON_DIR", "unused-local-fixture")
        .env("GIT_OBJECT_DIRECTORY", "unused-local-fixture")
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", "unused-local-fixture")
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.bare")
        .env("GIT_CONFIG_VALUE_0", "true")
        .env("GIT_CONFIG_PARAMETERS", "invalid-fixture")
        .env("GIT_EXEC_PATH", "unused-local-fixture")
        .env("GIT_EXTERNAL_DIFF", "unused-local-fixture")
        .env("GIT_SSH_COMMAND", "unused-local-fixture")
        .env("GIT_ASKPASS", "unused-local-fixture")
        .output()
        .expect("environment isolation fixture");
    assert!(
        output.status.success(),
        "isolation test failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let fixture = Fixture::new(&[]);
    fixture.commit("base");
    fixture.write(
        if cfg!(windows) { "git.exe" } else { "git" },
        b"inert fixture data",
    );
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    assert!(
        status(&engine, &repo, false)
            .entries
            .iter()
            .any(|entry| entry.kind == EntryKind::Untracked)
    );
}

#[test]
fn invalid_existing_head_ref_is_not_classified_as_unborn() {
    let fixture = Fixture::new(&[]);
    fixture.commit("base");
    fixture.write(".git/refs/heads/main", "not an object\n");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    assert!(matches!(
        submit(&engine, Request::Inspect(repo, Inspection::Info)).result,
        Err(GitError::InvalidReference)
    ));
}

#[test]
fn notification_arrives_after_the_result_is_available() {
    let fixture = Fixture::new(&[]);
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let (completed, notifications) = std::sync::mpsc::channel();
    let task = engine
        .submit_notifying(Request::Inspect(repo, Inspection::Info), completed)
        .expect("notifyable request");
    assert_eq!(
        notifications
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("completion event"),
        task.id()
    );
    assert_eq!(task.state(), GitTaskState::Completed);
    assert!(matches!(
        task.try_take().expect("published report").result,
        Ok(TaskValue::Info(_))
    ));
    assert!(task.try_take().is_none());
}

#[cfg(unix)]
#[test]
fn type_change_aligns_patch_fragments_and_rejects_linked_object_metadata() {
    let fixture = Fixture::new(&[]);
    fixture.write("changes-type", "original\n");
    fixture.write("ordinary", "first\n");
    fixture.commit("base");
    fs::remove_file(fixture.path().join("changes-type")).expect("remove file before replacing it");
    std::os::unix::fs::symlink("ordinary", fixture.path().join("changes-type"))
        .expect("new symbolic link");
    fixture.write("ordinary", "second\n");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    let patch = diff(&engine, &repo, DiffTarget::Worktree, None);
    assert_eq!(patch.files.len(), 2);
    let changed = &patch.files[0];
    assert_eq!(changed.path.as_bytes(), b"changes-type");
    assert_eq!(changed.change, FileChange::TypeChanged);
    assert_eq!(changed.hunks.len(), 2);
    assert_eq!(patch.files[1].path.as_bytes(), b"ordinary");
    std::os::unix::fs::symlink(
        "ordinary",
        fixture.path().join(".git/objects/info/disallowed"),
    )
    .expect("isolated linked metadata fixture");
    let result = submit(&engine, Request::Inspect(repo, Inspection::Info)).result;
    assert!(matches!(result, Err(GitError::SecurityPolicyViolation(_))));
}

#[test]
fn actual_branch_can_have_the_same_name_as_the_detached_sentinel() {
    let fixture = Fixture::new(&[]);
    fixture.git(["symbolic-ref", "HEAD", "refs/heads/(detached)"]);
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    assert_eq!(
        status(&engine, &repo, false)
            .branch
            .branch
            .expect("unborn named branch")
            .as_bytes(),
        b"(detached)"
    );
    fixture.commit("initial");
    assert_eq!(
        status(&engine, &repo, false)
            .branch
            .branch
            .expect("attached named branch")
            .as_bytes(),
        b"(detached)"
    );
}

#[test]
fn porcelain_decodes_copy_scores_submodule_flags_and_rejects_mixed_id_widths() {
    let sha1 = "a".repeat(40);
    let sha256 = "b".repeat(64);
    let entry = format!(
        "# branch.oid {sha1}\0# branch.head main\02 C. SCMU 160000 160000 160000 {sha1} {sha1} C085 current\0previous\0"
    );
    let status = RepositoryStatus::parse(entry.as_bytes()).expect("copy and submodule fields");
    assert!(
        matches!(&status.entries[0].kind, EntryKind::RenamedOrCopied { from, similarity: 85 }
        if from.as_bytes() == b"previous")
    );
    assert_eq!(
        status.entries[0].submodule,
        Some(gitrust_git::SubmoduleState::Submodule {
            commit: true,
            modified: true,
            untracked: true,
        })
    );
    let malformed = format!(
        "# branch.oid {sha1}\0# branch.head main\01 .M N... 100644 100644 100644 {sha1} {sha256} mismatch\0"
    );
    assert!(matches!(
        RepositoryStatus::parse(malformed.as_bytes()),
        Err(GitError::MalformedOutput(_))
    ));
}

#[test]
fn init_produces_an_unborn_workspace_and_never_replaces_existing_paths() {
    use gitrust_git::CreateRepository;

    let parent = tempfile::tempdir().expect("destination parent");
    let destination = parent.path().join("typed workspace");
    let engine = engine(EngineOptions::default());
    let request =
        CreateRepository::new(destination.clone(), "first".to_owned()).expect("valid init");
    let report = submit(&engine, Request::Create(request.clone()));
    let TaskValue::Provisioned(candidate) = report.result.expect("init succeeds") else {
        panic!("expected explicit candidate after init");
    };
    assert_eq!(
        candidate.worktree(),
        Some(
            destination
                .canonicalize()
                .expect("canonical path")
                .as_path()
        )
    );
    let repo = candidate.authorize_local_metadata();
    let TaskValue::Workspace(info, changes) = inspect(&engine, &repo, Inspection::Workspace) else {
        panic!("expected workspace info");
    };
    assert!(
        matches!(info.head, HeadState::Unborn(ref branch) if branch.as_bytes() == b"refs/heads/first")
    );
    assert!(!info.has_remote);
    assert!(changes.is_some_and(|status| status.entries.is_empty()));
    fs::write(destination.join("keep"), "unchanged").expect("user data");
    assert!(matches!(
        submit(&engine, Request::Create(request)).result,
        Err(GitError::DestinationExists)
    ));
    assert_eq!(
        fs::read_to_string(destination.join("keep")).expect("preserved data"),
        "unchanged"
    );
    assert!(CreateRepository::new(destination, "--bare".into()).is_err());
}

#[test]
fn cancellation_after_publication_keeps_the_completed_repository() {
    use gitrust_git::CreateRepository;
    use std::time::{Duration, Instant};

    let parent = tempfile::tempdir().expect("isolated destination parent");
    let destination = parent.path().join("published");
    let engine = engine(EngineOptions::default());
    let request =
        CreateRepository::new(destination.clone(), "main".to_owned()).expect("valid init request");
    let task = engine.submit(Request::Create(request)).expect("start init");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !destination.is_dir() {
        assert!(Instant::now() < deadline, "expected atomic publication");
        std::thread::sleep(Duration::from_millis(1));
    }
    task.cancel();
    assert!(matches!(
        task.wait().expect("late cancellation result").result,
        Ok(TaskValue::Provisioned(_))
    ));
    assert!(destination.join(".git").is_dir());
}

fn start_clone_waiting_for_tls(
    engine: &GitEngine,
    destination: &Path,
) -> (gitrust_git::GitTask, std::net::TcpStream) {
    use gitrust_git::{CloneRepository, CloneSource};
    use std::{
        io::Read,
        net::TcpListener,
        time::{Duration, Instant},
    };

    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("private HTTPS probe");
    listener
        .set_nonblocking(true)
        .expect("non-blocking listener");
    let address = listener.local_addr().expect("private loopback address");
    let source =
        CloneSource::new(&format!("https://{address}/fixture.git")).expect("HTTPS loopback URL");
    let request = CloneRepository::new(destination.to_owned(), source).expect("new destination");
    let task = engine
        .submit(Request::Clone(request))
        .expect("start HTTPS clone");
    let until = Instant::now() + Duration::from_secs(8);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < until,
                    "clone reached the loopback TLS probe"
                );
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => panic!("accept probe: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("bounded handshake probe");
    let mut first = [0; 1];
    stream.read_exact(&mut first).expect("first TLS record");
    assert_eq!(first[0], 22, "HTTPS must begin with the TLS handshake");
    (task, stream)
}

#[test]
fn clone_cancellation_keeps_existing_siblings_and_never_publishes_scratch_data() {
    let parent = tempfile::tempdir().expect("destination parent");
    fs::write(parent.path().join("keep"), "unchanged").expect("untouched sibling");
    let destination = parent.path().join("clone target");
    let engine = engine(EngineOptions::default());
    let (task, _stream) = start_clone_waiting_for_tls(&engine, &destination);
    task.cancel();
    assert!(matches!(
        task.wait().expect("cancelled clone report").result,
        Err(GitError::Cancelled)
    ));
    assert!(!destination.exists());
    assert_eq!(fs::read_dir(parent.path()).expect("cleanup").count(), 1);
    assert_eq!(
        fs::read_to_string(parent.path().join("keep")).expect("preserved sibling"),
        "unchanged"
    );
}

#[test]
fn clone_tls_failure_and_engine_shutdown_clear_each_owned_scratch_directory() {
    let parent = tempfile::tempdir().expect("destination parent");
    let engine = engine(EngineOptions::default());
    let destination = parent.path().join("clone target");
    let (task, stream) = start_clone_waiting_for_tls(&engine, &destination);
    drop(stream);
    let Err(GitError::CommandFailed(output)) = task.wait().expect("TLS failure report").result
    else {
        panic!("expected redacted clone failure");
    };
    assert_eq!(output.stdout.len(), 0);
    assert_eq!(output.stderr.len(), 0);
    assert_eq!(fs::read_dir(parent.path()).expect("cleanup").count(), 0);
    let (task, _stream) = start_clone_waiting_for_tls(&engine, &destination);
    engine.shutdown();
    assert!(matches!(
        task.wait().expect("shutdown report").result,
        Err(GitError::Cancelled)
    ));
    assert_eq!(fs::read_dir(parent.path()).expect("cleanup").count(), 0);
}

#[test]
#[ignore = "opt-in HTTPS test against a public host"]
fn public_https_clone_materializes_and_validates_a_workspace() {
    use gitrust_git::{CloneRepository, CloneSource};
    use std::time::Duration;

    let url = std::env::var("GITRUST_PUBLIC_CLONE_TEST_URL").expect("explicit public repository");
    let source = CloneSource::new(&url).expect("public HTTPS URL");
    let parent = tempfile::tempdir().expect("isolated clone parent");
    let destination = parent.path().join("clone");
    let engine = engine(EngineOptions {
        timeout: Duration::from_secs(180),
        ..EngineOptions::default()
    });
    let request = Request::Clone(CloneRepository::new(destination, source).expect("new path"));
    let report = submit(&engine, request);
    let value = report.result.expect("public HTTPS clone");
    let TaskValue::Provisioned(candidate) = value else {
        panic!("expected published clone");
    };
    let repo = candidate.authorize_local_metadata();
    let TaskValue::Workspace(info, Some(status)) = inspect(&engine, &repo, Inspection::Workspace)
    else {
        panic!("expected materialized working tree");
    };
    assert!(info.has_remote);
    assert_eq!(status.entries, []);
}

mod writes {
    use super::*;
    use gitrust_core::changes::{
        ChangeKey, ChangeSide, CommitAuthor, CommitDraft, CommitSigning, PatchSelection,
    };
    use gitrust_git::{ChangePatch, Mutation, MutationAction, change_rows};
    use std::{fmt::Write as _, sync::Arc};

    fn key(side: ChangeSide, path: impl Into<PathBuf>) -> ChangeKey {
        ChangeKey {
            path: path.into(),
            side,
        }
    }

    fn write(
        engine: &GitEngine,
        repo: &TrustedRepository,
        action: MutationAction,
    ) -> Result<TaskValue, GitError> {
        apply(engine, repo, status(engine, repo, false), action)
    }

    fn apply(
        engine: &GitEngine,
        repo: &TrustedRepository,
        snapshot: RepositoryStatus,
        action: MutationAction,
    ) -> Result<TaskValue, GitError> {
        submit(
            engine,
            Request::Mutate(
                repo.clone(),
                Box::new(Mutation {
                    expected: Arc::new(snapshot),
                    action,
                }),
            ),
        )
        .result
    }

    fn preview(engine: &GitEngine, repo: &TrustedRepository, key: ChangeKey) -> ChangePatch {
        let TaskValue::Change(patch) = inspect(engine, repo, Inspection::Change(key)) else {
            panic!("expected preview");
        };
        patch
    }

    fn review(
        engine: &GitEngine,
        repo: &TrustedRepository,
        keys: Vec<ChangeKey>,
    ) -> Vec<ChangePatch> {
        let TaskValue::DiscardReview(patches) =
            inspect(engine, repo, Inspection::DiscardReview(keys))
        else {
            panic!("expected review");
        };
        patches
    }

    fn author() -> CommitAuthor {
        CommitAuthor {
            name: "Stage Four".into(),
            email: "fixture@example.invalid".into(),
        }
    }

    fn partial(patch: ChangePatch, hunk: usize, line: Option<usize>) -> MutationAction {
        MutationAction::Partial {
            patch: Arc::new(patch),
            selection: vec![PatchSelection { hunk, line }],
        }
    }

    #[test]
    fn first_commit_whole_batch_and_amend_use_only_the_index() {
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        let r = trust(&e, f.path());
        f.write("-option", "alpha\n");
        f.write("unicode cópia.txt", "bravo\n");
        let paths = vec![
            key(ChangeSide::Untracked, "-option"),
            key(ChangeSide::Untracked, "unicode cópia.txt"),
        ];
        let untracked = preview(&e, &r, paths[1].clone()).display();
        assert!(untracked.text.contains("+bravo"), "{untracked:?}");
        write(&e, &r, MutationAction::Stage(paths)).expect("stage two under a lock");
        write(
            &e,
            &r,
            MutationAction::Unstage(vec![key(ChangeSide::Staged, "-option")]),
        )
        .expect("unstage unborn");
        f.write("unicode cópia.txt", "uncommitted\n");
        let draft = CommitDraft {
            title: "Primeiro commit".into(),
            description: "Descrição".into(),
            author: author(),
            ..CommitDraft::default()
        };
        assert!(matches!(
            write(
                &e,
                &r,
                MutationAction::Commit(CommitDraft {
                    author: CommitAuthor::default(),
                    ..draft.clone()
                })
            ),
            Err(GitError::AuthorRequired)
        ));
        let TaskValue::Changed(Some(initial)) =
            write(&e, &r, MutationAction::Commit(draft)).expect("first commit")
        else {
            panic!("no commit ID");
        };
        assert_eq!(f.oid("HEAD"), initial);
        assert_eq!(f.git(["show", "HEAD:unicode cópia.txt"]).stdout, b"bravo\n");
        assert_eq!(
            f.git(["ls-tree", "-z", "--name-only", "HEAD"]).stdout,
            "unicode cópia.txt\0".as_bytes(),
        );
        let TaskValue::CommitDraft(mut amend, _) =
            inspect(&e, &r, Inspection::CommitDraft { amend: true })
        else {
            panic!("no amend draft");
        };
        assert_eq!(amend.title, "Primeiro commit");
        amend.title = "Edited title".into();
        amend.author = CommitAuthor {
            name: "Committer".into(),
            ..author()
        };
        write(&e, &r, MutationAction::Commit(amend)).expect("message-only amend");
        assert_ne!(f.oid("HEAD"), initial);
        assert_eq!(
            f.git(["log", "-1", "--format=%an|%cn|%s"]).stdout,
            b"Stage Four|Committer|Edited title\n"
        );
        assert_eq!(f.git(["show", "HEAD:unicode cópia.txt"]).stdout, b"bravo\n");
    }

    #[test]
    fn single_line_and_hunk_apply_in_both_directions_and_keep_other_bytes() {
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        let name = "cópia [arquivo].txt";
        let mut before = String::new();
        for line in 0..32 {
            write!(before, "line {line}\r\n").expect("text fixture");
        }
        before.push_str("old tail");
        f.write(name, &before);
        f.commit("baseline");
        let r = trust(&e, f.path());
        let after = before
            .replace("line 3\r", "ação\t\r")
            .replace("old tail", "new tail");
        f.write(name, &after);
        let unstaged = key(ChangeSide::Unstaged, name);
        let staged = key(ChangeSide::Staged, name);
        let patch = preview(&e, &r, unstaged.clone());
        let data = patch.display();
        assert!(data.partial);
        assert_eq!(data.hunks.len(), 2);
        assert!(data.text.contains("\\ No newline"));
        let add = data.hunks[0]
            .lines
            .iter()
            .position(|line| line.text.starts_with("ação"))
            .expect("addition");
        write(&e, &r, partial(patch, 0, Some(add))).expect("stage exactly one addition");
        let index = f.git(["show", ":cópia [arquivo].txt"]).stdout;
        assert!(
            index
                .windows(b"a\xc3\xa7\xc3\xa3o\t\r\n".len())
                .any(|bytes| bytes == b"a\xc3\xa7\xc3\xa3o\t\r\n")
        );
        assert!(
            index
                .windows(b"line 3\r\n".len())
                .any(|bytes| bytes == b"line 3\r\n")
        );
        assert!(index.ends_with(b"old tail"));
        let rows = change_rows(&status(&e, &r, false)).expect("same path on both layers");
        assert!(rows.iter().any(|row| row.key == staged));
        assert!(rows.iter().any(|row| row.key == unstaged));
        let cached = preview(&e, &r, staged.clone());
        let pos = cached.display().hunks[0]
            .lines
            .iter()
            .position(|line| line.text.starts_with("ação"))
            .expect("staged addition");
        write(&e, &r, partial(cached, 0, Some(pos))).expect("unstage exactly one line");
        assert_eq!(
            f.git(["show", ":cópia [arquivo].txt"]).stdout,
            before.as_bytes()
        );
        write(&e, &r, partial(preview(&e, &r, unstaged.clone()), 1, None))
            .expect("stage entire hunk");
        assert_eq!(
            f.git(["show", ":cópia [arquivo].txt"]).stdout,
            before.replace("old tail", "new tail").as_bytes()
        );
        write(&e, &r, partial(preview(&e, &r, staged), 0, None)).expect("unstage entire hunk");
        assert_eq!(
            f.git(["show", ":cópia [arquivo].txt"]).stdout,
            before.as_bytes()
        );
        assert_eq!(
            fs::read(f.path().join(name)).expect("worktree"),
            after.as_bytes()
        );
    }

    #[test]
    fn partial_new_and_deleted_files_only_change_the_selected_index_lines() {
        let f = Fixture::new(&[]);
        f.write("removed", b"one\ntwo\n");
        f.commit("baseline");
        let e = engine(EngineOptions::default());
        let r = trust(&e, f.path());
        f.write("new", b"alpha\nbeta\n");
        let new = preview(&e, &r, key(ChangeSide::Untracked, "new"));
        write(&e, &r, partial(new, 0, Some(1))).expect("stage one new-file line");
        assert_eq!(f.git(["show", ":new"]).stdout, b"beta\n");
        assert_eq!(
            fs::read(f.path().join("new")).expect("new worktree file"),
            b"alpha\nbeta\n"
        );
        let new = preview(&e, &r, key(ChangeSide::Staged, "new"));
        write(&e, &r, partial(new, 0, None)).expect("unstage added hunk");
        assert_eq!(f.git(["ls-files", "--", "new"]).stdout, []);
        fs::remove_file(f.path().join("removed")).expect("fixture delete");
        let removed = preview(&e, &r, key(ChangeSide::Unstaged, "removed"));
        write(&e, &r, partial(removed, 0, Some(0))).expect("stage one deleted-file line");
        assert_eq!(f.git(["show", ":removed"]).stdout, b"two\n");
        let removed = preview(&e, &r, key(ChangeSide::Unstaged, "removed"));
        write(&e, &r, partial(removed, 0, None)).expect("stage final deletion");
        assert_eq!(f.git(["ls-files", "--", "removed"]).stdout, []);
        let removed = preview(&e, &r, key(ChangeSide::Staged, "removed"));
        write(&e, &r, partial(removed, 0, Some(1))).expect("unstage one deleted-file line");
        assert_eq!(f.git(["show", ":removed"]).stdout, b"two\n");
        assert!(!f.path().join("removed").exists());
    }

    #[test]
    fn mutation_that_times_out_before_execution_leaves_index_untouched() {
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        let r = trust(&e, f.path());
        f.write("pending", b"keep\n");
        let snapshot = status(&e, &r, false);
        let timed = engine(EngineOptions {
            timeout: std::time::Duration::from_nanos(1),
            ..EngineOptions::default()
        });
        let result = apply(
            &timed,
            &r,
            snapshot,
            MutationAction::Stage(vec![key(ChangeSide::Untracked, "pending")]),
        );
        assert!(matches!(result, Err(GitError::Timeout)));
        assert!(!f.path().join(".git/index.lock").exists());
        assert_eq!(f.git(["ls-files", "--"]).stdout, []);
    }

    #[cfg(unix)]
    #[test]
    fn authorization_does_not_follow_a_replaced_repository_at_the_same_path() {
        let original = Fixture::new(&[]);
        let replacement = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        let r = trust(&e, original.path());
        original.write("same", "original");
        let snapshot = status(&e, &r, false);
        replacement.write("same", "replacement");
        let archive = replacement.path().join("displaced");
        fs::rename(original.path().join(".git"), &archive).expect("displace metadata");
        fs::rename(
            replacement.path().join(".git"),
            original.path().join(".git"),
        )
        .expect("replace metadata");
        let result = apply(
            &e,
            &r,
            snapshot,
            MutationAction::Stage(vec![key(ChangeSide::Untracked, "same")]),
        );
        assert!(matches!(result, Err(GitError::SecurityPolicyViolation(_))));
        assert_eq!(original.git(["ls-files", "--"]).stdout, []);
    }

    #[test]
    fn discard_uses_reviewed_bytes_and_preserves_existing_index_and_locks() {
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        f.write("tracked", b"HEAD\n");
        f.commit("baseline");
        let r = trust(&e, f.path());
        f.write("tracked", b"index\n");
        f.git(["add", "--", "tracked"]);
        f.write("tracked", b"to be discarded\n");
        f.write("new", b"new\0binary\n");
        let key1 = key(ChangeSide::Unstaged, "tracked");
        let key2 = key(ChangeSide::Untracked, "new");
        let reviewed = review(&e, &r, vec![key1.clone(), key2.clone()]);
        assert!(reviewed[1].display().binary);
        f.write("tracked", b"changed since review\n");
        assert!(matches!(
            write(&e, &r, MutationAction::Discard(reviewed)),
            Err(GitError::StaleSnapshot)
        ));
        assert!(f.path().join("new").exists());
        let before_lock = status(&e, &r, false);
        let lock = f.path().join(".git/index.lock");
        fs::write(&lock, "external lock").expect("external lock");
        assert!(matches!(
            apply(
                &e,
                &r,
                before_lock,
                MutationAction::Stage(vec![key2.clone()])
            ),
            Err(GitError::RepositoryLocked)
        ));
        assert_eq!(fs::read(&lock).expect("lock preserved"), b"external lock");
        fs::remove_file(lock).expect("release fixture lock");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        write(
            &e,
            &r,
            MutationAction::Discard(review(&e, &r, vec![key1, key2])),
        )
        .expect("confirmed discard");
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        write(&e, &r, MutationAction::Discard(review(&e, &r, vec![key1])))
            .expect("confirmed discard");
        assert_eq!(
            fs::read(f.path().join("tracked")).expect("worktree"),
            b"index\n"
        );
        assert_eq!(f.git(["show", ":tracked"]).stdout, b"index\n");
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        assert!(!f.path().join("new").exists());
        write(
            &e,
            &r,
            MutationAction::Discard(review(&e, &r, vec![key(ChangeSide::Staged, "tracked")])),
        )
        .expect("confirmed staged discard");
        assert_eq!(
            fs::read(f.path().join("tracked")).expect("worktree"),
            b"HEAD\n"
        );
        assert_eq!(f.git(["show", ":tracked"]).stdout, b"HEAD\n");
    }

    #[test]
    fn binary_rename_and_non_utf8_paths_are_whole_file_changes() {
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        f.write("old", b"old\0binary\n");
        f.commit("baseline");
        let r = trust(&e, f.path());
        f.git(["mv", "old", "rename with spaces"]);
        let key = key(ChangeSide::Staged, "rename with spaces");
        let rows = change_rows(&status(&e, &r, false)).expect("rename");
        let renamed = rows.iter().find(|row| row.key == key).expect("rename row");
        assert_eq!(renamed.from.as_deref(), Some(Path::new("old")));
        let patch = preview(&e, &r, key.clone());
        assert!(patch.display().text.contains("rename from old"));
        assert!(!patch.display().partial);
        write(&e, &r, MutationAction::Unstage(vec![key]))
            .expect("unstage original and destination");
        assert_eq!(f.git(["diff", "--cached", "--name-only"]).stdout, []);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let raw = PathBuf::from(OsStr::from_bytes(b"name\xff [literal]"));
            f.write(&raw, b"native");
            let native = super::writes::key(ChangeSide::Untracked, &raw);
            write(&e, &r, MutationAction::Stage(vec![native])).expect("native bytes");
            assert!(
                f.git(["ls-files", "-z", "--"])
                    .stdout
                    .split(|byte| *byte == 0)
                    .any(|line| line == raw.as_os_str().as_bytes())
            );
        }
    }

    #[test]
    fn rejects_concurrent_snapshots_and_configuration_changes() {
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        let r = trust(&e, f.path());
        f.write("first", b"first");
        f.write("second", b"second");
        let baseline = Arc::new(status(&e, &r, false));
        let task = |path| {
            Request::Mutate(
                r.clone(),
                Box::new(Mutation {
                    expected: baseline.clone(),
                    action: MutationAction::Stage(vec![key(ChangeSide::Untracked, path)]),
                }),
            )
        };
        let first = e.submit(task("first")).expect("enqueue");
        let second = e.submit(task("second")).expect("enqueue");
        assert!(matches!(
            first.wait().expect("first").result,
            Ok(TaskValue::Changed(None))
        ));
        assert!(matches!(
            second.wait().expect("second").result,
            Err(GitError::StaleSnapshot)
        ));
        assert_eq!(f.git(["ls-files", "--"]).stdout, b"first\n");
        let before = status(&e, &r, false);
        f.git([
            "config",
            "--local",
            "gpg.ssh.program",
            "unauthorized-signer",
        ]);
        let result = apply(
            &e,
            &r,
            before,
            MutationAction::Stage(vec![key(ChangeSide::Untracked, "second")]),
        );
        assert!(matches!(
            result,
            Err(GitError::RepositoryNotTrusted | GitError::SecurityPolicyViolation(_))
        ));
        assert_eq!(f.git(["ls-files", "--"]).stdout, b"first\n");
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires a disposable GPG keyring and GITRUST_TEST_GPG_FINGERPRINT"]
    fn openpgp_uses_the_explicit_system_signer_and_key() {
        let keyring = std::env::var_os("GNUPGHOME").expect("isolated verification keyring");
        let fingerprint =
            std::env::var("GITRUST_TEST_GPG_FINGERPRINT").expect("disposable fingerprint");
        assert!(Path::new(&keyring).join("gitrust-test-keyring").is_file());
        let f = Fixture::new(&[]);
        let e = engine(EngineOptions::default());
        let r = trust(&e, f.path());
        f.write("file", b"signed content");
        write(
            &e,
            &r,
            MutationAction::Stage(vec![key(ChangeSide::Untracked, "file")]),
        )
        .expect("stage");
        let draft = CommitDraft {
            title: "Signed with OpenPGP".into(),
            author: author(),
            signing: CommitSigning::OpenPgp(fingerprint),
            ..CommitDraft::default()
        };
        write(&e, &r, MutationAction::Commit(draft)).expect("requested OpenPGP signature");
        assert!(
            fixture_command(f.path())
                .env("GNUPGHOME", keyring)
                .args(["-c", "gpg.program=/usr/bin/gpg", "verify-commit", "HEAD",])
                .output()
                .expect("verify with the disposable keyring")
                .status
                .success()
        );
    }

    #[cfg(unix)]
    #[test]
    fn ssh_explicit_signing_and_signed_amend_policy() {
        let f = Fixture::new(&[]);
        let signer = Path::new("/usr/bin/ssh-keygen");
        if !signer.is_file() {
            return;
        }
        let keys = tempfile::tempdir().expect("test keys only");
        let key_path = keys.path().join("fixture");
        assert!(
            Command::new(signer)
                .env_clear()
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(&key_path)
                .status()
                .expect("generate disposable key")
                .success()
        );
        let e = engine(EngineOptions::default());
        let r = trust(&e, f.path());
        f.write("file", b"content");
        write(
            &e,
            &r,
            MutationAction::Stage(vec![key(ChangeSide::Untracked, "file")]),
        )
        .expect("stage");
        let draft = CommitDraft {
            title: "Signed".into(),
            author: author(),
            signing: CommitSigning::Ssh(key_path),
            ..CommitDraft::default()
        };
        write(&e, &r, MutationAction::Commit(draft.clone())).expect("sign using explicit key");
        let old = f.oid("HEAD");
        let commit = f.git(["cat-file", "commit", "HEAD"]);
        assert!(
            String::from_utf8_lossy(&commit.stdout)
                .contains("gpgsig -----BEGIN SSH SIGNATURE-----")
        );
        let unsigned = CommitDraft {
            signing: CommitSigning::None,
            amend: true,
            ..draft.clone()
        };
        assert!(matches!(
            write(&e, &r, MutationAction::Commit(unsigned)),
            Err(GitError::SigningFailed)
        ));
        assert_eq!(f.oid("HEAD"), old);
        write(
            &e,
            &r,
            MutationAction::Commit(CommitDraft {
                title: "Signed amend".into(),
                amend: true,
                ..draft
            }),
        )
        .expect("signed amend");
        assert_ne!(f.oid("HEAD"), old);
    }
}

#[path = "support/navigation.rs"]
mod navigation;
