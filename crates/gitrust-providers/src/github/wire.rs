use gitrust_core::github::{self, Notice, RepositoryKey};
use serde::Deserialize;

#[derive(Deserialize)]
pub(super) struct Profile {
    pub(super) id: u64,
    pub(super) login: String,
    name: Option<String>,
}

impl Profile {
    pub(super) fn model(self) -> Result<github::Profile, Notice> {
        RepositoryKey::new(&self.login, "profile").ok_or(Notice::InvalidResponse)?;
        if self.id == 0 {
            return Err(Notice::InvalidResponse);
        }
        Ok(github::Profile {
            id: self.id,
            login: self.login,
            name: self.name,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Installations {
    pub(super) installations: Vec<Installation>,
}

#[derive(Deserialize)]
pub(super) struct Installation {
    pub(super) id: u64,
    pub(super) account: Profile,
    pub(super) suspended_at: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Repositories {
    pub(super) repositories: Vec<Repository>,
}

#[derive(Deserialize)]
pub(super) struct Repository {
    name: String,
    owner: Profile,
    private: bool,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    fork: bool,
    description: Option<String>,
    language: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    default_branch: String,
    permissions: Option<Permissions>,
    parent: Option<Box<Repository>>,
}

impl Repository {
    fn key(&self) -> Result<RepositoryKey, Notice> {
        RepositoryKey::new(&self.owner.login, &self.name).ok_or(Notice::InvalidResponse)
    }

    pub(super) fn model(self) -> Result<github::Repository, Notice> {
        let key = self.key()?;
        let parent = self.parent.as_ref().map(|repo| repo.key()).transpose()?;
        Ok(github::Repository {
            key,
            parent,
            private: self.private,
            archived: self.archived,
            fork: self.fork,
            description: self.description,
            language: self.language,
            updated: self.updated_at.unwrap_or_default(),
            default_branch: self.default_branch,
            can_push: self.permissions.is_some_and(|permissions| permissions.push),
        })
    }
}

#[derive(Deserialize)]
struct Permissions {
    push: bool,
}

#[derive(Deserialize)]
pub(super) struct Pull {
    number: u64,
    title: String,
    user: Option<Profile>,
    state: String,
    draft: bool,
    head: PullRef,
    base: PullRef,
}

#[derive(Deserialize)]
struct PullRef {
    #[serde(rename = "ref")]
    branch: String,
    sha: String,
    repo: Option<Box<Repository>>,
}

impl Pull {
    pub(super) fn model(self) -> Result<github::PullRequest, Notice> {
        Ok(github::PullRequest {
            number: self.number,
            title: self.title,
            draft: self.draft,
            state: self.state,
            author: self.user.map_or_else(String::new, |user| user.login),
            head: self.head.branch,
            base: self.base.branch,
            revision: self.head.sha,
            head_repository: self.head.repo.as_ref().map(|repo| repo.key()).transpose()?,
        })
    }
}

#[derive(Deserialize)]
pub(super) struct Checks {
    pub(super) check_runs: Vec<CheckRun>,
}

#[derive(Deserialize)]
pub(super) struct CheckRun {
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: Option<String>,
}

impl CheckRun {
    pub(super) fn model(self) -> github::Check {
        github::Check {
            name: self.name,
            state: self.status,
            conclusion: self.conclusion,
            details: self.html_url.filter(|url| super::browser_url_allowed(url)),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct Statuses {
    pub(super) statuses: Vec<Status>,
}

#[derive(Deserialize)]
pub(super) struct Status {
    context: String,
    state: String,
    target_url: Option<String>,
}

impl Status {
    pub(super) fn model(self) -> github::Check {
        let conclusion = (self.state != "pending").then(|| self.state.clone());
        github::Check {
            name: self.context,
            state: self.state,
            conclusion,
            details: self
                .target_url
                .filter(|url| super::browser_url_allowed(url)),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct WorkflowRuns {
    pub(super) workflow_runs: Vec<CheckRun>,
}
