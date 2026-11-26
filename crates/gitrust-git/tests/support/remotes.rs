use super::*;
use gitrust_core::remotes::{Auth, Credential, Integration, RemoteAction, RemoteNotice};
use gitrust_git::{
    CloneRepository, RebaseProof, RemoteEndpoint, RemoteRequest, RemoteResult, RemoteSnapshot,
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

struct Server {
    child: Child,
    directory: TempDir,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start() -> Option<Self> {
        for tool in ["/usr/bin/python3", "/usr/bin/openssl"] {
            if !Path::new(tool).is_file() {
                eprintln!("loopback HTTPS fixture skipped: {tool} unavailable");
                return None;
            }
        }
        let directory = tempfile::tempdir().expect("server root");
        let cert = Command::new("/usr/bin/openssl")
            .current_dir(directory.path())
            .args([
                "req",
                "-x509",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-days",
                "1",
                "-subj",
                "/CN=localhost",
                "-addext",
                "subjectAltName=DNS:localhost,IP:127.0.0.1",
                "-keyout",
                "fixture.key",
                "-out",
                "fixture.crt",
            ])
            .output()
            .expect("certificate process");
        assert!(cert.status.success(), "{cert:?}");
        for name in ["public.git", "private.git", "protected.git", "slow.git"] {
            let output = fixture_command(directory.path())
                .args(["init", "--quiet", "--bare", "--initial-branch=main", name])
                .output()
                .expect("initialize bare remote");
            assert!(output.status.success(), "{output:?}");
        }
        let mut child = Command::new("/usr/bin/python3")
            .current_dir(directory.path())
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/smart_https.py"))
            .arg("/usr/bin/git")
            .stdout(Stdio::piped())
            .spawn()
            .expect("loopback fixture starts");
        let mut port = String::new();
        BufReader::new(child.stdout.take().expect("loopback fixture stdout"))
            .read_line(&mut port)
            .expect("loopback fixture listening");
        Some(Self {
            child,
            directory,
            port: port.trim().parse().expect("loopback port"),
        })
    }

    fn url(&self, name: &str) -> String {
        format!("https://localhost:{}/{name}.git", self.port)
    }

    fn auth(&self) -> Auth {
        Auth {
            ca: Some(self.directory.path().join("fixture.crt")),
            ..Auth::default()
        }
    }

    fn seed(&self, repo: &Fixture, name: &str) {
        let remote = self.directory.path().join(format!("{name}.git"));
        let output = fixture_command(repo.path())
            .env("GIT_ALLOW_PROTOCOL", "file")
            .arg("push")
            .arg("--")
            .arg(remote)
            .arg("HEAD:refs/heads/main")
            .output()
            .expect("fixture seed");
        assert!(output.status.success(), "{output:?}");
    }

    fn requests(&self) -> String {
        fs::read_to_string(self.directory.path().join("requests")).unwrap_or_default()
    }
}

fn snapshot(engine: &GitEngine, repo: &TrustedRepository) -> RemoteSnapshot {
    let TaskValue::Remotes(value) = inspect(engine, repo, Inspection::Remotes) else {
        panic!("expected remotes");
    };
    value
}

fn request(engine: &GitEngine, repo: &TrustedRepository, action: RemoteAction) -> RemoteRequest {
    RemoteRequest {
        expected: snapshot(engine, repo),
        status: Some(Arc::new(status(engine, repo, false))),
        action,
        bridge: None,
        progress: None,
        rebase: None,
    }
}

fn execute(
    engine: &GitEngine,
    repo: &TrustedRepository,
    request: RemoteRequest,
) -> Result<RemoteResult, GitError> {
    let report = submit(engine, Request::Remote(repo.clone(), Box::new(request)));
    assert!(report.processes.is_empty(), "network diagnostics retained");
    let TaskValue::RemoteResult(result) = report.result? else {
        panic!("expected outcome");
    };
    Ok(result)
}

fn run(engine: &GitEngine, repo: &TrustedRepository, action: RemoteAction) -> RemoteNotice {
    let outcome = execute(engine, repo, request(engine, repo, action));
    match outcome {
        Ok(result) => result.notice,
        Err(GitError::Remote(notice)) => notice,
        Err(error) => panic!("remote action failed: {error:?}"),
    }
}

fn add(engine: &GitEngine, repo: &TrustedRepository, name: &str, endpoint: String) {
    assert_eq!(
        run(engine, repo, RemoteAction::Add(name.into(), endpoint, None)),
        RemoteNotice::Updated
    );
}

fn setup() -> (Fixture, GitEngine, TrustedRepository) {
    let fixture = Fixture::new(&[]);
    fixture.git(["config", "--local", "user.name", "Stage Six"]);
    fixture.git(["config", "--local", "user.email", "six@example.invalid"]);
    fixture.write("shared", "base\n");
    fixture.commit("base");
    let engine = engine(EngineOptions::default());
    let repo = trust(&engine, fixture.path());
    (fixture, engine, repo)
}

#[test]
fn remote_configuration_is_local_and_stale_or_unsafe_edits_fail() {
    let (fixture, engine, repo) = setup();
    add(
        &engine,
        &repo,
        "work",
        "https://example.invalid/project.git".into(),
    );
    add(
        &engine,
        &repo,
        "backup",
        "git@example.invalid:project.git".into(),
    );
    assert_eq!(snapshot(&engine, &repo).entries.len(), 2);
    let stale = request(&engine, &repo, RemoteAction::Remove("backup".into()));
    assert_eq!(
        run(
            &engine,
            &repo,
            RemoteAction::Edit(
                "backup".into(),
                "https://example.invalid/fetch.git".into(),
                Some("ssh://git@example.invalid:2222/push.git".into()),
            )
        ),
        RemoteNotice::Updated
    );
    assert!(matches!(
        execute(&engine, &repo, stale),
        Err(GitError::StaleSnapshot)
    ));
    assert_eq!(
        run(
            &engine,
            &repo,
            RemoteAction::Rename("backup".into(), "origin".into())
        ),
        RemoteNotice::Updated
    );
    assert_eq!(
        run(
            &engine,
            &repo,
            RemoteAction::Upstream("origin".into(), "team/main".into())
        ),
        RemoteNotice::Updated
    );
    let upstream = snapshot(&engine, &repo).upstream;
    assert_eq!(upstream, Some(("origin".into(), "team/main".into())));
    assert_eq!(
        run(&engine, &repo, RemoteAction::Remove("origin".into())),
        RemoteNotice::Updated
    );
    fixture.git([
        "config",
        "--local",
        "remote.work.fetch",
        "+refs/heads/*:refs/heads/*",
    ]);
    let remove = request(&engine, &repo, RemoteAction::Remove("work".into()));
    assert!(matches!(
        execute(&engine, &repo, remove),
        Err(GitError::UnsupportedRepositoryFeature(_))
    ));
    let head = fixture.oid("HEAD");
    let command = request(
        &engine,
        &repo,
        RemoteAction::Add(
            "--no-tags".into(),
            "https://example.invalid/repo.git".into(),
            None,
        ),
    );
    assert!(matches!(
        execute(&engine, &repo, command),
        Err(GitError::InvalidReference)
    ));
    assert_eq!(head, fixture.oid("HEAD"));
    fixture.git([
        "config",
        "--local",
        "remote.hidden.url",
        "https://account:fixture-password@example.invalid/repo",
    ]);
    let snapshot = snapshot(&engine, &repo);
    assert!(
        snapshot
            .entries
            .iter()
            .any(|remote| remote.name == "hidden" && remote.fetch.is_none())
    );
    assert!(!format!("{snapshot:?}").contains("fixture-password"));
}

#[test]
fn https_clone_requires_tls_and_auth_without_exposing_secrets() {
    let (fixture, engine, _repo) = setup();
    let Some(server) = Server::start() else {
        return;
    };
    server.seed(&fixture, "public");
    server.seed(&fixture, "private");
    let targets = tempfile::tempdir().expect("clone targets");
    let clone = |target: &str, name: &str, auth| {
        let endpoint = RemoteEndpoint::new(&server.url(name)).expect("TLS endpoint");
        let request = CloneRepository::remote(targets.path().join(target), endpoint, auth, None)
            .expect("clone request");
        let report = submit(&engine, Request::Clone(request));
        assert!(report.processes.is_empty());
        assert!(!format!("{report:?}").contains("fixture-password"));
        report.result
    };
    assert!(matches!(
        clone("untrusted", "public", Auth::default()),
        Err(GitError::Remote(RemoteNotice::Certificate))
    ));
    assert!(matches!(
        clone("public", "public", server.auth()),
        Ok(TaskValue::Provisioned(_))
    ));
    assert!(matches!(
        clone("anonymous", "private", server.auth()),
        Err(GitError::Remote(RemoteNotice::Authentication))
    ));
    let account = |secret: &str| Auth {
        credential: Credential::Password {
            user: "fixture".into(),
            secret: secret.into(),
        },
        ..server.auth()
    };
    assert!(matches!(
        clone("wrong", "private", account("incorrect")),
        Err(GitError::Remote(RemoteNotice::Authentication))
    ));
    assert!(matches!(
        clone("private", "private", account("fixture-password")),
        Ok(TaskValue::Provisioned(_))
    ));
    assert!(!targets.path().join("untrusted").exists());
    assert!(!targets.path().join("wrong").exists());
    let config =
        fs::read_to_string(targets.path().join("private/.git/config")).expect("cloned config");
    assert!(!config.contains("fixture-password"));
}

fn push(remote: &str, name: &str, track: bool, auth: Auth) -> RemoteAction {
    RemoteAction::Push {
        remote: remote.into(),
        branch: name.into(),
        track,
        auth,
    }
}

fn pull(strategy: Integration, auth: Auth) -> RemoteAction {
    RemoteAction::Pull("origin".into(), strategy, auth)
}

#[test]
fn remote_publish_fetch_and_fast_forward_use_only_selected_refs() {
    let (fixture, engine, repo) = setup();
    let Some(server) = Server::start() else {
        return;
    };
    server.seed(&fixture, "public");
    add(&engine, &repo, "origin", server.url("public"));
    add(&engine, &repo, "backup", server.url("protected"));
    let before = server.requests();
    snapshot(&engine, &repo);
    inspect(&engine, &repo, Inspection::WorkspaceNavigation);
    assert_eq!(before, server.requests());
    assert_eq!(
        run(
            &engine,
            &repo,
            pull(Integration::FastForward, server.auth())
        ),
        RemoteNotice::NoUpstream
    );
    assert_eq!(
        run(
            &engine,
            &repo,
            RemoteAction::Fetch(vec!["origin".into(), "backup".into()], server.auth())
        ),
        RemoteNotice::Fetched
    );
    assert_eq!(fixture.oid("HEAD"), fixture.oid("origin/main"));
    assert_eq!(
        run(&engine, &repo, push("origin", "topic", true, server.auth())),
        RemoteNotice::Pushed
    );
    assert_eq!(
        run(
            &engine,
            &repo,
            push("origin", "topic", false, server.auth())
        ),
        RemoteNotice::UpToDate
    );
    assert_eq!(
        run(
            &engine,
            &repo,
            push("backup", "topic", false, server.auth())
        ),
        RemoteNotice::Rejected
    );
    assert_eq!(
        snapshot(&engine, &repo).upstream,
        Some(("origin".into(), "topic".into()))
    );
    assert_eq!(
        run(
            &engine,
            &repo,
            RemoteAction::Upstream("origin".into(), "main".into())
        ),
        RemoteNotice::Updated
    );
}

#[test]
fn remote_pull_fast_forward_and_rejected_push_preserve_worktree() {
    let (fixture, engine, repo) = setup();
    let Some(server) = Server::start() else {
        return;
    };
    server.seed(&fixture, "public");
    add(&engine, &repo, "origin", server.url("public"));
    run(
        &engine,
        &repo,
        RemoteAction::Upstream("origin".into(), "main".into()),
    );
    let base = fixture.oid("HEAD");
    fixture.write("server-only", "remote\n");
    let tip = fixture.commit("remote");
    server.seed(&fixture, "public");
    fixture.git(["reset", "--hard", base.as_str()]);
    assert_eq!(
        run(&engine, &repo, push("origin", "main", false, server.auth())),
        RemoteNotice::NonFastForward
    );
    fixture.write("untracked", "keep\n");
    let dirty = request(
        &engine,
        &repo,
        pull(Integration::FastForward, server.auth()),
    );
    assert!(matches!(
        execute(&engine, &repo, dirty),
        Err(GitError::MergeDirty)
    ));
    assert_eq!(
        fs::read(fixture.path().join("untracked")).expect("preserved"),
        b"keep\n"
    );
    fs::remove_file(fixture.path().join("untracked")).expect("remove fixture file");
    assert_eq!(
        run(
            &engine,
            &repo,
            pull(Integration::FastForward, server.auth())
        ),
        RemoteNotice::Integrated
    );
    assert_eq!(fixture.oid("HEAD"), tip);
    assert_eq!(
        run(
            &engine,
            &repo,
            pull(Integration::FastForward, server.auth())
        ),
        RemoteNotice::UpToDate
    );
}

fn diverge(fixture: &Fixture, server: &Server, conflict: bool) -> (ObjectId, ObjectId) {
    let base = fixture.oid("HEAD");
    fixture.write(if conflict { "shared" } else { "remote" }, "theirs\n");
    let theirs = fixture.commit("remote work");
    server.seed(fixture, "public");
    fixture.git(["reset", "--hard", base.as_str()]);
    fixture.write(if conflict { "shared" } else { "local" }, "ours\n");
    (fixture.commit("local work"), theirs)
}

#[test]
fn remote_pull_rebase_moves_the_local_commit_onto_the_fetched_tip() {
    let (fixture, engine, repo) = setup();
    let Some(server) = Server::start() else {
        return;
    };
    add(&engine, &repo, "origin", server.url("public"));
    run(
        &engine,
        &repo,
        RemoteAction::Upstream("origin".into(), "main".into()),
    );
    let (ours, theirs) = diverge(&fixture, &server, false);
    let result = execute(
        &engine,
        &repo,
        request(
            &engine,
            &repo,
            pull(Integration::FastForward, server.auth()),
        ),
    )
    .expect("fetch succeeded");
    assert_eq!(result.notice, RemoteNotice::Diverged);
    assert_eq!(result.fetched, ["origin"]);
    assert_eq!(fixture.oid("HEAD"), ours);
    assert_eq!(
        run(&engine, &repo, pull(Integration::Rebase, server.auth())),
        RemoteNotice::Integrated
    );
    assert_eq!(fixture.oid("HEAD^"), theirs);
    assert!(fixture.path().join("local").is_file());
    assert!(fixture.path().join("remote").is_file());
}

fn recover(
    engine: &GitEngine,
    repo: &TrustedRepository,
    proof: Arc<RebaseProof>,
    action: RemoteAction,
) -> Result<RemoteResult, GitError> {
    let mut request = request(engine, repo, action);
    request.rebase = Some(proof);
    execute(engine, repo, request)
}

#[test]
fn remote_pull_rebase_conflicts_protect_resolutions_and_can_continue_or_abort() {
    for abort in [true, false] {
        let (fixture, engine, repo) = setup();
        let Some(server) = Server::start() else {
            return;
        };
        add(&engine, &repo, "origin", server.url("public"));
        run(
            &engine,
            &repo,
            RemoteAction::Upstream("origin".into(), "main".into()),
        );
        let (ours, theirs) = diverge(&fixture, &server, true);
        let result = execute(
            &engine,
            &repo,
            request(&engine, &repo, pull(Integration::Rebase, server.auth())),
        )
        .expect("fetch and rebase");
        assert_eq!(result.notice, RemoteNotice::RebasePending);
        let proof = result.rebase.expect("rebase proof");
        if abort {
            let original = fs::read(fixture.path().join("shared")).expect("conflict markers");
            fixture.write("shared", "new work\n");
            assert!(matches!(
                recover(
                    &engine,
                    &repo,
                    Arc::clone(&proof),
                    RemoteAction::AbortRebase
                ),
                Err(GitError::StaleSnapshot)
            ));
            assert_eq!(
                fs::read(fixture.path().join("shared")).expect("preserved"),
                b"new work\n"
            );
            fixture.write("shared", original);
            assert_eq!(
                recover(&engine, &repo, proof, RemoteAction::AbortRebase)
                    .expect("valid abort")
                    .notice,
                RemoteNotice::RebaseAborted
            );
            assert_eq!(fixture.oid("HEAD"), ours);
        } else {
            fixture.write("shared", "resolved\n");
            fixture.git(["add", "--", "shared"]);
            assert_eq!(
                recover(&engine, &repo, proof, RemoteAction::ContinueRebase)
                    .expect("valid continue")
                    .notice,
                RemoteNotice::Integrated
            );
            assert_eq!(fixture.oid("HEAD^"), theirs);
            assert_eq!(
                fs::read(fixture.path().join("shared")).expect("preserved"),
                b"resolved\n"
            );
        }
        assert!(!fixture.path().join(".git/rebase-merge").exists());
    }
}

#[test]
fn remote_pull_merge_preserves_pending_merge_for_the_changes_workspace() {
    for conflict in [false, true] {
        let (fixture, engine, repo) = setup();
        let Some(server) = Server::start() else {
            return;
        };
        add(&engine, &repo, "origin", server.url("public"));
        run(
            &engine,
            &repo,
            RemoteAction::Upstream("origin".into(), "main".into()),
        );
        let (ours, theirs) = diverge(&fixture, &server, conflict);
        let result = execute(
            &engine,
            &repo,
            request(&engine, &repo, pull(Integration::Merge, server.auth())),
        )
        .expect("fetch and merge");
        assert_eq!(result.notice, RemoteNotice::MergePending);
        assert!(result.merge.is_some());
        assert_eq!(fixture.oid("HEAD"), ours);
        assert_eq!(fixture.oid("MERGE_HEAD"), theirs);
        assert_eq!(!fixture.git(["ls-files", "-u"]).stdout.is_empty(), conflict);
    }
}

#[test]
fn interrupted_fetch_never_publishes_tracking_refs() {
    let (fixture, engine, repo) = setup();
    let Some(server) = Server::start() else {
        return;
    };
    server.seed(&fixture, "slow");
    add(&engine, &repo, "origin", server.url("slow"));
    let action = || RemoteAction::Fetch(vec!["origin".into()], server.auth());
    let task = engine
        .submit(Request::Remote(
            repo.clone(),
            Box::new(request(&engine, &repo, action())),
        ))
        .expect("queued fetch");
    let start = Instant::now();
    while server.requests().is_empty() && start.elapsed() < Duration::from_secs(4) {
        thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(server.requests(), "");
    task.cancel();
    let report = task.wait().expect("cancel report");
    assert!(matches!(
        report.result,
        Ok(TaskValue::RemoteResult(RemoteResult {
            notice: RemoteNotice::Cancelled,
            ..
        })) | Err(GitError::Cancelled)
    ));
    assert!(!fixture.path().join(".git/refs/remotes/origin").exists());
    let deadline = super::engine(EngineOptions {
        timeout: Duration::from_secs(1),
        ..EngineOptions::default()
    });
    assert_eq!(run(&deadline, &repo, action()), RemoteNotice::Timeout);
    assert!(!fixture.path().join(".git/refs/remotes/origin").exists());
}

#[test]
fn push_keeps_signed_object_bytes_and_rebase_refuses_to_replace_them() {
    let (fixture, engine, repo) = setup();
    if !Path::new("/usr/bin/ssh-keygen").is_file() {
        eprintln!("signed Git fixture skipped: ssh-keygen unavailable");
        return;
    }
    let Some(server) = Server::start() else {
        return;
    };
    add(&engine, &repo, "origin", server.url("public"));
    run(
        &engine,
        &repo,
        RemoteAction::Upstream("origin".into(), "main".into()),
    );
    let (local, _) = diverge(&fixture, &server, false);
    let keys = tempfile::tempdir().expect("ephemeral signing key directory");
    let key = keys.path().join("signing-key");
    let output = Command::new("/usr/bin/ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .output()
        .expect("create ephemeral signing key");
    assert!(output.status.success());
    fixture.git([
        "-c",
        "gpg.format=ssh",
        "-c",
        &format!("user.signingkey={}", key.display()),
        "commit",
        "--allow-empty",
        "-S",
        "-m",
        "signed local work",
    ]);
    let signed = fixture.oid("HEAD");
    assert_ne!(signed, local);
    assert_eq!(
        run(&engine, &repo, pull(Integration::Rebase, server.auth())),
        RemoteNotice::SignedRebase
    );
    assert_eq!(fixture.oid("HEAD"), signed);
    assert_eq!(
        run(
            &engine,
            &repo,
            push("origin", "signed", false, server.auth())
        ),
        RemoteNotice::Pushed
    );
    let output = fixture_command(&server.directory.path().join("public.git"))
        .args(["cat-file", "commit", "refs/heads/signed"])
        .output()
        .expect("retrieve pushed object");
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        fixture.git(["cat-file", "commit", signed.as_str()]).stdout
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("BEGIN SSH SIGNATURE"));
}
