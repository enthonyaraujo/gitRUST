use super::*;
use std::{
    io::{BufRead as _, BufReader, Read as _, Write as _},
    net::TcpListener,
    sync::mpsc,
    thread,
};

struct Reply {
    method: &'static str,
    path: &'static str,
    code: u16,
    headers: &'static str,
    body: Value,
}

fn reply(method: &'static str, path: &'static str, code: u16, body: Value) -> Reply {
    Reply {
        method,
        path,
        code,
        body,
        headers: "",
    }
}

fn server(replies: Vec<Reply>) -> (Client, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
    let origin =
        Box::leak(format!("http://{}", listener.local_addr().expect("address")).into_boxed_str());
    let (received, read) = mpsc::channel();
    let worker = thread::spawn(move || {
        for reply in replies {
            let (stream, _) = listener.accept().expect("request");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("bounded reads");
            let mut input = BufReader::new(stream);
            let mut first = String::new();
            input.read_line(&mut first).expect("request line");
            assert!(
                first.starts_with(&format!("{} {} HTTP/", reply.method, reply.path)),
                "{first}"
            );
            let mut request = first;
            let mut len = 0;
            loop {
                let mut header = String::new();
                input.read_line(&mut header).expect("header");
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length: ") {
                    len = value.trim().parse().expect("content length");
                }
                assert!(!header.is_empty(), "unterminated header");
                request.push_str(&header);
            }
            let mut body = vec![0; len];
            input.read_exact(&mut body).expect("request body");
            request.push_str(&String::from_utf8_lossy(&body));
            let _ = received.send(request);
            let payload = reply.body.to_string();
            write!(input.get_mut(), "HTTP/1.1 {} Fixture\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{}\r\n{}",
                reply.code, payload.len(), reply.headers, payload).expect("response");
        }
    });
    (Client::new(false, origin, origin), read, worker)
}

fn profile(number: u64, login: &str) -> Value {
    json!({"id": number, "login": login, "name": "Example"})
}

fn repository() -> RepositoryKey {
    RepositoryKey::new("owner", "private-project").expect("repository key")
}

fn repo() -> Value {
    json!({"name": "private-project", "owner": profile(1, "owner"), "private": true,
        "default_branch": "main", "permissions": {"push": true}, "updated_at": "2026-11-26"})
}

fn device(expires: u64) -> Value {
    json!({"device_code":"fixture-device","user_code":"ABCD-EFGH","verification_uri":DEVICE_URL,"expires_in":expires,"interval":1})
}

fn token(label: &str) -> Value {
    json!({"access_token": format!("fixture-{label}"), "refresh_token": format!("refresh-{label}"),
        "expires_in": 5, "refresh_token_expires_in":30})
}

const CLIENT: &str = "Iv23fixtureId";

#[test]
fn device_flow_waits_for_authorization_and_rotates_the_bound_session() {
    let (client, requests, work) = server(vec![
        reply("POST", "/login/device/code", 200, device(15)),
        reply(
            "POST",
            "/login/oauth/access_token",
            200,
            json!({"error":"authorization_pending"}),
        ),
        reply(
            "POST",
            "/login/oauth/access_token",
            200,
            json!({"error":"slow_down","interval":2}),
        ),
        reply("POST", "/login/oauth/access_token", 200, token("first")),
        reply("GET", "/user", 200, profile(1, "owner")),
        reply("POST", "/login/oauth/access_token", 200, token("second")),
    ]);
    let id = ClientId::new(CLIENT).expect("Client ID");
    let stop = Cancellation::default();
    let start = std::time::Instant::now();
    let session = client
        .login(&id, &stop, |prompt| assert_eq!(prompt.code, "ABCD-EFGH"))
        .expect("authorize account");
    assert!(start.elapsed() >= Duration::from_secs(8));
    assert_eq!(session.profile().id, 1);
    assert!(session.needs_refresh());
    let refreshed = client.refresh(&id, &session, &stop).expect("rotate token");
    assert_eq!(refreshed.access(), "fixture-second");
    let sealed = refreshed.for_vault().expect("vault export");
    assert!(Session::from_vault(2, &sealed).is_err());
    assert_eq!(
        Session::from_vault(1, &sealed)
            .expect("bound session")
            .access(),
        "fixture-second"
    );
    assert!(!format!("{refreshed:?}").contains("fixture-second"));
    work.join().expect("fixture complete");
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert!(!requests[0].contains("authorization:"));
    assert!(
        requests[4]
            .to_lowercase()
            .contains("authorization: bearer fixture-first")
    );
    assert!(requests[5].contains("refresh-first"));
    assert!(!requests[5].contains("client_secret"));
}

#[test]
fn authorization_can_cancel_expire_or_be_denied() {
    for expected in [Notice::Cancelled, Notice::Expired, Notice::Denied] {
        let mut replies = vec![reply(
            "POST",
            "/login/device/code",
            200,
            device(if expected == Notice::Expired { 1 } else { 10 }),
        )];
        if expected == Notice::Denied {
            replies.push(reply(
                "POST",
                "/login/oauth/access_token",
                200,
                json!({"error":"access_denied"}),
            ));
        }
        let (client, _, work) = server(replies);
        let stop = Cancellation::default();
        let result = client.login(&ClientId::new(CLIENT).expect("client ID"), &stop, |_| {
            if expected == Notice::Cancelled {
                stop.cancel();
            }
        });
        assert_eq!(result.expect_err("authorization failed"), expected);
        work.join().expect("fixture complete");
    }
}

#[test]
fn private_repository_pages_do_not_follow_returned_urls_or_cross_account_tokens() {
    let mut first = reply(
        "GET",
        "/user/installations/9/repositories?per_page=30&page=1",
        200,
        json!({"repositories":[repo()]}),
    );
    first.headers = "Link: <https://unapproved.invalid/repositories?page=2>; rel=\"next\"\r\n";
    let (client, requests, work) = server(vec![
        reply(
            "GET",
            "/user/installations?per_page=30&page=1",
            200,
            json!({"installations":[{"id":9,"account": profile(2,"organization")}]}),
        ),
        first,
        reply(
            "GET",
            "/user/installations/9/repositories?per_page=30&page=2",
            403,
            json!({}),
        ),
    ]);
    let stop = Cancellation::default();
    let installations = client
        .installations("first-account", 1, &stop)
        .expect("authorized organization");
    assert_eq!(installations.items[0].login, "organization");
    let page = client
        .repositories("first-account", Some(9), 1, &stop)
        .expect("authorized page");
    assert!(page.items[0].private);
    assert_eq!(page.next, Some(2));
    assert_eq!(
        client
            .repositories("second-account", Some(9), 2, &stop)
            .expect_err("permission denied"),
        Notice::Forbidden
    );
    work.join().expect("fixture complete");
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert!(
        requests[1]
            .to_lowercase()
            .contains("authorization: bearer first-account")
    );
    assert!(
        requests[2]
            .to_lowercase()
            .contains("authorization: bearer second-account")
    );
    assert!(!requests[2].contains("first-account"));
}

#[test]
fn redirection_revocation_rate_limit_and_bad_payload_are_distinct() {
    let mut redirect = reply("GET", "/user", 302, json!({}));
    redirect.headers = "Location: https://unapproved.invalid/user\r\n";
    let mut limit = reply("GET", "/user", 429, json!({}));
    limit.headers = "Retry-After: 17\r\n";
    let (client, _, work) = server(vec![
        redirect,
        limit,
        reply("GET", "/user", 401, json!({})),
        reply("GET", "/user", 200, json!({"id":"invalid"})),
    ]);
    let stop = Cancellation::default();
    for expected in [
        Notice::Redirect,
        Notice::RateLimited(17),
        Notice::Reconnect,
        Notice::InvalidResponse,
    ] {
        assert_eq!(
            client
                .profile("fixture", &stop)
                .expect_err("request failed"),
            expected
        );
    }
    work.join().expect("fixture complete");
    assert!(browser_url_allowed("https://github.com/owner/repo/actions"));
    assert!(!browser_url_allowed(
        "https://github.com@unapproved.invalid/project"
    ));
    assert!(!browser_url_allowed(
        "https://github.com/login?return_to=https://unapproved.invalid"
    ));
}

#[test]
fn creates_private_repository_fork_and_draft_without_retrying_mutations() {
    let details = json!({"number":4,"title":"Draft test","user":profile(1,"owner"),
        "state":"open","draft":true,
        "head":{"ref":"topic","sha":"a".repeat(40),"repo":repo()},
        "base":{"ref":"main","sha":"b".repeat(40),"repo":repo()}});
    let (client, requests, work) = server(vec![
        reply("POST", "/user/repos", 201, repo()),
        reply("POST", "/repos/owner/private-project/forks", 202, repo()),
        reply(
            "POST",
            "/repos/owner/private-project/pulls",
            201,
            details.clone(),
        ),
        reply(
            "GET",
            "/repos/owner/private-project/pulls?per_page=30&page=1&state=open",
            200,
            json!([details]),
        ),
        reply("POST", "/user/repos", 503, json!({})),
    ]);
    let stop = Cancellation::default();
    let repo = repository();
    let create = CreateRepository {
        owner: repo.owner().into(),
        name: repo.name().into(),
        description: "Fixture".into(),
        private: true,
        local: None,
    };
    assert_eq!(
        client
            .create_repository("fixture", &create, false, &stop)
            .expect("created")
            .key,
        repo
    );
    client
        .fork("fixture", &repo, None, repo.name(), &stop)
        .expect("fork accepted");
    let pull = client
        .create_pull(
            "fixture",
            &CreatePull {
                destination: repo.clone(),
                base: "main".into(),
                source: repo.clone(),
                head: "topic".into(),
                title: "Draft test".into(),
                body: String::new(),
                draft: true,
            },
            &stop,
        )
        .expect("draft created");
    assert!(pull.draft);
    assert_eq!(
        client
            .pulls("fixture", &repo, 1, PullFilter::Open, &stop)
            .expect("pull list")
            .items[0],
        pull
    );
    assert_eq!(
        client
            .create_repository("fixture", &create, false, &stop)
            .expect_err("response uncertain"),
        Notice::UnknownMutation
    );
    work.join().expect("fixture complete");
    let requests = requests.try_iter().collect::<Vec<_>>();
    assert!(requests[0].contains("\"private\":true"));
    assert!(requests[2].contains("\"draft\":true"));
}

#[test]
fn checks_distinguish_pending_from_no_results_and_workflows_keep_conclusions() {
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let (client, _, work) = server(vec![
        reply(
            "GET",
            "/repos/owner/private-project/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/check-runs?per_page=30&page=1",
            200,
            json!({"check_runs":[{"name":"build","status":"in_progress","conclusion":null}]}),
        ),
        reply(
            "GET",
            "/repos/owner/private-project/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/status?per_page=30&page=1",
            200,
            json!({"statuses":[{"context":"deploy","state":"pending"}]}),
        ),
        reply(
            "GET",
            "/repos/owner/private-project/actions/runs?per_page=30&page=1&head_sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            200,
            json!({"workflow_runs":[{"name":"CI","status":"completed","conclusion":"neutral"}]}),
        ),
    ]);
    let repo = repository();
    let stop = Cancellation::default();
    let checks = client.checks("fixture", &repo, sha, &stop).expect("checks");
    assert_eq!(checks.len(), 2);
    assert!(checks.iter().all(|check| check.conclusion.is_none()));
    let runs = client
        .workflow_runs("fixture", &repo, sha, &stop)
        .expect("workflow runs");
    assert_eq!(runs[0].conclusion.as_deref(), Some("neutral"));
    work.join().expect("fixture complete");
}

#[test]
fn production_transport_requires_tls_before_transmitting_credentials() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
    listener.set_nonblocking(true).expect("non-blocking probe");
    let origin = Box::leak(
        format!("http://{}", listener.local_addr().expect("fixture address")).into_boxed_str(),
    );
    let client = Client::new(true, origin, origin);
    assert_eq!(
        client
            .profile("fixture-secret", &Cancellation::default())
            .expect_err("plaintext is prohibited"),
        Notice::Offline
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}
