//! Linux single-instance lifetime and XDG URL activation.

use std::{
    fs::{self, DirBuilder, File, OpenOptions, Permissions, TryLockError},
    io::{self, Read as _, Write as _},
    net::Shutdown,
    os::unix::{
        fs::{
            DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _,
            PermissionsExt as _,
        },
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use gitrust_core::github::Notice;
use gitrust_providers::github::{Callback, MAX_CALLBACK_BYTES};
use rustix::{fs::OFlags, net::sockopt::socket_peercred, process::getuid};

use super::Inbox;

const HANDLER: &str = "io.github.enthonyaraujo.gitRUST.OAuth.desktop";
const MIME: &str = "x-scheme-handler/gitrust";
const IO_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) struct Instance {
    inbox: Inbox,
    socket: PathBuf,
    worker: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    lock: File,
}

impl Instance {
    pub(crate) fn inbox(&self) -> Inbox {
        self.inbox.clone()
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = UnixStream::connect(&self.socket);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.socket);
        let _ = self.lock.unlock();
    }
}

fn invalid() -> io::Error {
    io::ErrorKind::PermissionDenied.into()
}

fn private_dir(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != getuid().as_raw() || meta.mode() & 0o077 != 0 {
        return Err(invalid());
    }
    Ok(())
}

fn runtime() -> io::Result<PathBuf> {
    let root = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(invalid)?;
    private_dir(&root)?;
    let directory = root.join("gitrust");
    match DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    private_dir(&directory)?;
    Ok(directory)
}

pub(crate) fn start() -> io::Result<Option<Instance>> {
    start_at(&runtime()?)
}

fn start_at(directory: &Path) -> io::Result<Option<Instance>> {
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits().cast_signed())
        .open(directory.join("instance.lock"))?;
    let metadata = lock.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != getuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid());
    }
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(error)) => return Err(error),
    }
    let socket = directory.join("callback.sock");
    match fs::symlink_metadata(&socket) {
        Ok(meta) if meta.file_type().is_socket() && meta.uid() == getuid().as_raw() => {
            fs::remove_file(&socket)?;
        }
        Ok(_) => return Err(invalid()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&socket)?;
    let mut instance = Instance {
        inbox: Inbox::default(),
        socket,
        worker: None,
        stop: Arc::new(AtomicBool::new(false)),
        lock,
    };
    fs::set_permissions(&instance.socket, Permissions::from_mode(0o600))?;
    let stop = instance.stop.clone();
    let inbox = instance.inbox();
    instance.worker = Some(
        thread::Builder::new()
            .name("gitrust-oauth-ipc".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    match stream {
                        Ok(mut stream) => {
                            let accepted =
                                receive(&mut stream).is_ok_and(|callback| inbox.route(callback));
                            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                            let _ = stream.write_all(&[u8::from(accepted)]);
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
            })?,
    );
    Ok(Some(instance))
}

fn verify_peer(stream: &UnixStream) -> io::Result<()> {
    if socket_peercred(stream)?.uid == getuid() {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn receive(stream: &mut UnixStream) -> io::Result<Callback> {
    verify_peer(stream)?;
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut bytes = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > MAX_CALLBACK_BYTES {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let uri = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    Callback::parse(uri).map_err(|_| invalid())
}

pub(crate) fn forward(uri: &str) -> bool {
    Callback::parse(uri).is_ok()
        && runtime()
            .and_then(|directory| send(&directory, uri))
            .unwrap_or(false)
}

fn send(directory: &Path, uri: &str) -> io::Result<bool> {
    let path = directory.join("callback.sock");
    let meta = fs::symlink_metadata(&path)?;
    if !meta.file_type().is_socket() || meta.uid() != getuid().as_raw() || meta.mode() & 0o077 != 0
    {
        return Err(invalid());
    }
    let mut stream = UnixStream::connect(path)?;
    verify_peer(&stream)?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.set_read_timeout(Some(IO_TIMEOUT + IO_TIMEOUT))?;
    stream.write_all(uri.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;
    let mut reply = [0];
    stream.read_exact(&mut reply)?;
    Ok(reply == [1])
}

fn desktop_entry(executable: &str) -> Option<String> {
    // Desktop Entry strings and Exec quoting apply two levels of escaping.
    if executable.chars().any(|c| c.is_control() || c == '%') {
        return None;
    }
    let mut quoted = String::new();
    for character in executable.chars() {
        match character {
            '\\' => quoted.push_str(r"\\\\"),
            '"' | '$' | '`' => {
                quoted.push_str(r"\\");
                quoted.push(character);
            }
            _ => quoted.push(character),
        }
    }
    Some(format!(
        "[Desktop Entry]\nType=Application\nName=gitRUST OAuth\nNoDisplay=true\nExec=\"{quoted}\" --oauth-callback %u\nMimeType={MIME};\n"
    ))
}

pub(crate) fn register() -> Result<(), Notice> {
    register_handler().map_err(|_| Notice::OAuthHandler)
}

fn register_handler() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let content = executable
        .to_str()
        .and_then(desktop_entry)
        .ok_or_else(invalid)?;
    let directory = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .filter(|path| path.is_absolute())
        .ok_or_else(invalid)?
        .join("applications");
    fs::create_dir_all(&directory)?;
    let file = directory.join(HANDLER);
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    temporary.write_all(content.as_bytes())?;
    temporary.persist(file)?;
    let status = Command::new("/usr/bin/gio")
        .args(["mime", MIME, HANDLER])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitrust_providers::github::Cancellation;

    #[test]
    fn socket_forwards_to_original_and_refuses_a_second_instance() {
        let dir = tempfile::tempdir().expect("test runtime");
        let root = dir.path();
        assert!(send(root, "invalid").is_err());
        let instance = start_at(root).expect("open listener").expect("primary");
        assert!(start_at(root).expect("check uniqueness").is_none());
        let (attempt, uri) = crate::browser_oauth::tests::fixture();
        assert!(
            instance
                .inbox
                .wait_for(attempt, &Cancellation::default(), || {
                    assert!(send(root, &uri).expect("callback accepted"));
                    assert!(!send(root, &uri).expect("repeat rejected"));
                    Ok(())
                })
                .is_ok()
        );
        assert!(!send(root, &"x".repeat(MAX_CALLBACK_BYTES + 1)).unwrap_or(false));
        drop(instance);
        assert!(send(root, &uri).is_err());
        drop(UnixListener::bind(root.join("callback.sock")).expect("stale socket"));
        assert!(start_at(root).expect("reap stale listener").is_some());
    }

    #[test]
    fn refuses_symlink_and_quotes_desktop_entry() {
        let dir = tempfile::tempdir().expect("test runtime");
        let root = dir.path();
        let target = root.join("untouched");
        fs::write(&target, b"fixture").expect("target");
        std::os::unix::fs::symlink(&target, root.join("callback.sock")).expect("symlink");
        assert!(start_at(root).is_err());
        assert_eq!(fs::read(target).expect("untouched"), b"fixture");
        let entry = desktop_entry("/opt/path with spaces/gitRUST").expect("desktop entry");
        assert!(entry.contains("Exec=\"/opt/path with spaces/gitRUST\" --oauth-callback %u"));
        assert!(desktop_entry("/opt/%u").is_none());
    }
}
