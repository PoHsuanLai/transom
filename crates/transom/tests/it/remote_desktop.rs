use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use transom::BoxFuture;
use transom::access::{Answer, Asker, FakeAsker, Question};
use transom::chooser::ChooserError;
use transom::peers::Tether;
use transom::remote_desktop::{
    ConnectError, Consent, ConsentAnswer, ConsentError, DeviceKinds, FakeConsent, FakeEis,
    RemoteDesktopService, ShellConsent,
};
use transom::screencast::Dict;
use transom::screencast::session::SessionKey;
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

    service.close(&key(1)).await;
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
async fn the_shell_consent_asks_its_own_question_naming_the_input() {
    let asker = Arc::new(FakeAsker::new(vec![
        Answer::Granted,
        Answer::Denied,
        Answer::Dismissed,
    ]));
    let consent = ShellConsent::new(Box::new(asker.clone()));
    let kinds = DeviceKinds::from_wire(3);
    let answers = [
        consent.ask("Remote", kinds).await.expect("answer"),
        consent.ask("Remote", kinds).await.expect("answer"),
        consent.ask("Remote", kinds).await.expect("answer"),
    ];
    assert_eq!(
        answers,
        [
            ConsentAnswer::Allowed,
            ConsentAnswer::Declined,
            ConsentAnswer::Declined
        ],
        "only the yes button is a yes; closing the sheet is not"
    );
    let first = &asker.asked()[0];
    assert_eq!(first.app_label, "Remote");
    assert!(
        first.body.contains("keyboard and pointer"),
        "{}",
        first.body
    );
    assert!(!first.body.contains("touchscreen"));
    assert!(!first.title.to_lowercase().contains("share"));
    let all = DeviceKinds::ALL.phrase();
    assert_eq!(all, "keyboard, pointer and touchscreen");
}

/// An asker with nobody behind it: the shell is not there.
#[derive(Debug)]
struct Absent;

impl Asker for Absent {
    fn ask<'a>(&'a self, _: &'a Question) -> BoxFuture<'a, Result<Answer, ChooserError>> {
        Box::pin(async { Err(ChooserError::Unavailable) })
    }
}

#[tokio::test]
async fn with_no_shell_to_ask_nothing_is_granted_and_the_session_can_be_asked_again() {
    let eis = Arc::new(FakeEis::new());
    let service = RemoteDesktopService::new(
        Box::new(ShellConsent::new(Box::new(Absent))),
        Box::new(eis.clone()),
    );
    service.create(key(1), "a");
    assert_eq!(service.begin(&key(1)).await.0, 2);
    assert!(matches!(
        service.connect(&key(1)).await,
        Err(ConnectError::NotAllowed)
    ));
    assert_eq!(service.select(&key(1), &types(1)).0, 0, "still choosing");
}

#[tokio::test]
async fn asking_for_no_device_at_all_asks_nothing() {
    let (service, consent, _) = service(vec![ConsentAnswer::Allowed]);
    service.create(key(1), "a");
    service.select(&key(1), &types(0));
    assert_eq!(service.begin(&key(1)).await.0, 2);
    assert!(consent.asked().is_empty());
}

/// A consent that holds the question open until the test lets it go.
#[derive(Debug)]
struct Gate {
    entered: Arc<tokio::sync::Notify>,
    proceed: Arc<tokio::sync::Notify>,
}

impl Consent for Gate {
    fn ask<'a>(
        &'a self,
        _: &'a str,
        _: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        Box::pin(async move {
            self.entered.notify_one();
            self.proceed.notified().await;
            Ok(ConsentAnswer::Allowed)
        })
    }
}

#[tokio::test]
async fn a_yes_belongs_to_the_session_it_was_asked_about() {
    let gate = Gate {
        entered: Arc::default(),
        proceed: Arc::default(),
    };
    let (entered, proceed) = (gate.entered.clone(), gate.proceed.clone());
    let eis = Arc::new(FakeEis::new());
    let service = Arc::new(RemoteDesktopService::new(
        Box::new(gate),
        Box::new(eis.clone()),
    ));
    service.create(key(1), "org.example.Meet");
    let asking = tokio::spawn({
        let service = service.clone();
        async move { service.begin(&key(1)).await }
    });
    entered.notified().await;
    // While the person is deciding the handle is closed and opened again by someone else.
    service.close(&key(1)).await;
    service.create(key(1), "org.example.Other");
    proceed.notify_one();
    let (response, _) = asking.await.expect("the call ends");
    assert_eq!(response, 1, "the yes was for a session that is gone");
    assert!(
        matches!(
            service.connect(&key(1)).await,
            Err(ConnectError::NotAllowed)
        ),
        "the new session was never allowed"
    );
}

#[tokio::test]
async fn a_session_has_one_socket_and_closing_it_takes_the_socket_back() {
    let (service, _, eis) = service(vec![ConsentAnswer::Allowed]);
    service.create(key(1), "a");
    service.begin(&key(1)).await;
    let fd = service.connect(&key(1)).await.expect("a socket");
    assert!(matches!(
        service.connect(&key(1)).await,
        Err(ConnectError::AlreadyConnected)
    ));
    assert_eq!(eis.asked().len(), 1, "no second socket was asked for");

    let mut client = UnixStream::from(fd);
    assert!(eis.ended().is_empty());
    service.close(&key(1)).await;
    assert_eq!(eis.ended(), vec![1], "the compositor was told to stop");
    let mut byte = [0_u8; 1];
    assert_eq!(
        client.read(&mut byte).expect("end of file"),
        0,
        "the compositor's end is closed, so the app's input stops"
    );
}

#[tokio::test]
async fn a_session_that_never_took_a_socket_has_nothing_to_take_back() {
    let (service, _, eis) = service(vec![ConsentAnswer::Allowed]);
    service.create(key(1), "a");
    service.begin(&key(1)).await;
    service.close(&key(1)).await;
    service.close(&key(2)).await;
    assert!(eis.ended().is_empty());
}

#[tokio::test]
async fn sessions_end_with_the_peer_they_hang_on_and_their_sockets_are_taken_back() {
    let (service, _, eis) = service(vec![ConsentAnswer::Allowed, ConsentAnswer::Allowed]);
    let hung_on = |caller: &str| Tether::new(Some(caller.to_owned()), Some(":1.2".to_owned()));
    service
        .create_for(key(1), "a", hung_on(":1.7"))
        .expect("room");
    service
        .create_for(key(2), "b", hung_on(":1.8"))
        .expect("room");
    for n in [1, 2] {
        service.begin(&key(n)).await;
        service.connect(&key(n)).await.expect("a socket");
    }

    assert_eq!(service.release_peer(":1.7").await, vec![key(1)]);
    assert_eq!(service.len(), 1, "the other caller's session stays");
    assert_eq!(eis.ended(), vec![1]);

    // The frontend leaving ends what is left, whoever called.
    assert_eq!(service.release_peer(":1.2").await, vec![key(2)]);
    assert!(service.is_empty());
    assert_eq!(eis.ended(), vec![1, 2]);
    assert!(service.release_peer(":1.2").await.is_empty(), "once");
}

#[test]
fn one_caller_cannot_fill_the_book_of_sessions() {
    let (service, _, _) = service(vec![]);
    let hung_on = Tether::new(Some(":1.7".to_owned()), None);
    for n in 0..transom::peers::SESSION_LIMITS.per_caller {
        let n = u32::try_from(n).expect("small");
        assert!(service.create_for(key(n), "a", hung_on.clone()).is_ok());
    }
    assert!(service.create_for(key(999), "a", hung_on).is_err());
    assert!(
        service
            .create_for(key(1000), "a", Tether::new(Some(":1.8".to_owned()), None))
            .is_ok()
    );
}
