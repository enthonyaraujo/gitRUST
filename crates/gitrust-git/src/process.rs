use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use process_wrap::std::{ChildWrapper, CommandWrap};

use crate::{GitError, OutputStream, ProcessOutput};

const POLL: Duration = Duration::from_millis(8);

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProcessLimits {
    pub(crate) stdout: usize,
    pub(crate) stderr: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct Control<'a> {
    pub(crate) deadline: Instant,
    pub(crate) cancelled: &'a AtomicBool,
    pub(crate) shutdown: &'a AtomicBool,
}

impl Control<'_> {
    pub(crate) fn check(self) -> Result<(), GitError> {
        if self.cancelled.load(Ordering::Acquire) || self.shutdown.load(Ordering::Acquire) {
            return Err(GitError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(GitError::Timeout);
        }
        Ok(())
    }
}

pub(crate) const NULL_PATH: &str = "/dev/null";

fn command(executable: &Path, cwd: &Path, args: &[OsString], input: &[u8]) -> Command {
    let mut cmd = Command::new(executable);
    cmd.env_clear()
        .current_dir(cwd)
        .args(args)
        .stdin(if input.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", NULL_PATH)
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .env("LC_ALL", "C");
    #[cfg(windows)]
    for key in ["SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key).filter(|v| Path::new(v).is_absolute()) {
            cmd.env(key, value);
        }
    }
    cmd
}

fn collect(stream: impl Read, limit: usize, which: OutputStream) -> Result<Vec<u8>, GitError> {
    let mut data = Vec::new();
    stream
        .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
        .read_to_end(&mut data)
        .map_err(GitError::io)?;
    if data.len() > limit {
        return Err(GitError::OutputLimitExceeded(which));
    }
    Ok(data)
}

fn stop(child: &mut dyn ChildWrapper) {
    let _ = child.start_kill();
    let _ = child.wait();
}

enum PipeMessage {
    Output(OutputStream, Result<Vec<u8>, GitError>),
    Input(io::Result<()>),
}

fn supervise(
    child: &mut dyn ChildWrapper,
    receiver: &mpsc::Receiver<PipeMessage>,
    control: Control<'_>,
    start: Instant,
    mut input: Option<io::Result<()>>,
) -> Result<(ProcessOutput, io::Result<()>), GitError> {
    let mut stdout = None;
    let mut stderr = None;
    loop {
        control.check()?;
        if stdout.is_some()
            && stderr.is_some()
            && input.is_some()
            && let Some(status) = child.try_wait().map_err(GitError::io)?
        {
            return Ok((
                ProcessOutput {
                    status,
                    duration: start.elapsed(),
                    stdout: stdout.take().unwrap_or_default(),
                    stderr: stderr.take().unwrap_or_default(),
                },
                input.take().unwrap_or(Ok(())),
            ));
        }
        let remaining = control.deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(POLL.min(remaining)) {
            Ok(PipeMessage::Output(OutputStream::Stdout, data)) => stdout = Some(data?),
            Ok(PipeMessage::Output(OutputStream::Stderr, data)) => stderr = Some(data?),
            Ok(PipeMessage::Input(result)) => input = Some(result),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected)
                if stdout.is_some() && stderr.is_some() && input.is_some() =>
            {
                thread::sleep(POLL.min(remaining));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(GitError::Io(io::Error::other(
                    "Git output reader ended unexpectedly",
                )));
            }
        }
    }
}

pub(crate) fn run(
    executable: &Path,
    cwd: &Path,
    args: &[OsString],
    input: &[u8],
    limits: ProcessLimits,
    control: Control<'_>,
) -> Result<ProcessOutput, GitError> {
    control.check()?;
    let start = Instant::now();
    let mut wrapped = CommandWrap::from(command(executable, cwd, args, input));
    #[cfg(unix)]
    wrapped.wrap(process_wrap::std::ProcessGroup::leader());
    #[cfg(windows)]
    wrapped.wrap(process_wrap::std::JobObject);
    let mut child = wrapped.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::PermissionDenied {
            GitError::PermissionDenied
        } else {
            GitError::SpawnFailed(error)
        }
    })?;
    let streams = child.stdout().take().zip(child.stderr().take());
    let Some((stdout, stderr)) = streams else {
        stop(&mut *child);
        return Err(GitError::Io(io::Error::other(
            "Git output pipes unavailable",
        )));
    };
    let input_pipe = child.stdin().take();
    thread::scope(|scope| {
        let result = (|| {
            let (sender, receiver) = mpsc::channel();
            let out_sender = sender.clone();
            thread::Builder::new()
                .name("gitrust-stdout".into())
                .spawn_scoped(scope, move || {
                    let result = collect(stdout, limits.stdout, OutputStream::Stdout);
                    let _ = out_sender.send(PipeMessage::Output(OutputStream::Stdout, result));
                })
                .map_err(GitError::SpawnFailed)?;
            let err_sender = sender.clone();
            thread::Builder::new()
                .name("gitrust-stderr".into())
                .spawn_scoped(scope, move || {
                    let result = collect(stderr, limits.stderr, OutputStream::Stderr);
                    let _ = err_sender.send(PipeMessage::Output(OutputStream::Stderr, result));
                })
                .map_err(GitError::SpawnFailed)?;
            let input_done = input_pipe.is_none().then_some(Ok(()));
            if let Some(mut pipe) = input_pipe {
                let input_sender = sender.clone();
                thread::Builder::new()
                    .name("gitrust-stdin".into())
                    .spawn_scoped(scope, move || {
                        let result = pipe.write_all(input);
                        drop(pipe);
                        let _ = input_sender.send(PipeMessage::Input(result));
                    })
                    .map_err(GitError::SpawnFailed)?;
            }
            drop(sender);
            supervise(&mut *child, &receiver, control, start, input_done)
        })();
        match result {
            Ok((output, input)) => {
                if output.status.success() {
                    input.map_err(GitError::io)?;
                }
                Ok(output)
            }
            Err(error) => {
                stop(&mut *child);
                Err(error)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const BIG: &[u8] = &[b'x'; 256 * 1024];

    fn fixture_args(name: &str) -> Vec<OsString> {
        ["--ignored", "--exact", name, "--nocapture"]
            .map(OsString::from)
            .into()
    }

    fn fixture(
        name: &str,
        dir: &Path,
        input: &[u8],
        limits: ProcessLimits,
        timeout: Duration,
        cancelled: &AtomicBool,
    ) -> Result<ProcessOutput, GitError> {
        let shutdown = AtomicBool::new(false);
        let bin = std::env::current_exe().expect("fixture executable");
        run(
            &bin,
            dir,
            &fixture_args(name),
            input,
            limits,
            Control {
                deadline: Instant::now() + timeout,
                cancelled,
                shutdown: &shutdown,
            },
        )
    }

    #[test]
    #[ignore = "invoked by the stream tests as a child process"]
    fn fixture_streams() {
        let stderr = thread::spawn(|| io::stderr().lock().write_all(BIG).expect("fixture stderr"));
        io::stdout().lock().write_all(BIG).expect("fixture stdout");
        stderr.join().expect("fixture writer");
    }

    #[test]
    #[ignore = "invoked with a private piped stdin by its parent test"]
    fn fixture_input() {
        let mut buffer = Vec::new();
        io::stdin()
            .lock()
            .read_to_end(&mut buffer)
            .expect("fixture stdin");
        assert_eq!(buffer, BIG);
        io::stdout()
            .lock()
            .write_all(b"received private input")
            .expect("fixture stdout");
    }

    #[test]
    #[ignore = "invoked by the process lifetime tests as a child process"]
    fn fixture_sleep() {
        std::fs::write("started", b"yes").expect("fixture marker");
        thread::sleep(Duration::from_secs(10));
    }

    #[test]
    #[ignore = "invoked to keep inherited pipes open after the parent exits"]
    fn fixture_descendant() {
        let child = Command::new(std::env::current_exe().expect("fixture binary"))
            .args(fixture_args("process::tests::fixture_sleep"))
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(Stdio::null())
            .spawn()
            .expect("fixture grandchild");
        // The test supervisor owns the lifetime of this process tree.
        let _child = child;
    }

    #[test]
    fn drains_both_pipes_supports_private_input_and_limits_each_stream() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let cancel = AtomicBool::new(false);
        let generous = ProcessLimits {
            stdout: BIG.len() * 2,
            stderr: BIG.len() * 2,
        };
        let run = |name, input, limits| {
            fixture(
                name,
                dir.path(),
                input,
                limits,
                Duration::from_secs(5),
                &cancel,
            )
        };
        let raw = run("process::tests::fixture_streams", &[], generous).expect("successful output");
        assert!(raw.status.success());
        assert!(raw.stdout.windows(BIG.len()).any(|window| window == BIG));
        assert_eq!(raw.stderr, BIG);
        let private =
            run("process::tests::fixture_input", BIG, generous).expect("successful input");
        let marker = b"received private input";
        assert!(
            private
                .stdout
                .windows(marker.len())
                .any(|window| window == marker)
        );
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let mut limits = generous;
            match stream {
                OutputStream::Stdout => limits.stdout = 8192,
                OutputStream::Stderr => limits.stderr = 8192,
            }
            assert!(
                matches!(run("process::tests::fixture_streams", &[], limits),
                Err(GitError::OutputLimitExceeded(which)) if which == stream)
            );
        }
    }

    #[test]
    fn deadline_terminates_descendants_with_inherited_pipes() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        let result = fixture(
            "process::tests::fixture_descendant",
            dir.path(),
            &[],
            ProcessLimits {
                stdout: 16384,
                stderr: 16384,
            },
            Duration::from_secs(2),
            &cancel,
        );
        assert!(matches!(result, Err(GitError::Timeout)));
        assert!(
            dir.path().join("started").exists(),
            "grandchild entered its wait"
        );
        assert!(
            start.elapsed() < Duration::from_secs(7),
            "descendant did not retain the pipes"
        );
    }

    #[test]
    fn an_active_read_is_cancelled_and_reaped() {
        let dir = tempfile::tempdir().expect("fixture directory");
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        thread::scope(|scope| {
            let signal = scope.spawn(|| {
                let bound = Instant::now() + Duration::from_secs(2);
                while !dir.path().join("started").exists() && Instant::now() < bound {
                    thread::sleep(Duration::from_millis(5));
                }
                cancel.store(true, Ordering::Release);
            });
            let result = fixture(
                "process::tests::fixture_sleep",
                dir.path(),
                &[],
                ProcessLimits {
                    stdout: 16384,
                    stderr: 16384,
                },
                Duration::from_secs(6),
                &cancel,
            );
            signal.join().expect("cancellation signal sent");
            assert!(matches!(result, Err(GitError::Cancelled)));
            assert!(dir.path().join("started").exists(), "cancelled after spawn");
            assert!(start.elapsed() < Duration::from_secs(5));
        });
    }
}
