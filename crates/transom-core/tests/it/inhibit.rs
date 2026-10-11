use transom_core::{InhibitBook, InhibitFlags, RequestHandle};

fn handle(n: u32) -> RequestHandle {
    RequestHandle(format!("/org/freedesktop/portal/desktop/request/x/{n}"))
}

#[test]
fn flags_from_the_wire_drop_unknown_bits() {
    assert_eq!(InhibitFlags::from_wire(0xff).to_wire(), 15);
    assert!(InhibitFlags::from_wire(12).contains(InhibitFlags::IDLE));
    assert!(!InhibitFlags::from_wire(4).contains(InhibitFlags::IDLE));
}

#[test]
fn the_union_follows_the_requests() {
    let mut book = InhibitBook::new();
    let c = book.hold(handle(1), InhibitFlags::IDLE);
    assert!(c.is_change());
    assert_eq!(book.active(), InhibitFlags::IDLE);

    let c = book.hold(handle(2), InhibitFlags::IDLE);
    assert!(
        !c.is_change(),
        "a second holder of the same flag changes nothing"
    );

    let c = book.hold(handle(3), InhibitFlags::SUSPEND);
    assert_eq!(c.after, InhibitFlags::IDLE.union(InhibitFlags::SUSPEND));

    assert!(
        !book.release(&handle(1)).is_change(),
        "handle 2 still holds idle"
    );
    assert!(book.release(&handle(2)).is_change());
    assert_eq!(book.active(), InhibitFlags::SUSPEND);

    assert!(
        !book.release(&handle(99)).is_change(),
        "an unknown handle changes nothing"
    );
    book.release(&handle(3));
    assert!(book.is_empty());
    assert_eq!(book.active(), InhibitFlags::NONE);
}

#[test]
fn holding_a_handle_again_replaces_its_flags() {
    let mut book = InhibitBook::new();
    book.hold(handle(1), InhibitFlags::IDLE);
    book.hold(handle(1), InhibitFlags::LOGOUT);
    assert_eq!(book.len(), 1);
    assert_eq!(book.active(), InhibitFlags::LOGOUT);
}
