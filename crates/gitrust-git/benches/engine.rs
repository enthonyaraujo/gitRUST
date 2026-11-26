//! Opt-in release benchmarks against isolated synthetic repositories.
#![allow(clippy::expect_used)]

use std::{
    fs,
    hint::black_box,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use gitrust_git::{
    DiffTarget, EngineOptions, GitEngine, GitExecutable, HistoryPage, Inspection, RepositoryDiff,
    RepositoryStatus, Request, TaskReport, TaskValue, TrustedRepository,
};
use tempfile::TempDir;

const RUNS: usize = 21;
const STATUS: &[&str] = &[
    "status",
    "--porcelain=v2",
    "-z",
    "--branch",
    "--ahead-behind",
    "--untracked-files=all",
    "--ignore-submodules=all",
    "--find-renames",
    "--no-ignored",
];
const DIFF: &[&str] = &[
    "diff",
    "--raw",
    "-z",
    "--patch",
    "--abbrev=40",
    "--full-index",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-relative",
    "--ignore-submodules=all",
    "--find-renames",
    "--diff-filter=u",
    "--no-indent-heuristic",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--diff-algorithm=myers",
    "--exit-code",
    "--ours",
    "--",
];

fn cli(executable: &GitExecutable, cwd: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(executable.path());
    command
        .current_dir(cwd)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--no-lazy-fetch",
            "--no-replace-objects",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "core.quotePath=true",
            "-c",
            "protocol.allow=never",
            "-c",
            "user.name=Benchmark",
            "-c",
            "user.email=bench@example.invalid",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args);
    #[cfg(windows)]
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
}

fn direct(executable: &GitExecutable, cwd: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut cmd = cli(executable, cwd, args);
    let output = if input.is_empty() {
        cmd.output().expect("Git benchmark command")
    } else {
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Git benchmark process");
        child
            .stdin
            .take()
            .expect("benchmark Git input")
            .write_all(input)
            .expect("write benchmark input");
        child.wait_with_output().expect("Git benchmark result")
    };
    assert!(
        output.status.success()
            || (args.first() == Some(&"diff") && output.status.code() == Some(1)),
        "benchmark CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn new_repo(executable: &GitExecutable) -> TempDir {
    let dir = tempfile::tempdir().expect("benchmark fixture directory");
    direct(
        executable,
        dir.path(),
        &[
            "init",
            "--quiet",
            "--object-format=sha1",
            "--initial-branch=main",
        ],
        &[],
    );
    dir
}

fn trust(engine: &GitEngine, root: &Path) -> TrustedRepository {
    let outcome = engine
        .submit(Request::Identify(root.into()))
        .expect("enqueue fixture discovery")
        .wait()
        .expect("receive fixture identification")
        .result
        .expect("identify fixture");
    let TaskValue::Identified(candidate) = outcome else {
        panic!("expected fixture candidate");
    };
    candidate.authorize_local_metadata()
}

fn run(engine: &GitEngine, repo: &TrustedRepository, operation: Inspection) -> TaskReport {
    let report = engine
        .submit(Request::Inspect(repo.clone(), operation))
        .expect("submit benchmark request")
        .wait()
        .expect("receive benchmark result");
    assert!(report.result.is_ok(), "benchmark engine failed: {report:?}");
    report
}

fn sample(label: &str, units: usize, mut operation: impl FnMut()) {
    for _ in 0..2 {
        operation();
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    let percentile = |value: usize| samples[(samples.len() * value).div_ceil(100_usize) - 1];
    let unit_count = f64::from(u32::try_from(units).expect("units fit in u32"));
    let ms = |value: Duration| value.as_secs_f64() * 1_000.0;
    println!(
        "{label},{units},{RUNS},{:.3},{:.3},{:.3},{:.3},{:.3},{:.1},{}",
        ms(samples[0]),
        ms(percentile(50)),
        ms(percentile(95)),
        ms(percentile(99)),
        ms(samples[RUNS - 1]),
        unit_count / percentile(50).as_secs_f64(),
        rss_kib()
    );
}

fn rss_kib() -> String {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|body| {
            body.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .and_then(|value| value.split_whitespace().next())
                    .map(str::to_owned)
            })
        })
        .unwrap_or_else(|| "NA".into())
}

fn status_benchmarks(executable: &GitExecutable, engine: &GitEngine) {
    let operation = Inspection::Status {
        include_ignored: false,
    };
    for count in [100, 1_000, 10_000, 100_000] {
        let fixture = new_repo(executable);
        let root = fixture.path();
        for group in 0..count / 100 {
            let path = root.join(format!("folder-{group:04}"));
            fs::create_dir(&path).expect("fixture folder");
            for index in 0..100 {
                fs::write(path.join(format!("file-{index:02}")), b"tracked\n")
                    .expect("benchmark file");
            }
        }
        direct(executable, root, &["add", "--all", "--"], &[]);
        direct(
            executable,
            root,
            &["commit", "--quiet", "-m", "tracked files"],
            &[],
        );
        for group in 0..count / 100 {
            fs::write(
                root.join(format!("folder-{group:04}/file-00")),
                b"changed\n",
            )
            .expect("modify one percent");
        }
        let repo = trust(engine, root);
        sample(&format!("cli_status_{count}"), 1, || {
            black_box(direct(executable, root, STATUS, &[]));
        });
        sample(&format!("engine_status_{count}"), 1, || {
            black_box(run(engine, &repo, operation.clone()));
        });

        let mut synthetic = b"# branch.oid (initial)\0# branch.head main\0".to_vec();
        let empty_oid = "0".repeat(40);
        for index in 0..count {
            write!(
                synthetic,
                "1 .M N... 100644 100644 100644 {empty_oid} {empty_oid} folder/sample-{index}\0"
            )
            .expect("write porcelain fixture");
        }
        sample(&format!("parser_porcelain_{count}"), count, || {
            black_box(RepositoryStatus::parse(&synthetic).expect("parse synthetic porcelain"));
        });
    }
}

fn diff_benchmarks(executable: &GitExecutable, engine: &GitEngine) {
    let fixture = new_repo(executable);
    let root = fixture.path();
    let path = root.join("long.txt");
    let body = |prefix| {
        let mut bytes = Vec::new();
        for line in 0..50_000 {
            writeln!(bytes, "{prefix} {line:07} example text with spaces")
                .expect("write diff fixture");
        }
        bytes
    };
    fs::write(&path, body("before")).expect("base diff file");
    direct(executable, root, &["add", "--", "long.txt"], &[]);
    direct(
        executable,
        root,
        &["commit", "--quiet", "-m", "base diff"],
        &[],
    );
    fs::write(&path, body("after")).expect("new diff file");
    let raw = direct(executable, root, DIFF, &[]).stdout;
    let repo = trust(engine, root);
    sample("cli_diff_100000_lines", 1, || {
        black_box(direct(executable, root, DIFF, &[]));
    });
    sample("engine_diff_100000_lines", 1, || {
        black_box(run(
            engine,
            &repo,
            Inspection::Diff {
                target: DiffTarget::Worktree,
                path: None,
            },
        ));
    });
    sample("parser_diff_100000_lines", 100_000, || {
        black_box(RepositoryDiff::parse(&raw).expect("parse real patch"));
    });
}

fn history_benchmarks(executable: &GitExecutable, engine: &GitEngine) {
    let fixture = new_repo(executable);
    let root = fixture.path();
    let mut stream = Vec::new();
    for index in 0..3_000 {
        write!(stream, "commit refs/heads/main\ncommitter Bench <bench@example.invalid> {} +0000\ndata 5\nentry\n\n",
            1_700_000_000 + index).expect("write history fixture");
    }
    stream.extend_from_slice(b"done\n");
    direct(
        executable,
        root,
        &["fast-import", "--quiet", "--done"],
        &stream,
    );
    let repo = trust(engine, root);
    let head = direct(
        executable,
        root,
        &["rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"],
        &[],
    );
    let head = std::str::from_utf8(&head.stdout).expect("object ID").trim();
    let page = HistoryPage::new(2_000, 50).expect("valid history page");
    sample("cli_history_3000_page_50", 1, || {
        let ids = direct(
            executable,
            root,
            &[
                "rev-list",
                "--max-count=51",
                "--skip=2000",
                "--end-of-options",
                head,
                "--",
            ],
            &[],
        );
        let sample = &ids.stdout[..50 * 41];
        black_box(direct(executable, root, &["cat-file", "--batch"], sample));
    });
    sample("engine_history_3000_page_50", 1, || {
        black_box(run(engine, &repo, Inspection::History(page)));
    });
}

fn throughput(executable: &GitExecutable) {
    let fixture = new_repo(executable);
    for workers in [1, 2, 4] {
        let engine = GitEngine::new(
            executable.clone(),
            EngineOptions {
                workers,
                ..EngineOptions::default()
            },
        )
        .expect("worker batch");
        let repo = trust(&engine, fixture.path());
        sample(
            &format!("engine_status_batch_workers_{workers}"),
            32,
            || {
                let tasks: Vec<_> = (0..32)
                    .map(|_| {
                        engine
                            .submit(Request::Inspect(
                                repo.clone(),
                                Inspection::Status {
                                    include_ignored: false,
                                },
                            ))
                            .expect("enqueue benchmark batch")
                    })
                    .collect();
                for task in tasks {
                    let result = task.wait().expect("batch result");
                    assert!(result.result.is_ok(), "{result:?}");
                    black_box(result);
                }
            },
        );
    }
}

fn main() {
    let executable = GitExecutable::discover().expect("native Git 2.45+ on trusted PATH");
    let engine =
        GitEngine::new(executable.clone(), EngineOptions::default()).expect("benchmark workers");
    let dir = executable.path().parent().expect("Git directory");
    println!(
        "# os={} arch={} git={} logical_cpus={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        executable.version(),
        std::thread::available_parallelism().expect("CPU count")
    );
    println!(
        "case,units,samples,min_ms,p50_ms,p95_ms,p99_ms,max_ms,units_per_second_at_p50,self_rss_kib"
    );
    sample("cli_version", 1, || {
        black_box(direct(&executable, dir, &["--version"], &[]));
    });
    sample("engine_version_validation", 1, || {
        drop(black_box(
            GitExecutable::from_absolute(executable.path()).expect("verify Git version"),
        ));
    });
    status_benchmarks(&executable, &engine);
    diff_benchmarks(&executable, &engine);
    history_benchmarks(&executable, &engine);
    throughput(&executable);
}
