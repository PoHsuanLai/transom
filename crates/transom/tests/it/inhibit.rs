use transom::BoxFuture;
use transom::inhibit::{
    EffectError, FakeEffect, HoldError, InhibitEffect, InhibitHub, MAX_MONITORS, MonitorAdded,
    MonitorKnown, QueryEnd, logind_what,
};
use transom::peers::{HOLD_LIMITS, Tether};
use transom_core::{InhibitFlags, RequestHandle};

/// A caller `n`, relayed by the frontend `:1.2`.
fn tether(n: u32) -> Tether {
    Tether::new(Some(format!(":1.{n}")), Some(":1.2".to_owned()))
}

fn handle(n: &str) -> RequestHandle {
    RequestHandle(format!("/org/freedesktop/portal/desktop/request/{n}"))
}

#[tokio::test]
async fn the_session_is_told_only_when_the_union_changes() {
    let fake = FakeEffect::new();
    let log = fake.log();
    let mut hub = InhibitHub::new(Box::new(fake));
    let idle = InhibitFlags::IDLE;
    let both = idle.union(InhibitFlags::SUSPEND);

    hub.hold(handle("a"), idle, tether(10))
        .await
        .expect("hold a");
    hub.hold(handle("b"), idle, tether(11))
        .await
        .expect("hold b, same union");
    hub.hold(handle("c"), InhibitFlags::SUSPEND, tether(12))
        .await
        .expect("hold c");
    hub.release(&handle("a"))
        .await
        .expect("release a, b still idles");
    hub.release(&handle("nobody"))
        .await
        .expect("release of nothing");
    hub.release(&handle("b")).await.expect("release b");
    hub.release(&handle("c")).await.expect("release c");

    let told = log.lock().expect("log").clone();
    assert_eq!(
        told,
        vec![idle, both, InhibitFlags::SUSPEND, InhibitFlags::NONE]
    );
    assert_eq!(hub.held(), 0);
    assert_eq!(hub.active(), InhibitFlags::NONE);
}

#[tokio::test]
async fn a_monitor_is_known_until_removed() {
    let mut hub = InhibitHub::new(Box::new(FakeEffect::new()));
    assert_eq!(hub.note_answer("/s"), MonitorKnown::Unknown);
    assert_eq!(hub.add_monitor("/s"), MonitorAdded::New);
    assert_eq!(hub.add_monitor("/s"), MonitorAdded::AlreadyThere);
    assert_eq!(hub.note_answer("/s"), MonitorKnown::Known);
    assert_eq!(
        hub.monitor("/s").expect("monitor").query_end,
        QueryEnd::Answered
    );
    hub.remove_monitor("/s");
    assert!(hub.monitor("/s").is_none());
}

#[test]
fn flags_become_the_login_managers_words() {
    let table = [
        (InhibitFlags::NONE, ""),
        (InhibitFlags::IDLE, "idle"),
        (InhibitFlags::SUSPEND, "sleep"),
        (InhibitFlags::LOGOUT, "shutdown"),
        (InhibitFlags::USER_SWITCH, ""),
        (
            InhibitFlags::IDLE.union(InhibitFlags::SUSPEND),
            "sleep:idle",
        ),
    ];
    for (flags, want) in table {
        assert_eq!(logind_what(flags), want, "{flags:?}");
    }
}

#[tokio::test]
async fn holds_end_with_the_peer_they_hang_on() {
    let fake = FakeEffect::new();
    let log = fake.log();
    let mut hub = InhibitHub::new(Box::new(fake));
    hub.hold(handle("a"), InhibitFlags::IDLE, tether(10))
        .await
        .expect("hold a");
    hub.hold(handle("b"), InhibitFlags::SUSPEND, tether(11))
        .await
        .expect("hold b");

    assert_eq!(hub.release_peer(":1.10").await, vec![handle("a")]);
    assert_eq!(hub.held(), 1);
    assert_eq!(hub.active(), InhibitFlags::SUSPEND);
    // The frontend leaving releases what is left, whoever asked.
    assert_eq!(hub.release_peer(":1.2").await, vec![handle("b")]);
    assert_eq!(hub.active(), InhibitFlags::NONE);
    assert!(hub.release_peer(":1.2").await.is_empty(), "once");
    assert_eq!(
        log.lock().expect("log").last(),
        Some(&InhibitFlags::NONE),
        "the session was told to let go"
    );
}

#[tokio::test]
async fn one_caller_cannot_hold_more_than_its_share() {
    let mut hub = InhibitHub::new(Box::new(FakeEffect::new()));
    for n in 0..HOLD_LIMITS.per_caller {
        hub.hold(handle(&n.to_string()), InhibitFlags::IDLE, tether(10))
            .await
            .expect("room");
    }
    let refused = hub
        .hold(handle("one-too-many"), InhibitFlags::IDLE, tether(10))
        .await;
    assert!(matches!(refused, Err(HoldError::TooMany)));
    hub.hold(handle("other"), InhibitFlags::IDLE, tether(11))
        .await
        .expect("another caller still fits");
    hub.hold(handle("0"), InhibitFlags::SUSPEND, tether(10))
        .await
        .expect("a handle already held is replaced, not counted twice");
    assert_eq!(hub.held(), HOLD_LIMITS.per_caller + 1);
}

/// An effect that cannot reach the session.
#[derive(Debug)]
struct Failing;

impl InhibitEffect for Failing {
    fn apply(&mut self, _: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>> {
        Box::pin(async { Err(EffectError::Session("down".to_owned())) })
    }
}

#[tokio::test]
async fn a_hold_the_session_was_not_told_about_is_not_kept() {
    let mut hub = InhibitHub::new(Box::new(Failing));
    let refused = hub.hold(handle("a"), InhibitFlags::IDLE, tether(10)).await;
    assert!(matches!(refused, Err(HoldError::Effect(_))));
    assert_eq!(hub.held(), 0);
    assert_eq!(hub.active(), InhibitFlags::NONE);
}

#[test]
fn monitors_are_capped() {
    let mut hub = InhibitHub::new(Box::new(FakeEffect::new()));
    for n in 0..MAX_MONITORS {
        assert_eq!(hub.add_monitor(&n.to_string()), MonitorAdded::New);
    }
    assert_eq!(hub.add_monitor("one-too-many"), MonitorAdded::Full);
    assert_eq!(hub.add_monitor("0"), MonitorAdded::AlreadyThere);
}
