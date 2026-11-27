use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crossbeam_channel::{Receiver, Sender, after, bounded, select_biased};
use gitrust_core::github::Notice;
use gitrust_providers::github::{BrowserAttempt, Callback, Cancellation};

#[cfg(target_os = "linux")]
#[path = "oauth_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{Instance, forward, register, start};

#[cfg(not(target_os = "linux"))]
pub(crate) struct Instance;

#[cfg(not(target_os = "linux"))]
pub(crate) fn start() -> std::io::Result<Option<Instance>> {
    Ok(Some(Instance))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn forward(_uri: &str) -> bool {
    false
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn register() -> Result<(), Notice> {
    Err(Notice::OAuthHandler)
}

type Authorization = (BrowserAttempt, Callback);

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
struct Pending {
    attempt: BrowserAttempt,
    cancellation: Cancellation,
    sender: Sender<Authorization>,
}

#[derive(Clone, Default)]
pub(crate) struct Inbox(Arc<Mutex<Option<Pending>>>);

impl Inbox {
    pub(crate) fn wait_for(
        &self,
        attempt: BrowserAttempt,
        cancellation: &Cancellation,
        open: impl FnOnce() -> Result<(), Notice>,
    ) -> Result<Authorization, Notice> {
        let (sender, receiver) = bounded(1);
        let deadline = attempt.deadline();
        {
            let mut pending = self.0.lock().map_err(|_| Notice::Unavailable)?;
            if pending.is_some() {
                return Err(Notice::Unavailable);
            }
            *pending = Some(Pending {
                attempt,
                cancellation: cancellation.clone(),
                sender,
            });
        }
        let _clear = ClearPending(self);
        cancellation.check()?;
        open()?;
        wait(&receiver, cancellation, deadline)
    }

    #[cfg(any(target_os = "linux", test))]
    fn route(&self, callback: Callback) -> bool {
        let Ok(mut slot) = self.0.lock() else {
            return false;
        };
        if !slot.as_ref().is_some_and(|pending| {
            pending.cancellation.check().is_ok()
                && Instant::now() < pending.attempt.deadline()
                && pending.attempt.matches(&callback)
        }) {
            return false;
        }
        slot.take()
            .is_some_and(|pending| pending.sender.try_send((pending.attempt, callback)).is_ok())
    }
}

struct ClearPending<'a>(&'a Inbox);

impl Drop for ClearPending<'_> {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.0.0.lock() {
            slot.take();
        }
    }
}

fn wait(
    receiver: &Receiver<Authorization>,
    cancellation: &Cancellation,
    deadline: Instant,
) -> Result<Authorization, Notice> {
    let expiry = after(deadline.saturating_duration_since(Instant::now()));
    select_biased! {
        recv(cancellation.signal()) -> _ => Err(Notice::Cancelled),
        recv(expiry) -> _ => Err(Notice::Expired),
        recv(receiver) -> value => value.map_err(|_| Notice::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gitrust_providers::github::{ClientId, REDIRECT_URI};

    pub(super) fn fixture() -> (BrowserAttempt, String) {
        let attempt = BrowserAttempt::new().expect("test attempt");
        let url = attempt.authorize_url(&ClientId::new("fixtureOAuth").expect("test client"));
        let state = url
            .split('&')
            .find_map(|field| field.strip_prefix("state="))
            .expect("state");
        (
            attempt,
            format!("{REDIRECT_URI}?state={state}&code=fixture-code"),
        )
    }

    #[test]
    fn routes_one_callback_and_rejects_replay_and_cancellation() {
        let inbox = Inbox::default();
        let cancel = Cancellation::default();
        let (attempt, uri) = fixture();
        let (_, wrong) = fixture();
        let callback = |value| Callback::parse(value).expect("fixture callback");
        assert!(!inbox.route(callback(&uri)));
        assert!(
            inbox
                .wait_for(attempt, &cancel, || {
                    assert!(!inbox.route(callback(&wrong)));
                    assert!(inbox.route(callback(&uri)));
                    assert!(!inbox.route(callback(&uri)));
                    Ok(())
                })
                .is_ok()
        );
        let (attempt, uri) = fixture();
        assert_eq!(
            inbox
                .wait_for(attempt, &cancel, || {
                    cancel.cancel();
                    assert!(!inbox.route(callback(&uri)));
                    Ok(())
                })
                .expect_err("cancelled"),
            Notice::Cancelled
        );
        assert!(!inbox.route(callback(&uri)));
        let (_sender, receiver) = bounded(1);
        assert_eq!(
            wait(&receiver, &Cancellation::default(), Instant::now()).expect_err("deadline"),
            Notice::Expired
        );
    }
}
