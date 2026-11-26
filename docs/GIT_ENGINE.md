# Stage 02 Git Engine

The `gitrust-git` crate inspects local repositories with a native system Git 2.45.0 or newer. It is independent of egui, persistence, providers and credential handling. [ADR-0007](adr/0007-local-git-inspection.md) records the runtime floor and trust boundary.

## Public contract

| Request | Result | Supported input |
| --- | --- | --- |
| `Request::Identify(path)` | `RepositoryCandidate` | Absolute or process-relative directory, including subdirectories, normal working trees, bare repositories and standard linked worktrees. Uses filesystem metadata only. |
| `Inspection::Info` | `RepositoryInfo` | Canonical worktree, Git and common Git directories, attached/detached/unborn HEAD and storage object format. |
| `Inspection::Status` | `RepositoryStatus` | Index and worktree status, ignored entries on request, branch headers, rename/copy and conflict-stage data from porcelain v2. Requires a working tree. |
| `Inspection::Diff` | `RepositoryDiff` | Worktree vs index, index vs HEAD (including unborn), or two complete commit IDs. Optionally restrict to a literal repository-relative path. Bare repositories support commit-to-commit diff. |
| `Inspection::History` | `History` | 1–500 commits at an offset, starting at the HEAD observed for that request. Parents, author, committer, timestamps, subject and complete message bytes. |
| `Inspection::Branches` | `BranchList` | Local and remote-tracking refs, current branch, configured upstream and HEAD state. |

Discover Git at startup on a background thread with `GitExecutable::discover()`, or provide an administrator-selected absolute path to `from_absolute()`. Both inspect a native executable and obtain its actual version. Discovery examines absolute PATH entries only. It does not install or update Git. An invalid first matching executable is reported. Git silently supplied through a relative working-directory lookup is ineligible.

Obtain the candidate first, present its canonical location, and ask the user to authorize local metadata before calling `authorize_local_metadata()`. Trust is scoped to that candidate and does not grant permission to run extensions. The engine rechecks metadata on every inspection.

```rust
use gitrust_git::{EngineOptions, GitEngine, GitExecutable, Inspection, Request, TaskValue};
use std::{error::Error, path::PathBuf, sync::mpsc};

fn inspect_approved_path(selected: PathBuf) -> Result<(), Box<dyn Error>> {
    let engine = GitEngine::new(GitExecutable::discover()?, EngineOptions::default())?;
    let identified = engine.submit(Request::Identify(selected))?.wait()?.result?;
    let TaskValue::Identified(candidate) = identified else {
        return Err("expected a repository candidate".into());
    };

    // The caller has obtained explicit authorization for this candidate.
    let approved = candidate.authorize_local_metadata();
    let (completed, notifications) = mpsc::channel();
    let task = engine.submit_notifying(
        Request::Inspect(approved, Inspection::Status { include_ignored: false }),
        completed,
    )?;
    // This function runs on a dispatcher thread. A GUI wakes on this notification.
    assert_eq!(notifications.recv()?, task.id());
    if let Some(report) = task.try_take() {
        let status = report.result?;
        let _ = status; // Hand the typed value to the view model.
    }
    engine.shutdown();
    Ok(())
}
```

`GitTask` exposes its ID, state, cancellation and a one-shot result. Use `submit_notifying` to dispatch completion to a UI event loop and `try_take` to read the delivered report. Use `wait` on a background thread or in tests. The engine creates no UI timers.

## Execution and limits

- Defaults are two workers, 64 queued requests, a 30-second active deadline, 32 MiB stdout and 256 KiB stderr per command. Options admit up to 32 workers and 4,096 queued requests. Queue time is excluded from the deadline. Configuration preflight has a separate 1 MiB file and 2 MiB output cap.
- The engine bounds decoded models to 250,000 status entries, refs or diff files, plus 500,000 lines per patch. An output or decoded-item limit fails the task. Callers receive no partial typed result.
- Workers sleep on a condition variable when idle. Read requests can share a repository. Both write classes reserve exclusive access by canonical common Git directory, and earlier writes block later conflicting readers. The Stage 02 inspection allowlist contains reads. The Stage 03 write requests provision separate destinations. The ordering applies within one engine instance. External Git processes can change repository state.
- Queued cancellations immediately free a slot. Active reads end through a Unix process group or a Windows Job Object, and workers drain or close their pipes and collect the child. The supervisor checks cancellation while a child is active. Shutdown cancels pending and active work and joins the workers. Call shutdown off the GUI thread.
- Deadlines are cooperative during filesystem inspection and decoding. OS filesystem or process cleanup calls can complete after a deadline. Future writes need operation-specific safe points and state reconciliation if an interruption leaves the outcome unknown. The engine never removes Git lockfiles.
- `TaskReport` records elapsed time, process exit codes, raw stdout and raw stderr for result-producing commands. Process `Debug` output is redacted. Treat repository values and both byte streams as private. Config preflight output is kept internal.

## Formats and behavior

- Object IDs are validated complete SHA-1 or SHA-256 values. The 12-character abbreviation is for display. No raw command, revision expression, arbitrary flag or transport is exposed by the public API.
- Status uses `status --porcelain=v2 -z --branch` with deterministic switches. NUL frames paths, including rename sources. Decoded paths are repository-relative. Unix paths round-trip without UTF-8 conversion. Windows conversion requires UTF-8 and rejects backslashes and colon syntax within a Git path.
- Diff uses `diff --raw -z --patch`; the raw records establish byte-exact identity and the patch supplies the content. Git exit 1 is successful for this operation. No-newline markers apply to the preceding line. Binary diffs contain metadata only. Unmerged index records are available in status. The worktree diff uses stage two for an unmerged path when Git can generate a two-way comparison. Combined patches are outside the contract.
- History snapshots the HEAD ID, calls `rev-list` with the page limit plus one, then sends a bounded list of IDs to `cat-file --batch`. Declared commit sizes frame arbitrary message bytes. The `root` identifies the snapshot for each request. Separate offset requests can reflect different HEADs.
- Refs use NUL-separated fields per `for-each-ref` record. The restricted reference syntax excludes control characters. Absent, malformed and missing-object references have different consequences for HEAD resolution.

## Security and compatibility

The engine accepts stable, user-controlled local metadata and treats repository content as data. It inspects config files without includes before using the repository. A narrow configuration allowlist blocks includes, hooks configuration, filter and diff drivers, filesystem monitors, aliases, pagers and worktree redirection. Object-store symlinks, alternate sources, partial clones, shallow boundaries, reftable and nonstandard gitfiles are refused. Submodule recursion and gitlink differences are disabled. Conventional local clones and standard linked worktrees are supported when their metadata passes policy.

Git's ownership checks remain active. No `safe.directory` override is injected. The engine clears inherited Git, home, pager, credential and transport variables. On Windows it preserves only absolute OS-root variables. It disables optional locks, replacement refs, global/system configuration, system/user attributes, prompts, lazy fetch and all Git transports. Direct UNC/device repository paths are refused on Windows. Use local filesystems. Git and the filesystem remain trusted execution dependencies, and this policy cannot isolate OS-level vulnerabilities, remote mounts or adversarial concurrent metadata replacement.

Policy rejection, absent or incompatible Git, permissions, invalid references, execution failure, timeout, cancellation, resource limits and malformed output have typed errors. Selected existing locks are surfaced before Git starts. Other command failures preserve their exit status and bytes for private diagnosis. The trust policy and future remote-operation boundaries are detailed in [Security Architecture](SECURITY_ARCHITECTURE.md).
## Stage 02 validation

Stage 02 was integrated into develop in merge commit `dc7c28a`. The implementation PR passed format and Clippy, MSRV 1.97.1, the three native platform jobs, and dependency and license auditing. Native suites passed 66 tests on Linux and macOS and 64 on Windows. The Windows count excludes two Unix filesystem fixtures that rely on Unix path bytes or symlink APIs. Its own tests cover Windows path conversion and Job Object cancellation. [PR #4](https://github.com/enthonyaraujo/gitRUST/pull/4) · [PR checks](https://github.com/enthonyaraujo/gitRUST/actions/runs/45816951711) · [Implementation merge checks](https://github.com/enthonyaraujo/gitRUST/actions/runs/45818673861)

The 16 local integration tests passed with the system Git 2.56.1 and with Git 2.45.0 built separately on Linux. A Linux syscall trace recorded no network calls in those disposable repository fixtures. The portability fixtures verify Git inspection on each platform and exercise the paths that platform can represent. [Reproducing the floor check](BUILD.md#git-integration-tests-and-hosted-runners)


## Stage 03 operations

`Inspection::Workspace` returns HEAD and optional worktree status from one preflight. `Request::Create` prepares an unborn repository in a new directory. `Request::Clone` accepts a typed public HTTPS source and optional bounded progress observer. Both return a validated candidate for explicit session authorization. The engine can cancel before atomic publication and does not replace an existing destination. Clone and materialization use separate network and local policies respectively. [Repository management](REPOSITORY_MANAGEMENT.md)
