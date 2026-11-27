//! Authorization request and one-time callback validation for GitHub.com OAuth Apps.

use std::{
    fmt,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use gitrust_core::github::Notice;
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;

use super::ClientId;

pub const REDIRECT_URI: &str = "gitrust://oauth/callback";
pub const MAX_CALLBACK_BYTES: usize = 4096;
const GITHUB_OAUTH_ISSUER: &str = "https://github.com/login/oauth";
const REQUIRED_SCOPES: [&str; 2] = ["repo", "read:user"];

fn random_value() -> Result<String, Notice> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Notice::Unavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub struct BrowserAttempt {
    state: String,
    verifier: String,
    deadline: Instant,
}

impl fmt::Debug for BrowserAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GitHub OAuth attempt")
    }
}

impl BrowserAttempt {
    pub fn new() -> Result<Self, Notice> {
        Ok(Self {
            state: random_value()?,
            verifier: random_value()?,
            deadline: Instant::now() + Duration::from_secs(300),
        })
    }

    #[must_use]
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    #[must_use]
    pub fn authorize_url(&self, client: &ClientId) -> String {
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("client_id", client.as_str())
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("state", &self.state)
            .append_pair("code_challenge", &challenge(&self.verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair(
                "scope",
                &format!("{} offline_access", REQUIRED_SCOPES.join(" ")),
            )
            .append_pair("prompt", "select_account")
            .finish();
        format!("{GITHUB_OAUTH_ISSUER}/authorize?{query}")
    }

    #[must_use]
    pub fn matches(&self, callback: &Callback) -> bool {
        self.state
            .as_bytes()
            .ct_eq(callback.state.as_bytes())
            .into()
    }

    pub(super) fn exchange_parts(self, callback: Callback) -> Result<(String, String), Notice> {
        if Instant::now() >= self.deadline {
            return Err(Notice::Expired);
        }
        if !self.matches(&callback) {
            return Err(Notice::InvalidInput);
        }
        Ok((callback.code.ok_or(Notice::Denied)?, self.verifier))
    }
}

pub struct Callback {
    state: String,
    code: Option<String>,
}

impl fmt::Debug for Callback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GitHub OAuth callback")
    }
}

impl Callback {
    pub fn parse(uri: &str) -> Result<Self, Notice> {
        if uri.len() > MAX_CALLBACK_BYTES
            || !uri.bytes().all(|byte| byte.is_ascii_graphic())
            || uri.contains(['#', '\\'])
        {
            return Err(Notice::InvalidInput);
        }
        let query = uri
            .strip_prefix(REDIRECT_URI)
            .and_then(|suffix| suffix.strip_prefix('?'))
            .ok_or(Notice::InvalidInput)?;
        let mut state = None;
        let mut code = None;
        let mut issuer = None;
        let mut error = None;
        let mut description = None;
        let mut error_uri = None;
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            let target = match key.as_ref() {
                "state" => &mut state,
                "code" => &mut code,
                "iss" => &mut issuer,
                "error" => &mut error,
                "error_description" => &mut description,
                "error_uri" => &mut error_uri,
                _ => return Err(Notice::InvalidInput),
            };
            if target.replace(value).is_some() {
                return Err(Notice::InvalidInput);
            }
        }
        if issuer
            .as_deref()
            .is_some_and(|value| value != GITHUB_OAUTH_ISSUER)
        {
            return Err(Notice::InvalidInput);
        }
        let safe = |value: &str| {
            value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        };
        let state = state
            .filter(|value| value.len() == 43 && safe(value))
            .ok_or(Notice::InvalidInput)?
            .into_owned();
        let code = match (code, error, description, error_uri) {
            (Some(code), None, None, None) if (1..=1024).contains(&code.len()) && safe(&code) => {
                Some(code.into_owned())
            }
            (None, Some(error), _, _) if error == "access_denied" => None,
            _ => return Err(Notice::InvalidInput),
        };
        Ok(Self { state, code })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_vector_and_request_has_no_verifier_or_secret() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let attempt = BrowserAttempt::new().expect("random candidate");
        let other = BrowserAttempt::new().expect("second candidate");
        assert_ne!(attempt.state, other.state);
        assert_ne!(attempt.verifier, other.verifier);
        let url = attempt.authorize_url(&ClientId::new("testClientId").expect("test client"));
        assert!(url.starts_with("https://github.com/login/oauth/authorize?"));
        assert!(url.contains("redirect_uri=gitrust%3A%2F%2Foauth%2Fcallback"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(!url.contains(&attempt.verifier));
        assert!(!format!("{attempt:?}").contains(&attempt.state));
    }

    #[test]
    fn callback_accepts_the_github_issuer_with_the_expected_state() {
        let attempt = BrowserAttempt::new().expect("random candidate");
        let uri = format!(
            "{REDIRECT_URI}?code=fixture-value&state={}&iss=https%3A%2F%2Fgithub.com%2Flogin%2Foauth",
            attempt.state
        );
        let callback = Callback::parse(&uri).expect("GitHub's issuer parameter");
        assert!(attempt.matches(&callback));

        let denial = format!(
            "{REDIRECT_URI}?error=access_denied&state={}&iss={GITHUB_OAUTH_ISSUER}",
            attempt.state
        );
        let callback = Callback::parse(&denial).expect("GitHub's issuer on denial");
        assert_eq!(attempt.exchange_parts(callback), Err(Notice::Denied));
    }

    #[test]
    fn callback_checks_origin_parameters_state_and_deadline() {
        let mut attempt = BrowserAttempt::new().expect("random candidate");
        let uri = format!("{REDIRECT_URI}?code=fixture-value&state={}", attempt.state);
        for invalid in [
            uri.replace("gitrust:", "https:"),
            uri.replace("//oauth/", "//oauth@untrusted.invalid/"),
            uri.replace("/callback?", "/callback/other?"),
            format!("{uri}&code=again"),
            format!("{uri}&iss="),
            format!("{uri}&iss=https://example.invalid/login/oauth"),
            format!("{uri}&iss={GITHUB_OAUTH_ISSUER}/"),
            format!("{uri}&iss={GITHUB_OAUTH_ISSUER}&iss={GITHUB_OAUTH_ISSUER}"),
            format!("{uri}&unknown=other"),
            format!("{uri}&error=access_denied"),
            format!("{uri}#fragment"),
            uri.replace("fixture-value", "%00"),
        ] {
            assert!(Callback::parse(&invalid).is_err());
        }
        let callback = Callback::parse(&uri).expect("valid callback");
        assert!(attempt.matches(&callback));
        assert!(!format!("{callback:?}").contains("fixture-value"));
        assert!(
            !BrowserAttempt::new()
                .expect("another attempt")
                .matches(&callback)
        );
        attempt.deadline = Instant::now();
        assert_eq!(attempt.exchange_parts(callback), Err(Notice::Expired));
    }
}
