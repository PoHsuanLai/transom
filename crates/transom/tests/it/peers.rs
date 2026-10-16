use transom::peers::{Admission, FRONTEND_NAME, HOLD_LIMITS, Limits, Tether, admit, departed};

/// A name, its old owner, its new owner, and the peer taken off (if any).
type OwnerChange<'a> = (&'a str, Option<&'a str>, Option<&'a str>, Option<&'a str>);

#[test]
fn a_change_of_owner_takes_a_peer_off_only_when_it_has_left_or_stopped_being_the_frontend() {
    let table: [OwnerChange; 8] = [
        // A peer disconnects.
        (":1.7", Some(":1.7"), None, Some(":1.7")),
        // A peer connects.
        (":1.7", None, Some(":1.7"), None),
        // The frontend's name goes: that owner is no longer the frontend.
        (FRONTEND_NAME, Some(":1.2"), None, Some(":1.2")),
        // The frontend's name changes hands.
        (FRONTEND_NAME, Some(":1.2"), Some(":1.9"), Some(":1.2")),
        // A new frontend appears: nothing left.
        (FRONTEND_NAME, None, Some(":1.9"), None),
        // Some other well-known name changing hands is not our business.
        ("org.example.App", Some(":1.3"), None, None),
        ("org.example.App", Some(":1.3"), Some(":1.4"), None),
        // Nothing changed.
        (FRONTEND_NAME, Some(":1.2"), Some(":1.2"), None),
    ];
    for (name, old, new, want) in table {
        assert_eq!(
            departed(name, old, new).as_deref(),
            want,
            "{name} {old:?} -> {new:?}"
        );
    }
}

#[test]
fn limits_count_per_caller_and_in_total() {
    let limits = Limits {
        total: 3,
        per_caller: 2,
    };
    let a = Tether::new(Some(":1.1".into()), None);
    let b = Tether::new(Some(":1.2".into()), None);
    let unknown = Tether::default();
    assert_eq!(admit([&a, &b], &a, limits), Admission::Allowed);
    assert_eq!(admit([&a, &a], &a, limits), Admission::Full);
    assert_eq!(
        admit([&a, &a, &b], &b, limits),
        Admission::Full,
        "the book is full"
    );
    assert_eq!(
        admit([&unknown, &unknown], &unknown, limits),
        Admission::Allowed,
        "callers we cannot name are only held to the total"
    );
    const { assert!(HOLD_LIMITS.per_caller <= HOLD_LIMITS.total) };
}
