use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use transom::chooser::FakeChooser;
use transom::remote_desktop::{
    ConnectError, Consent, ConsentAnswer, DeviceKinds, FakeConsent, FakeEis, RemoteDesktopService,
    ShellConsent,
};
use transom::screencast::Dict;
use transom::screencast::session::SessionKey;
use transom_core::{CastSource, CursorMode};
use zbus::zvariant::{OwnedValue, Value};

fn key(n: u32) -> SessionKey {
    SessionKey(format!("/org/freedesktop/portal/desktop/session/x/{n}"))
}

fn service(answers: Vec<ConsentAnswer>) -> (RemoteDesktopService, Arc<FakeConsent>, Arc<FakeEis>) {
    let consent = Arc::new(FakeConsent::new(answers));
    let eis = Arc::new(FakeEis::new());
    let service = RemoteDesktopService::new(Box::new(consent.clone()), Box::new(eis.clone()));
    (service, consent, eis)
}

fn types(bits: u32) -> Dict {
    let mut options = Dict::new();
    options.insert(
        "types".to_owned(),
        OwnedValue::try_from(Value::from(bits)).expect("a number"),
    );
    options
}

fn devices_of(results: &Dict) -> Option<u32> {
    results
        .get("devices")
        .and_then(|v| v.downcast_ref::<u32>().ok())
}

#[test]
fn a_session_is_opened_once_and_devices_are_chosen_before_start() {
    let (service, _, _) = service(vec![]);
    assert_eq!(service.create(key(1), "org.example.Remote").0, 0);
    assert_eq!(
        service.create(key(1), "org.example.Remote").0,
        2,
        "a handle is opened once"
    );
    assert_eq!(service.select(&key(1), &types(3)).0, 0);
    assert_eq!(service.select(&key(9), &types(3)).0, 2, "no such session");
    assert_eq!(service.len(), 1);
}

#[tokio::test]
async fn the_person_allows_and_the_devices_are_those_chosen() {
    let (service, consent, _) = service(vec![ConsentAnswer::Allowed]);
    service.create(key(1), "org.example.Remote");
    service.select(&key(1), &types(1 | 4));
    let (response, results) = service.begin(&key(1)).await;
    assert_eq!(response, 0);
    assert_eq!(devices_of(&results), Some(5));
    assert_eq!(
        consent.asked(),
        vec![("org.example.Remote".to_owned(), DeviceKinds::from_wire(5))]
    );
    assert_eq!(
        service.select(&key(1), &types(7)).0,
        2,
        "devices cannot change after the person has allowed"
    );
}

#[tokio::test]
async fn no_choice_means_every_kind_and_bits_we_do_not_offer_are_dropped() {
    let (service, _, _) = service(vec![ConsentAnswer::Allowed, ConsentAnswer::Allowed]);
    service.create(key(1), "a");
    assert_eq!(devices_of(&service.begin(&key(1)).await.1), Some(7));
    service.create(key(2), "b");
    service.select(&key(2), &types(0xff));
    assert_eq!(devices_of(&service.begin(&key(2)).await.1), Some(7));
}

#[tokio::test]
async fn declining_ends_the_session_and_an_empty_script_declines() {
    let (service, _, eis) = service(vec![ConsentAnswer::Declined]);
    service.create(key(1), "a");
    assert_eq!(service.begin(&key(1)).await.0, 1);
    assert!(service.is_empty(), "a declined session is gone");
    service.create(key(2), "b");
    assert_eq!(service.begin(&key(2)).await.0, 1, "nobody said yes");
    assert!(eis.asked().is_empty(), "no connection was asked for");
    assert_eq!(service.begin(&key(3)).await.0, 2, "no such session");
}

#[tokio::test]
async fn an_app_without_a_name_is_still_named_to_the_person() {
    let (service, consent, _) = service(vec![ConsentAnswer::Allowed]);
    service.create(key(1), "");
    service.begin(&key(1)).await;
    assert_eq!(consent.asked()[0].0, "An app");
}

#[tokio::test]
async fn a_socket_is_handed_out_only_after_the_person_allowed_it() {
    let (service, _, eis) = service(vec![ConsentAnswer::Allowed]);
    assert!(matches!(
        service.connect(&key(1)).await,
        Err(ConnectError::NoSession)
    ));
    service.create(key(1), "a");
    service.select(&key(1), &types(3));
    assert!(matches!(
        service.connect(&key(1)).await,
        Err(ConnectError::NotAllowed)
    ));
    assert!(eis.asked().is_empty());
    service.begin(&key(1)).await;
    let fd = service.connect(&key(1)).await.expect("a socket");
    assert_eq!(eis.asked(), vec![(1, DeviceKinds::from_wire(3))]);

    let mut client = UnixStream::from(fd);
    let mut server = eis.take_server().expect("the compositor's end");
    server.write_all(b"hi").expect("write");
    let mut got = [0_u8; 2];
    client.read_exact(&mut got).expect("read");
    assert_eq!(&got, b"hi");

    service.close(&key(1));
    assert!(matches!(
        service.connect(&key(1)).await,
        Err(ConnectError::NoSession)
    ));
}

#[tokio::test]
async fn each_session_has_its_own_number_for_the_compositor() {
    let (service, _, eis) = service(vec![ConsentAnswer::Allowed, ConsentAnswer::Allowed]);
    for n in [1, 2] {
        service.create(key(n), "a");
        service.begin(&key(n)).await;
        service.connect(&key(n)).await.expect("a socket");
    }
    let numbers: Vec<u64> = eis.asked().iter().map(|(n, _)| *n).collect();
    assert_eq!(numbers, vec![1, 2]);
}

#[tokio::test]
async fn the_shell_consent_is_yes_for_a_pick_and_no_for_a_cancel() {
    let pick = (vec![CastSource::Output("A".into())], CursorMode::Hidden);
    let consent = ShellConsent::new(Box::new(FakeChooser::new(vec![Some(pick), None])));
    let kinds = DeviceKinds::ALL;
    assert_eq!(
        consent.ask("Remote", kinds).await.expect("answer"),
        ConsentAnswer::Allowed
    );
    assert_eq!(
        consent.ask("Remote", kinds).await.expect("answer"),
        ConsentAnswer::Declined
    );
}
