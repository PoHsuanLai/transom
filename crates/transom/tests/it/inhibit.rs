use transom::inhibit::{FakeEffect, InhibitHub, MonitorAdded, MonitorKnown, logind_what};
use transom_core::{InhibitFlags, RequestHandle};

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

    hub.hold(handle("a"), idle).await.expect("hold a");
    hub.hold(handle("b"), idle)
        .await
        .expect("hold b, same union");
    hub.hold(handle("c"), InhibitFlags::SUSPEND)
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
    assert!(hub.monitor("/s").expect("monitor").answered_query_end);
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
