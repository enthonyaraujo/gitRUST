//! Provider-facing workspace state. Authentication secrets never enter these views.
#![allow(missing_docs)]

use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct RepositoryKey {
    owner: String,
    name: String,
}

impl RepositoryKey {
    #[must_use]
    pub fn new(owner: &str, name: &str) -> Option<Self> {
        let valid = |text: &str| {
            !text.is_empty()
                && text.len() <= 100
                && !matches!(text, "." | "..")
                && text
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        };
        (valid(owner) && valid(name)).then(|| Self {
            owner: owner.into(),
            name: name.into(),
        })
    }

    #[must_use]
    pub fn from_https(url: &str) -> Option<Self> {
        let path = url.strip_prefix("https://github.com/")?;
        let (owner, name) = path.split_once('/')?;
        Self::new(owner, name.strip_suffix(".git").unwrap_or(name))
    }

    #[must_use]
    pub fn from_remote(url: &str) -> Option<Self> {
        Self::from_https(url).or_else(|| {
            let path = url
                .strip_prefix("git@github.com:")
                .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
            let (owner, name) = path.split_once('/')?;
            Self::new(owner, name.strip_suffix(".git").unwrap_or(name))
        })
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
    #[must_use]
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
    #[must_use]
    pub fn https(&self) -> String {
        format!("https://github.com/{}.git", self.slug())
    }
    #[must_use]
    pub fn ssh(&self) -> String {
        format!("git@github.com:{}.git", self.slug())
    }
    #[must_use]
    pub fn browser(&self) -> String {
        format!("https://github.com/{}", self.slug())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Profile {
    pub id: u64,
    pub login: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Account {
    pub profile: Profile,
    pub connection: Connection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Connection {
    Temporary,
    Stored,
    Reconnect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Organization {
    pub login: String,
    pub installation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Repository {
    pub key: RepositoryKey,
    pub description: Option<String>,
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
    pub language: Option<String>,
    pub updated: String,
    pub default_branch: String,
    pub can_push: bool,
    pub parent: Option<RepositoryKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub head: String,
    pub head_repository: Option<RepositoryKey>,
    pub base: String,
    pub revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    pub name: String,
    pub state: String,
    pub conclusion: Option<String>,
    pub details: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DevicePrompt {
    pub code: String,
    pub expires_in: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Notice {
    InvalidClient,
    InvalidInput,
    Offline,
    Timeout,
    Cancelled,
    Expired,
    Denied,
    Reconnect,
    Forbidden,
    Unavailable,
    RateLimited(u64),
    InvalidResponse,
    Redirect,
    TemporarySession,
    StorageUnavailable,
    NoAccess,
    Created,
    AwaitingInstallation,
    ForkPending,
    LocalGitFailed,
    LocalGitComplete,
    UnknownMutation,
    SignedOut,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct View {
    pub client_id: String,
    pub workspace: Option<PathBuf>,
    pub local_repositories: Arc<[RepositoryKey]>,
    pub review: Option<PullRequest>,
    pub accounts: Arc<[Account]>,
    pub active: Option<u64>,
    pub workspace_account: Option<u64>,
    pub workspace_repo: Option<RepositoryKey>,
    pub workspace_branch: Option<String>,
    pub workspace_revision: Option<String>,
    pub organizations: Arc<[Organization]>,
    pub repositories: Arc<[Repository]>,
    pub repository_page: Option<u32>,
    pub pull_repository: Option<RepositoryKey>,
    pub pulls: Arc<[PullRequest]>,
    pub pull_page: Option<u32>,
    pub checks: Option<Arc<[Check]>>,
    pub check_scope: Option<(RepositoryKey, String)>,
    pub workflows: bool,
    pub prompt: Option<DevicePrompt>,
    pub notice: Option<Notice>,
    pub result: Option<RepositoryKey>,
    pub result_pull: Option<(RepositoryKey, u64)>,
    pub busy: bool,
    pub avatar: Option<Arc<Avatar>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Avatar {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CreateRepository {
    pub owner: String,
    pub name: String,
    pub description: String,
    pub private: bool,
    pub local: Option<Publish>,
}

#[derive(Clone, Debug)]
pub struct Publish {
    pub workspace: PathBuf,
    pub remote: String,
    pub branch: String,
}

#[derive(Clone, Debug)]
pub struct CreatePull {
    pub destination: RepositoryKey,
    pub base: String,
    pub source: RepositoryKey,
    pub head: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

#[derive(Clone, Debug)]
pub enum Action {
    Restore(String),
    SignIn {
        client_id: String,
        reconnect: Option<u64>,
    },
    Cancel,
    Select(u64),
    Bind(PathBuf, Option<u64>),
    Disconnect(u64),
    Refresh,
    Repositories {
        installation: Option<u64>,
        page: u32,
    },
    Clone {
        repository: RepositoryKey,
        parent: Option<RepositoryKey>,
        destination: PathBuf,
        ssh: bool,
    },
    Create(CreateRepository),
    ResumePublish {
        repository: RepositoryKey,
        publish: Publish,
    },
    Fork {
        repository: RepositoryKey,
        organization: Option<String>,
        name: String,
    },
    Repository(RepositoryKey),
    SyncFork {
        repository: RepositoryKey,
        branch: String,
    },
    Pulls {
        repository: RepositoryKey,
        page: u32,
        state: PullFilter,
    },
    CreatePull(CreatePull),
    CheckoutPull {
        repository: RepositoryKey,
        number: u64,
        local: String,
    },
    Checks {
        repository: RepositoryKey,
        revision: String,
    },
    Workflows {
        repository: RepositoryKey,
        revision: String,
    },
    Open(String),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PullFilter {
    #[default]
    Open,
    Closed,
    All,
}

impl PullFilter {
    #[must_use]
    pub const fn parameter(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::All => "all",
        }
    }
}

/// Encode a Git ref as a browser URL segment.
#[must_use]
pub fn encode_reference(value: &str) -> String {
    use std::fmt::Write as _;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}
