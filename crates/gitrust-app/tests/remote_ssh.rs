#![cfg(target_os = "linux")]
#![allow(clippy::expect_used)]

use std::{
    io::{BufRead, BufReader},
    path::Path,
    process::{Child, Command, Stdio},
};

use gitrust_core::remotes::{Auth, RemoteNotice};
use gitrust_git::{
    CloneRepository, EngineOptions, GitEngine, GitError, GitExecutable, RemoteEndpoint, Request,
    TaskValue,
};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        let _ = self.0.wait();
    }
}

#[test]
fn cloned_ssh_repository_requires_a_known_host_and_authorized_identity() {
    if !Path::new("/usr/bin/python3").is_file() {
        eprintln!("loopback SSH fixture skipped: Python unavailable");
        return;
    }
    let root = tempfile::tempdir().expect("disposable SSH fixture");
    let work = root.path().join("project.git");
    let git = GitExecutable::discover().expect("Git 2.45+");
    let status = Command::new(git.path())
        .env_clear()
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["init", "--quiet", "--initial-branch=main", "--bare"])
        .arg(&work)
        .status()
        .expect("Git initialization");
    assert!(status.success());
    let mut server = Server(
        Command::new("/usr/bin/python3")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/ssh_server.py"))
            .arg(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("OpenSSH fixture"),
    );
    let mut address = String::new();
    BufReader::new(server.0.stdout.take().expect("fixture stdout"))
        .read_line(&mut address)
        .expect("read SSH fixture address");
    if address.is_empty() {
        let exit = server.0.wait().expect("SSH fixture status");
        assert_eq!(exit.code(), Some(77));
        return;
    }
    let (user, port) = address.trim().split_once(' ').expect("user and port");
    let path = work.to_str().expect("ASCII fixture path");
    let engine = GitEngine::new(git, EngineOptions::default()).expect("Git engine");
    let clone = |name, known, identity| {
        let endpoint = RemoteEndpoint::new(&format!("ssh://{user}@127.0.0.1:{port}{path}"))
            .expect("valid SSH endpoint");
        let auth = Auth {
            known_hosts: Some(root.path().join(known)),
            identity: Some(root.path().join(identity)),
            ..Auth::default()
        };
        let clone = CloneRepository::remote(
            root.path().join(name),
            endpoint,
            auth,
            Some(env!("CARGO_BIN_EXE_gitrust").into()),
        )
        .expect("authorized clone request");
        let result = engine
            .submit(Request::Clone(clone))
            .expect("clone task")
            .wait()
            .expect("clone report");
        assert!(result.processes.is_empty());
        result.result
    };
    assert!(matches!(
        clone("unknown-host", "unknown", "identity"),
        Err(GitError::Remote(RemoteNotice::HostKey))
    ));
    assert!(matches!(
        clone("changed-host", "known_wrong", "identity"),
        Err(GitError::Remote(RemoteNotice::ChangedHostKey))
    ));
    assert!(matches!(
        clone("wrong-identity", "known_host", "wrong"),
        Err(GitError::Remote(
            RemoteNotice::Agent | RemoteNotice::Authentication
        ))
    ));
    let result = clone("authorized", "known_host", "identity");
    assert!(
        matches!(result, Ok(TaskValue::Provisioned(_))),
        "{result:?}"
    );
    assert!(root.path().join("authorized/.git").is_dir());
}
