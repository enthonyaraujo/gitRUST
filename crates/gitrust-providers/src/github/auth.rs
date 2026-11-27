use std::{
    fmt,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded};
use gitrust_core::github::{BrowserState, DevicePrompt, Notice, Profile};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{BrowserAttempt, Callback, Client, REDIRECT_URI};

#[derive(Clone, Debug)]
pub struct Cancellation {
    sender: Arc<Mutex<Option<Sender<()>>>>,
    receiver: Receiver<()>,
}

impl Default for Cancellation {
    fn default() -> Self {
        let (sender, receiver) = bounded(0);
        Self {
            sender: Arc::new(Mutex::new(Some(sender))),
            receiver,
        }
    }
}

impl Cancellation {
    pub fn cancel(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            sender.take();
        }
    }

    pub fn check(&self) -> Result<(), Notice> {
        self.wait(Duration::ZERO)
    }

    #[must_use]
    pub fn signal(&self) -> &Receiver<()> {
        &self.receiver
    }

    pub fn wait(&self, duration: Duration) -> Result<(), Notice> {
        match self.receiver.recv_timeout(duration) {
            Err(RecvTimeoutError::Timeout) => Ok(()),
            _ => Err(Notice::Cancelled),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientId(String);

impl ClientId {
    pub fn new(value: &str) -> Result<Self, Notice> {
        if (10..=100).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            Ok(Self(value.into()))
        } else {
            Err(Notice::InvalidClient)
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Deserialize)]
struct DeviceReply {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "interval")]
    interval: u64,
}

const fn interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenReply {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    refresh_token_expires_in: Option<u64>,
    error: Option<String>,
    interval: Option<u64>,
    scope: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct Session {
    account: u64,
    login: String,
    name: Option<String>,
    access: String,
    refresh: Option<String>,
    expires: Option<u64>,
    refresh_expires: Option<u64>,
    #[serde(default)]
    browser_client: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GitHub session")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

impl Session {
    fn from_reply(profile: Profile, reply: TokenReply) -> Result<Self, Notice> {
        let access = reply
            .access_token
            .filter(|token| valid_token(token))
            .ok_or(Notice::InvalidResponse)?;
        let refresh = reply.refresh_token.filter(|token| valid_token(token));
        let now = unix_time();
        Ok(Self {
            account: profile.id,
            login: profile.login,
            name: profile.name,
            access,
            refresh,
            expires: reply.expires_in.map(|time| now.saturating_add(time)),
            refresh_expires: reply
                .refresh_token_expires_in
                .map(|time| now.saturating_add(time)),
            browser_client: None,
            scopes: parse_scopes(reply.scope.as_deref()),
        })
    }

    #[must_use]
    pub fn profile(&self) -> Profile {
        Profile {
            id: self.account,
            login: self.login.clone(),
            name: self.name.clone(),
        }
    }

    #[must_use]
    pub fn access(&self) -> &str {
        &self.access
    }

    #[must_use]
    pub fn needs_refresh(&self) -> bool {
        self.expires
            .is_some_and(|at| at <= unix_time().saturating_add(60))
    }

    pub fn from_vault(account: u64, bytes: &[u8]) -> Result<Self, Notice> {
        Self::restore(account, None, bytes)
    }

    pub fn from_browser_vault(
        account: u64,
        client: &ClientId,
        bytes: &[u8],
    ) -> Result<Self, Notice> {
        Self::restore(account, Some(client.as_str()), bytes)
    }

    pub fn check_access(&self) -> Result<(), Notice> {
        if self.browser_client.is_some() {
            check_scopes(&self.scopes)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    fn restore(account: u64, browser_client: Option<&str>, bytes: &[u8]) -> Result<Self, Notice> {
        let record: Self = serde_json::from_slice(bytes).map_err(|_| Notice::StorageUnavailable)?;
        if record.account != account
            || record.browser_client.as_deref() != browser_client
            || !valid_token(&record.access)
            || gitrust_core::github::RepositoryKey::new(&record.login, "profile").is_none()
        {
            return Err(Notice::StorageUnavailable);
        }
        Ok(record)
    }

    /// Serialize for the OS credential vault only.
    pub fn for_vault(&self) -> Result<Vec<u8>, Notice> {
        serde_json::to_vec(self).map_err(|_| Notice::StorageUnavailable)
    }
}

fn parse_scopes(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
        .collect()
}

fn check_scopes(scopes: &[String]) -> Result<(), Notice> {
    if !scopes.iter().any(|scope| scope == "repo")
        || !scopes
            .iter()
            .any(|scope| matches!(scope.as_str(), "read:user" | "user"))
    {
        return Err(Notice::InsufficientScope);
    }
    Ok(())
}

fn valid_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() < 4096
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
}

fn oauth_error(error: Option<&str>) -> Notice {
    match error {
        Some("expired_token" | "token_expired") => Notice::Expired,
        Some("access_denied") => Notice::Denied,
        Some("bad_refresh_token" | "invalid_grant") => Notice::Reconnect,
        Some("incorrect_client_credentials" | "device_flow_disabled") => Notice::InvalidClient,
        _ => Notice::InvalidResponse,
    }
}

impl Client {
    pub fn login(
        &self,
        client_id: &ClientId,
        stop: &Cancellation,
        show: impl FnOnce(DevicePrompt),
    ) -> Result<Session, Notice> {
        let code = self
            .request::<DeviceReply>(
                self.oauth,
                "/login/device/code",
                Some(json!({"client_id": client_id.as_str()})),
                None,
                stop,
            )
            .map_err(|error| match error {
                Notice::NoAccess => Notice::InvalidClient,
                other => other,
            })?
            .body;
        if code.verification_uri != super::DEVICE_URL
            || !valid_token(&code.device_code)
            || !valid_token(&code.user_code)
            || code.user_code.len() > 32
            || !(1..=1800).contains(&code.expires_in)
            || !(1..=1800).contains(&code.interval)
        {
            return Err(Notice::InvalidResponse);
        }
        let deadline = Instant::now() + Duration::from_secs(code.expires_in);
        show(DevicePrompt {
            code: code.user_code,
            expires_in: code.expires_in,
        });
        let mut interval = Duration::from_secs(code.interval);
        loop {
            stop.wait(interval.min(deadline.saturating_duration_since(Instant::now())))?;
            if Instant::now() >= deadline {
                return Err(Notice::Expired);
            }
            let reply = self
                .request::<TokenReply>(
                    self.oauth,
                    "/login/oauth/access_token",
                    Some(json!({
                        "client_id": client_id.as_str(), "device_code": code.device_code,
                        "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    })),
                    None,
                    stop,
                )?
                .body;
            stop.check()?;
            match reply.error.as_deref() {
                Some("authorization_pending") => {}
                Some("slow_down") => {
                    interval = (interval + Duration::from_secs(5))
                        .max(Duration::from_secs(reply.interval.unwrap_or(0).min(1800)));
                }
                Some(_) => return Err(oauth_error(reply.error.as_deref())),
                None => {
                    let token = reply
                        .access_token
                        .as_deref()
                        .filter(|token| valid_token(token))
                        .ok_or(Notice::InvalidResponse)?;
                    let profile = self.profile(token, stop)?;
                    stop.check()?;
                    return Session::from_reply(profile, reply);
                }
            }
        }
    }

    pub fn exchange_browser(
        &self,
        client: &ClientId,
        secret: &str,
        attempt: BrowserAttempt,
        callback: Callback,
        stop: &Cancellation,
        progress: impl Fn(BrowserState),
    ) -> Result<Session, Notice> {
        stop.check()?;
        let (code, verifier) = attempt.exchange_parts(callback)?;
        progress(BrowserState::ExchangingCode);
        let reply = self
            .request::<TokenReply>(
                self.oauth,
                "/login/oauth/access_token",
                Some(json!({
                    "client_id": client.as_str(),
                    "client_secret": secret,
                    "code": code,
                    "code_verifier": verifier,
                    "redirect_uri": REDIRECT_URI,
                    "grant_type": "authorization_code",
                })),
                None,
                stop,
            )?
            .body;
        if let Some(error) = reply.error.as_deref() {
            return Err(match oauth_error(Some(error)) {
                Notice::InvalidClient => Notice::OAuthConfiguration,
                notice => notice,
            });
        }
        let scopes = parse_scopes(reply.scope.as_deref());
        check_scopes(&scopes)?;
        let access = reply
            .access_token
            .as_deref()
            .filter(|token| valid_token(token))
            .ok_or(Notice::InvalidResponse)?;
        stop.check()?;
        progress(BrowserState::FetchingProfile);
        let profile = self.profile(access, stop)?;
        stop.check()?;
        let mut session = Session::from_reply(profile, reply)?;
        session.browser_client = Some(client.as_str().into());
        Ok(session)
    }

    pub fn refresh_browser(
        &self,
        client: &ClientId,
        secret: &str,
        current: &Session,
        stop: &Cancellation,
    ) -> Result<Session, Notice> {
        if current.browser_client.as_deref() != Some(client.as_str()) {
            return Err(Notice::Reconnect);
        }
        self.rotate(client, Some(secret), current, stop)
    }

    pub fn refresh(
        &self,
        client_id: &ClientId,
        current: &Session,
        stop: &Cancellation,
    ) -> Result<Session, Notice> {
        if current.browser_client.is_some() {
            return Err(Notice::Reconnect);
        }
        self.rotate(client_id, None, current, stop)
    }

    fn rotate(
        &self,
        client_id: &ClientId,
        secret: Option<&str>,
        current: &Session,
        stop: &Cancellation,
    ) -> Result<Session, Notice> {
        let refresh = current
            .refresh
            .as_deref()
            .filter(|token| valid_token(token))
            .ok_or(Notice::Reconnect)?;
        if current.refresh_expires.is_some_and(|at| at <= unix_time()) {
            return Err(Notice::Reconnect);
        }
        let mut params = json!({
            "client_id": client_id.as_str(),
            "grant_type": "refresh_token",
            "refresh_token": refresh,
        });
        if let Some(secret) = secret {
            params["client_secret"] = json!(secret);
        }
        let reply = self
            .request::<TokenReply>(
                self.oauth,
                "/login/oauth/access_token",
                Some(params),
                None,
                stop,
            )?
            .body;
        if let Some(error) = reply.error.as_deref() {
            return Err(oauth_error(Some(error)));
        }
        // The previous refresh token has already rotated. Return the replacement
        // pair for immediate vault persistence before making another network call.
        let retain_scopes = reply.scope.is_none();
        let mut replacement = Session::from_reply(current.profile(), reply)?;
        replacement
            .browser_client
            .clone_from(&current.browser_client);
        if retain_scopes {
            replacement.scopes.clone_from(&current.scopes);
        }
        Ok(replacement)
    }
}

#[must_use]
pub fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
