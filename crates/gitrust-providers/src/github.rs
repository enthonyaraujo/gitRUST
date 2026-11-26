//! GitHub.com REST and Device Flow client.
#![allow(missing_docs, clippy::missing_errors_doc)]

mod auth;
mod wire;
pub use auth::{Cancellation, ClientId, Session, unix_time};

use gitrust_core::github::{
    Check, CreatePull, CreateRepository, Notice, Organization, Profile, PullFilter, PullRequest,
    Repository, RepositoryKey,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{fmt, time::Duration};

const PUBLIC_API: &str = "https://api.github.com";
const PUBLIC_DEVICE: &str = "https://github.com";
const PER_PAGE: usize = 30;

/// Credential-free paginated results. Continuations are numeric pages, never URL inputs.
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<u32>,
}

pub struct Client {
    agent: ureq::Agent,
    api: &'static str,
    oauth: &'static str,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GitHub API client")
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new(true, PUBLIC_API, PUBLIC_DEVICE)
    }
}

impl Client {
    fn new(https: bool, api: &'static str, oauth: &'static str) -> Self {
        let certificates = rustls_native_certs::load_native_certs();
        let roots = ureq::tls::RootCerts::from(
            certificates
                .certs
                .iter()
                .map(|cert| ureq::tls::Certificate::from_der(cert.as_ref()).to_owned()),
        );
        let tls = ureq::tls::TlsConfig::builder().root_certs(roots).build();
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .https_only(https)
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(12)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .build();
        Self {
            agent: config.into(),
            api,
            oauth,
        }
    }

    fn rest<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<Value>,
        token: &str,
        stop: &Cancellation,
    ) -> Result<Response<T>, Notice> {
        self.request(self.api, path, body, Some(token), stop)
    }

    fn request<T: DeserializeOwned>(
        &self,
        origin: &str,
        path: &str,
        body: Option<Value>,
        token: Option<&str>,
        stop: &Cancellation,
    ) -> Result<Response<T>, Notice> {
        stop.check()?;
        let url = format!("{origin}{path}");
        let mutation = body.is_some() && token.is_some();
        let configure = |headers: &mut ureq::http::HeaderMap| -> Result<(), Notice> {
            use ureq::http::{HeaderName, HeaderValue, header};
            headers.insert(
                header::ACCEPT,
                HeaderValue::from_static(if token.is_some() {
                    "application/vnd.github+json"
                } else {
                    "application/json"
                }),
            );
            headers.insert(header::USER_AGENT, HeaderValue::from_static("gitRUST"));
            if token.is_some() {
                headers.insert(
                    HeaderName::from_static("x-github-api-version"),
                    HeaderValue::from_static("2026-03-10"),
                );
            }
            if let Some(token) = token {
                let mut header = HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| Notice::Reconnect)?;
                header.set_sensitive(true);
                headers.insert(ureq::http::header::AUTHORIZATION, header);
            }
            Ok(())
        };
        let mut response = if let Some(body) = body {
            let mut request = self
                .agent
                .post(&url)
                .header("Content-Type", "application/json");
            configure(request.headers_mut().ok_or(Notice::InvalidInput)?)?;
            let body = serde_json::to_vec(&body).map_err(|_| Notice::InvalidInput)?;
            request.send(body.as_slice()).map_err(|error| {
                if mutation {
                    Notice::UnknownMutation
                } else {
                    network_error(&error)
                }
            })?
        } else {
            let request = || {
                let mut request = self.agent.get(&url);
                configure(request.headers_mut().ok_or(Notice::InvalidInput)?)?;
                request.call().map_err(|error| network_error(&error))
            };
            let response = request()?;
            if matches!(response.status().as_u16(), 502..=504) {
                stop.wait(Duration::from_millis(350))?;
                request()?
            } else {
                response
            }
        };
        let status = response.status().as_u16();
        let number = |key| {
            response
                .headers()
                .get(key)
                .and_then(|it| it.to_str().ok()?.parse::<u64>().ok())
        };
        let retry = number("retry-after");
        let remaining = number("x-ratelimit-remaining");
        if status == 429 || (status == 403 && (remaining == Some(0) || retry.is_some())) {
            let reset = number("x-ratelimit-reset").map(|at| at.saturating_sub(unix_time()));
            return Err(Notice::RateLimited(retry.or(reset).unwrap_or(60)));
        }
        match status {
            200..=299 => {}
            300..=399 => return Err(Notice::Redirect),
            401 => return Err(Notice::Reconnect),
            403 => return Err(Notice::Forbidden),
            404 => return Err(Notice::NoAccess),
            400 | 409 | 422 => return Err(Notice::InvalidInput),
            500..=599 if mutation => {
                return Err(Notice::UnknownMutation);
            }
            _ => return Err(Notice::Unavailable),
        }
        let more = response
            .headers()
            .get("link")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|links| links.split(',').any(|link| link.ends_with("rel=\"next\"")));
        let bytes = response
            .body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_vec()
            .map_err(|_| Notice::InvalidResponse)?;
        let body = serde_json::from_slice(&bytes).map_err(|_| Notice::InvalidResponse)?;
        Ok(Response { body, more })
    }

    /// Read a small public profile image without an Authorization header.
    pub fn avatar(&self, account: u64, stop: &Cancellation) -> Result<Vec<u8>, Notice> {
        stop.check()?;
        let mut response = self
            .agent
            .get(format!(
                "https://avatars.githubusercontent.com/u/{account}?s=64"
            ))
            .header("User-Agent", "gitRUST")
            .header("Accept", "image/png")
            .call()
            .map_err(|error| network_error(&error))?;
        if !response.status().is_success() {
            return Err(Notice::Unavailable);
        }
        response
            .body_mut()
            .with_config()
            .limit(256 * 1024)
            .read_to_vec()
            .map_err(|_| Notice::InvalidResponse)
    }

    pub fn profile(&self, token: &str, stop: &Cancellation) -> Result<Profile, Notice> {
        self.rest::<wire::Profile>("/user", None, token, stop)?
            .body
            .model()
    }

    pub fn installations(
        &self,
        token: &str,
        page: u32,
        stop: &Cancellation,
    ) -> Result<Page<Organization>, Notice> {
        let path = paged("/user/installations", page);
        let response = self.rest::<wire::Installations>(&path, None, token, stop)?;
        let next = response.next(page);
        let items = response
            .body
            .installations
            .into_iter()
            .filter(|installation| installation.suspended_at.is_none())
            .map(|installation| {
                let profile = installation.account.model()?;
                Ok(Organization {
                    login: profile.login,
                    installation: installation.id,
                })
            })
            .collect::<Result<_, Notice>>()?;
        Ok(Page { items, next })
    }

    pub fn repositories(
        &self,
        token: &str,
        installation: Option<u64>,
        page: u32,
        stop: &Cancellation,
    ) -> Result<Page<Repository>, Notice> {
        if let Some(id) = installation {
            let path = paged(&format!("/user/installations/{id}/repositories"), page);
            let Response { body, more } =
                self.rest::<wire::Repositories>(&path, None, token, stop)?;
            models(body.repositories, more, page)
        } else {
            let path = format!("{}&sort=updated", paged("/user/repos", page));
            let Response { body, more } =
                self.rest::<Vec<wire::Repository>>(&path, None, token, stop)?;
            models(body, more, page)
        }
    }

    pub fn repository(
        &self,
        token: &str,
        repo: &RepositoryKey,
        stop: &Cancellation,
    ) -> Result<Repository, Notice> {
        let model = self
            .rest::<wire::Repository>(&repository_path(repo, ""), None, token, stop)?
            .body
            .model()?;
        require_same(&model, repo)?;
        Ok(model)
    }

    /// Read metadata for an explicitly requested public repository without a token.
    pub fn public_repository(
        &self,
        repo: &RepositoryKey,
        stop: &Cancellation,
    ) -> Result<Repository, Notice> {
        let result = self
            .request::<wire::Repository>(self.api, &repository_path(repo, ""), None, None, stop)?
            .body
            .model()?;
        require_same(&result, repo)?;
        if result.private {
            return Err(Notice::NoAccess);
        }
        Ok(result)
    }

    pub fn create_repository(
        &self,
        token: &str,
        title: &CreateRepository,
        organization: bool,
        stop: &Cancellation,
    ) -> Result<Repository, Notice> {
        let repo = RepositoryKey::new(&title.owner, &title.name).ok_or(Notice::InvalidInput)?;
        if title.description.len() > 350 {
            return Err(Notice::InvalidInput);
        }
        let endpoint = if organization {
            format!("/orgs/{}/repos", repo.owner())
        } else {
            "/user/repos".into()
        };
        let body = json!({"name": repo.name(), "description": title.description, "private": title.private, "auto_init": false});
        let model = self
            .rest::<wire::Repository>(&endpoint, Some(body), token, stop)?
            .body
            .model()?;
        require_same(&model, &repo)?;
        if model.private != title.private {
            return Err(Notice::UnknownMutation);
        }
        Ok(model)
    }

    pub fn fork(
        &self,
        token: &str,
        repo: &RepositoryKey,
        organization: Option<&str>,
        name: &str,
        stop: &Cancellation,
    ) -> Result<Repository, Notice> {
        RepositoryKey::new(organization.unwrap_or(repo.owner()), name)
            .ok_or(Notice::InvalidInput)?;
        let mut body = json!({"name": name});
        if let Some(organization) = organization {
            body["organization"] = json!(organization);
        }
        self.rest::<wire::Repository>(&repository_path(repo, "/forks"), Some(body), token, stop)?
            .body
            .model()
    }

    pub fn sync_fork(
        &self,
        token: &str,
        repo: &RepositoryKey,
        branch: &str,
        stop: &Cancellation,
    ) -> Result<(), Notice> {
        reference(branch)?;
        self.rest::<Value>(
            &repository_path(repo, "/merge-upstream"),
            Some(json!({"branch": branch})),
            token,
            stop,
        )?;
        Ok(())
    }

    pub fn pulls(
        &self,
        token: &str,
        repo: &RepositoryKey,
        page: u32,
        state: PullFilter,
        stop: &Cancellation,
    ) -> Result<Page<PullRequest>, Notice> {
        let path = format!(
            "{}&state={}",
            paged(&repository_path(repo, "/pulls"), page),
            state.parameter()
        );
        let Response { body, more } = self.rest::<Vec<wire::Pull>>(&path, None, token, stop)?;
        let items = body
            .into_iter()
            .map(wire::Pull::model)
            .collect::<Result<_, _>>()?;
        Ok(Page {
            items,
            next: more.then_some(page.saturating_add(1)),
        })
    }

    pub fn pull(
        &self,
        token: &str,
        repo: &RepositoryKey,
        number: u64,
        stop: &Cancellation,
    ) -> Result<PullRequest, Notice> {
        self.rest::<wire::Pull>(
            &repository_path(repo, &format!("/pulls/{number}")),
            None,
            token,
            stop,
        )?
        .body
        .model()
    }

    pub fn create_pull(
        &self,
        token: &str,
        pull: &CreatePull,
        stop: &Cancellation,
    ) -> Result<PullRequest, Notice> {
        reference(&pull.head)?;
        reference(&pull.base)?;
        if pull.head.contains(':')
            || pull.title.trim().is_empty()
            || pull.title.len() > 256
            || pull.body.len() > 65_536
        {
            return Err(Notice::InvalidInput);
        }
        let body = json!({
            "base": pull.base, "head": format!("{}:{}", pull.source.owner(), pull.head),
            "head_repo": pull.source.name(), "title": pull.title, "body": pull.body, "draft": pull.draft,
        });
        self.rest::<wire::Pull>(
            &repository_path(&pull.destination, "/pulls"),
            Some(body),
            token,
            stop,
        )?
        .body
        .model()
    }

    pub fn workflow_runs(
        &self,
        token: &str,
        repo: &RepositoryKey,
        revision: &str,
        stop: &Cancellation,
    ) -> Result<Vec<Check>, Notice> {
        validate_sha(revision)?;
        let mut all = Vec::new();
        for page in 1..=3 {
            let path = format!(
                "{}&head_sha={revision}",
                paged(&repository_path(repo, "/actions/runs"), page)
            );
            let response = self.rest::<wire::WorkflowRuns>(&path, None, token, stop)?;
            all.extend(
                response
                    .body
                    .workflow_runs
                    .into_iter()
                    .map(wire::CheckRun::model),
            );
            if !response.more {
                break;
            }
        }
        Ok(all)
    }

    pub fn checks(
        &self,
        token: &str,
        repo: &RepositoryKey,
        revision: &str,
        stop: &Cancellation,
    ) -> Result<Vec<Check>, Notice> {
        validate_sha(revision)?;
        let mut all = Vec::new();
        for page in 1..=3 {
            let prefix = format!("/commits/{revision}");
            let path = paged(
                &repository_path(repo, &format!("{prefix}/check-runs")),
                page,
            );
            let checks = self.rest::<wire::Checks>(&path, None, token, stop)?;
            let more = checks.more;
            all.extend(
                checks
                    .body
                    .check_runs
                    .into_iter()
                    .map(wire::CheckRun::model),
            );
            if !more {
                break;
            }
        }
        for page in 1..=3 {
            let path = paged(
                &repository_path(repo, &format!("/commits/{revision}/status")),
                page,
            );
            let statuses = self.rest::<wire::Statuses>(&path, None, token, stop)?;
            let more = statuses.more;
            all.extend(statuses.body.statuses.into_iter().map(wire::Status::model));
            if !more {
                break;
            }
        }
        Ok(all)
    }
}

pub const DEVICE_URL: &str = "https://github.com/login/device";

#[must_use]
pub fn browser_url_allowed(value: &str) -> bool {
    let Some(path) = value.strip_prefix("https://github.com/") else {
        return false;
    };
    !path.is_empty()
        && value.len() <= 4096
        && !path.starts_with('/')
        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
        && !path
            .bytes()
            .any(|byte| byte <= b' ' || byte == 127 || b"\\?#".contains(&byte))
}

fn validate_sha(revision: &str) -> Result<(), Notice> {
    if revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Notice::InvalidInput)
    }
}

#[allow(clippy::case_sensitive_file_extension_comparisons)] // Git uses an exact ref suffix.
fn reference(value: &str) -> Result<(), Notice> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || value == "@"
        || value.contains("..")
        || value.contains("@{")
        || value.split('/').any(|segment| {
            segment.is_empty() || segment.starts_with('.') || segment.ends_with(".lock")
        })
        || value.ends_with('.')
        || value
            .bytes()
            .any(|byte| byte <= b' ' || byte == 127 || b"~^:?*[\\".contains(&byte))
    {
        Err(Notice::InvalidInput)
    } else {
        Ok(())
    }
}

fn repository_path(repo: &RepositoryKey, suffix: &str) -> String {
    format!("/repos/{}{suffix}", repo.slug())
}

fn require_same(model: &Repository, repo: &RepositoryKey) -> Result<(), Notice> {
    if model.key.slug().eq_ignore_ascii_case(&repo.slug()) {
        Ok(())
    } else {
        Err(Notice::InvalidResponse)
    }
}

fn network_error(error: &ureq::Error) -> Notice {
    if matches!(error, ureq::Error::Timeout(_)) {
        Notice::Timeout
    } else {
        Notice::Offline
    }
}

fn paged(path: &str, page: u32) -> String {
    format!("{path}?per_page={PER_PAGE}&page={}", page.max(1))
}

struct Response<T> {
    body: T,
    more: bool,
}

impl<T> Response<T> {
    fn next(&self, page: u32) -> Option<u32> {
        self.more.then_some(page.max(1).saturating_add(1))
    }
}

fn models(
    repositories: Vec<wire::Repository>,
    more: bool,
    page: u32,
) -> Result<Page<Repository>, Notice> {
    let items = repositories
        .into_iter()
        .map(wire::Repository::model)
        .collect::<Result<_, _>>()?;
    Ok(Page {
        items,
        next: more.then_some(page.max(1).saturating_add(1)),
    })
}

#[cfg(test)]
mod tests;
