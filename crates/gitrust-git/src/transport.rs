use std::{
    ffi::{OsStr, OsString},
    fmt, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use gitrust_core::remotes::{Auth, Credential, RemoteNotice};
use url::Url;

use crate::GitError;

/// A credential-free Git endpoint with a supported transport.
#[derive(Clone, Eq, PartialEq)]
pub struct RemoteEndpoint {
    value: String,
    ssh: Option<SshTarget>,
}

#[derive(Clone, Eq, PartialEq)]
struct SshTarget {
    host: String,
    port: Option<u16>,
    path: String,
}

impl fmt::Debug for RemoteEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RemoteEndpoint(<private URL>)")
    }
}

impl RemoteEndpoint {
    /// Validate an HTTPS, SSH URL or SCP-like SSH address.
    ///
    /// # Errors
    /// Rejects user information on HTTPS, passwords, and unsupported schemes or syntax.
    pub fn new(value: &str) -> Result<Self, GitError> {
        let invalid = || GitError::InvalidArgument("remote endpoint");
        if value.is_empty()
            || value.len() > 4096
            || value
                .bytes()
                .any(|b| b <= b' ' || b == 127 || b"\\?#".contains(&b))
        {
            return Err(invalid());
        }
        let ssh = if let Some((scheme, remainder)) = value.split_once("://") {
            let url = Url::parse(value).map_err(|_| invalid())?;
            let host = url.host_str().ok_or_else(invalid)?;
            if url.password().is_some() || url.path().is_empty() || url.path() == "/" {
                return Err(invalid());
            }
            match scheme {
                "https"
                    if !remainder
                        .split('/')
                        .next()
                        .is_some_and(|authority| authority.contains('@')) =>
                {
                    None
                }
                "ssh" => {
                    let host = host.trim_start_matches('[').trim_end_matches(']');
                    Some(SshTarget {
                        host: ssh_host(url.username(), host)?,
                        port: url.port(),
                        path: ssh_path(url.path())?,
                    })
                }
                _ => return Err(invalid()),
            }
        } else {
            let (authority, path) = value.split_once(':').ok_or_else(invalid)?;
            let (user, host) = authority.split_once('@').ok_or_else(invalid)?;
            if user.is_empty() || host.contains(':') {
                return Err(invalid());
            }
            Some(SshTarget {
                host: ssh_host(user, host)?,
                port: None,
                path: ssh_path(path)?,
            })
        };
        Ok(Self {
            value: value.to_owned(),
            ssh,
        })
    }

    /// Validated, credential-free endpoint used in Git arguments.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Whether this endpoint uses OpenSSH.
    #[must_use]
    pub const fn is_ssh(&self) -> bool {
        self.ssh.is_some()
    }

    /// Derive a directory suggestion from the endpoint path.
    #[must_use]
    pub fn directory(&self) -> Option<&str> {
        self.value
            .rsplit(['/', ':'])
            .next()
            .map(|name| name.strip_suffix(".git").unwrap_or(name))
            .filter(|name| {
                !name.is_empty()
                    && *name != "."
                    && *name != ".."
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
    }
}

fn ssh_host(user: &str, host: &str) -> Result<String, GitError> {
    if (!user.is_empty()
        && (user.starts_with('-')
            || !user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))))
        || host.is_empty()
        || host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".:-".contains(&b))
    {
        return Err(GitError::InvalidArgument("SSH host"));
    }
    Ok(if user.is_empty() {
        host.to_owned()
    } else {
        format!("{user}@{host}")
    })
}

fn ssh_path(path: &str) -> Result<String, GitError> {
    if path.is_empty()
        || path.starts_with('-')
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_.-".contains(&b))
    {
        return Err(GitError::InvalidArgument("SSH repository path"));
    }
    Ok(path.to_owned())
}

pub(crate) struct Network {
    pub(crate) protocol: &'static str,
    pub(crate) env: Vec<(&'static str, OsString)>,
    pub(crate) options: Vec<OsString>,
}

impl Network {
    pub(crate) fn prepare(
        endpoint: &RemoteEndpoint,
        auth: &Auth,
        bridge: Option<&Path>,
    ) -> Result<Self, GitError> {
        let ssh = endpoint.is_ssh();
        let protocol = if ssh { "ssh" } else { "https" };
        let mut network = Self {
            protocol,
            env: Vec::new(),
            options: Vec::new(),
        };
        network.option(format!("protocol.{protocol}.allow=always"));
        for setting in [
            "credential.helper=",
            "http.sslVerify=true",
            "http.followRedirects=false",
            "http.extraHeader=",
            "maintenance.auto=false",
            "gc.auto=0",
            "fetch.recurseSubmodules=false",
            "push.recurseSubmodules=no",
        ] {
            network.option(setting);
        }
        if ssh {
            network.ssh(endpoint, auth, bridge)?;
        } else {
            network.https(endpoint, auth)?;
        }
        Ok(network)
    }

    fn option(&mut self, option: impl Into<OsString>) {
        self.options.push("-c".into());
        self.options.push(option.into());
    }

    fn https(&mut self, endpoint: &RemoteEndpoint, auth: &Auth) -> Result<(), GitError> {
        if let Some(ca) = &auth.ca {
            self.env
                .push(("GIT_SSL_CAINFO", regular(ca)?.into_os_string()));
        }
        match &auth.credential {
            Credential::Anonymous => {}
            Credential::MemoryCache | Credential::SecretService => {
                let helper = if matches!(auth.credential, Credential::MemoryCache) {
                    "cache"
                } else {
                    "libsecret"
                };
                let path =
                    helper_path(helper).ok_or(GitError::Remote(RemoteNotice::Authentication))?;
                self.option(format!("credential.helper={path}"));
                if let Some(home) =
                    std::env::var_os("HOME").filter(|home| Path::new(home).is_absolute())
                {
                    self.env.push(("HOME", home));
                }
                if helper == "libsecret"
                    && let Some(bus) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS")
                {
                    self.env.push(("DBUS_SESSION_BUS_ADDRESS", bus));
                }
            }
            Credential::LinkedAccount => {
                return Err(GitError::Remote(RemoteNotice::Authentication));
            }
            Credential::ScopedToken {
                endpoint: approved,
                secret,
            } => {
                if auth.ca.is_some() || approved != endpoint.as_str() {
                    return Err(GitError::Remote(RemoteNotice::Authentication));
                }
                self.basic_auth("x-access-token", secret)?;
            }
            Credential::Password { user, secret } => self.basic_auth(user, secret)?,
        }
        Ok(())
    }

    fn basic_auth(&mut self, user: &str, secret: &str) -> Result<(), GitError> {
        if user.is_empty()
            || user.contains(':')
            || secret.is_empty()
            || user.len() + secret.len() > 8192
            || user.chars().chain(secret.chars()).any(char::is_control)
        {
            return Err(GitError::InvalidArgument("HTTPS credentials"));
        }
        // Git receives only the variable name in argv. Redirects remain disabled.
        let value = format!("{user}:{secret}");
        let header = format!("Authorization: Basic {}", base64(value.as_bytes()));
        self.env.push(("GITRUST_HTTP_AUTH", header.into()));
        self.options
            .push("--config-env=http.extraHeader=GITRUST_HTTP_AUTH".into());
        Ok(())
    }

    fn ssh(
        &mut self,
        endpoint: &RemoteEndpoint,
        auth: &Auth,
        bridge: Option<&Path>,
    ) -> Result<(), GitError> {
        if !matches!(auth.credential, Credential::Anonymous) || auth.ca.is_some() {
            return Err(GitError::InvalidArgument(
                "HTTPS authentication on SSH endpoint",
            ));
        }
        let bridge = regular(bridge.ok_or(GitError::UnsupportedRepositoryFeature(
            "native SSH bridge unavailable",
        ))?)?;
        if !system_executable(Path::new("/usr/bin/ssh")) {
            return Err(GitError::UnsupportedRepositoryFeature(
                "OpenSSH unavailable",
            ));
        }
        self.env.extend([
            ("GIT_SSH", bridge.into_os_string()),
            ("GIT_SSH_VARIANT", "ssh".into()),
            ("GITRUST_SSH_ENDPOINT", endpoint.as_str().into()),
        ]);
        for (key, path) in [
            ("GITRUST_SSH_KNOWN_HOSTS", &auth.known_hosts),
            ("GITRUST_SSH_IDENTITY", &auth.identity),
        ] {
            if let Some(path) = path {
                self.env.push((key, regular(path)?.into_os_string()));
            }
        }
        if let Some(socket) = agent_socket() {
            self.env.push(("SSH_AUTH_SOCK", socket));
        }
        Ok(())
    }
}

fn helper_path(name: &str) -> Option<String> {
    ["/usr/libexec/git-core", "/usr/lib/git-core", "/usr/bin"]
        .into_iter()
        .map(|prefix| format!("{prefix}/git-credential-{name}"))
        .find(|candidate| system_executable(Path::new(candidate)))
}

fn system_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        fs::canonicalize(path).is_ok_and(|resolved| {
            resolved.is_file()
                && resolved.ancestors().all(|part| {
                    fs::metadata(part)
                        .is_ok_and(|metadata| metadata.uid() == 0 && metadata.mode() & 0o022 == 0)
                })
        })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}

fn agent_socket() -> Option<OsString> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt as _;
        std::env::var_os("SSH_AUTH_SOCK").filter(|socket| {
            Path::new(socket).is_absolute()
                && fs::metadata(socket).is_ok_and(|data| data.file_type().is_socket())
        })
    }
    #[cfg(not(unix))]
    None
}

fn regular(path: &Path) -> Result<PathBuf, GitError> {
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(GitError::InvalidArgument("authentication path"));
    }
    let path = fs::canonicalize(path).map_err(GitError::io)?;
    if !path.is_file() {
        return Err(GitError::InvalidArgument("authentication path"));
    }
    Ok(path)
}

/// Run the private SSH entrypoint of the native application.
/// Returns None when the regular application should start.
#[must_use]
pub fn ssh_bridge() -> Option<ExitCode> {
    let endpoint = std::env::var_os("GITRUST_SSH_ENDPOINT")?;
    let result = endpoint
        .to_str()
        .and_then(|endpoint| RemoteEndpoint::new(endpoint).ok())
        .and_then(|endpoint| endpoint.ssh)
        .and_then(|target| execute_ssh(&target));
    Some(result.unwrap_or(ExitCode::FAILURE))
}

fn execute_ssh(target: &SshTarget) -> Option<ExitCode> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let service = ["git-upload-pack", "git-receive-pack"]
        .into_iter()
        .find(|service| {
            args.last() == Some(&OsString::from(format!("{service} '{}'", target.path)))
        })?;
    if !system_executable(Path::new("/usr/bin/ssh")) {
        return None;
    }
    let mut command = Command::new("/usr/bin/ssh");
    command.args([
        "-F",
        "/dev/null",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "UpdateHostKeys=no",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "SendEnv=GIT_PROTOCOL",
    ]);
    if let Some(port) = target.port {
        command.arg("-p").arg(port.to_string());
    }
    if let Some(path) = std::env::var_os("GITRUST_SSH_KNOWN_HOSTS") {
        let path = regular(Path::new(&path)).ok()?;
        let path = path.to_str()?;
        if path.bytes().any(|b| b <= b' ' || b"%$~\\\"".contains(&b)) {
            return None;
        }
        command.arg("-o").arg(format!("UserKnownHostsFile={path}"));
    }
    if let Some(path) = std::env::var_os("GITRUST_SSH_IDENTITY") {
        command.arg("-i").arg(regular(Path::new(&path)).ok()?);
    }
    command
        .args([OsStr::new("--"), OsStr::new(&target.host)])
        .arg(format!("{service} '{}'", target.path));
    let code = command.status().ok()?.code()?;
    u8::try_from(code).ok().map(ExitCode::from)
}

fn base64(bytes: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        text.push(char::from(CHARS[usize::from(a >> 2)]));
        text.push(char::from(CHARS[usize::from((a & 3) << 4 | b >> 4)]));
        text.push(if chunk.len() > 1 {
            char::from(CHARS[usize::from((b & 15) << 2 | c >> 6)])
        } else {
            '='
        });
        text.push(if chunk.len() > 2 {
            char::from(CHARS[usize::from(c & 63)])
        } else {
            '='
        });
    }
    text
}

pub(crate) fn network_notice(bytes: &[u8], writing: bool) -> RemoteNotice {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    if text.contains("host identification has changed") {
        RemoteNotice::ChangedHostKey
    } else if text.contains("host key verification failed") || text.contains("no host key is known")
    {
        RemoteNotice::HostKey
    } else if text.contains("certificate") || text.contains("ssl peer") {
        RemoteNotice::Certificate
    } else if text.contains("permission denied (publickey") {
        if agent_socket().is_none() {
            RemoteNotice::Agent
        } else {
            RemoteNotice::Authentication
        }
    } else if writing && (text.contains("returned error: 403") || text.contains("http 403")) {
        RemoteNotice::Rejected
    } else if [
        "authentication failed",
        "could not read username",
        "could not read password",
        "returned error: 401",
        "returned error: 403",
        "http 401",
        "http 403",
    ]
    .iter()
    .any(|cause| text.contains(cause))
    {
        RemoteNotice::Authentication
    } else if text.contains("non-fast-forward") || text.contains("fetch first") {
        RemoteNotice::NonFastForward
    } else if text.contains("[remote rejected]") || text.contains("pre-receive hook declined") {
        RemoteNotice::Rejected
    } else if [
        "resolve host",
        "could not resolve",
        "connection refused",
        "network is unreachable",
        "connection timed out",
    ]
    .iter()
    .any(|cause| text.contains(cause))
    {
        RemoteNotice::Offline
    } else if writing {
        RemoteNotice::Uncertain
    } else {
        RemoteNotice::Failed
    }
}

#[cfg(test)]
mod scoped_token_tests {
    use super::*;

    #[test]
    fn provider_header_is_scoped_and_its_value_never_enters_arguments() {
        let url = "https://github.com/example/test.git";
        let endpoint = RemoteEndpoint::new(url).expect("authorized HTTPS");
        let auth = Auth {
            credential: Credential::ScopedToken {
                endpoint: url.into(),
                secret: "fixture-only".into(),
            },
            ..Auth::default()
        };
        let prepared = Network::prepare(&endpoint, &auth, None).expect("approved endpoint");
        assert!(
            prepared
                .options
                .iter()
                .any(|value| value == "--config-env=http.extraHeader=GITRUST_HTTP_AUTH")
        );
        assert!(
            prepared
                .options
                .iter()
                .all(|value| !value.to_string_lossy().contains("fixture-only"))
        );
        for different in [
            "https://example.invalid/test.git",
            "https://github.com/example/other.git",
        ] {
            let different = RemoteEndpoint::new(different).expect("valid HTTPS");
            assert!(matches!(
                Network::prepare(&different, &auth, None),
                Err(GitError::Remote(RemoteNotice::Authentication))
            ));
        }
    }
}
