use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::Arc;

use peephole_bus::peephole_core::Standing;
use transom::BoxFuture;
use transom::access::{Answer, Asker, FakeAsker, Question};
use transom::app_proof::{AppProof, AppProver, TrustAll, judge_app};
use transom::chooser::ShellError;
use transom::keeper::{Keeper, unix_now};
use transom::names::AppNames;
use transom::peers::Tether;
use transom::remote_desktop::{ConnectError, DeviceKinds, FakeEis, RemoteDesktopService};
use transom::screencast::Dict;
use transom::screencast::options::{restore_data, restore_token};
use transom::screencast::restore::MemoryStore;
use transom::screencast::session::SessionKey;
use transom_core::{Keep, RestoreToken};
use zbus::zvariant::{OwnedValue, Value};

use crate::app_proof::{REMOTE, REMOTE_ID, REMOTE_PROGRAM, names_with_entries, program};

fn key(n: u32) -> SessionKey {
    SessionKey(format!("/org/freedesktop/portal/desktop/session/x/{n}"))
}

fn service(answers: Vec<Answer>) -> (RemoteDesktopService, Arc<FakeAsker>, Arc<FakeEis>) {
    let asker = Arc::new(FakeAsker::new(answers));
    let eis = Arc::new(FakeEis::new());
    let service = RemoteDesktopService::new(Box::new(asker.clone()), Box::new(eis.clone()));
    (service, asker, eis)
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
    let (service, consent, _) = service(vec![Answer::Granted]);
    service.create(key(1), "org.example.Remote");
    service.select(&key(1), &types(1 | 4));
    let (response, results) = service.begin(&key(1)).await;
    assert_eq!(response, 0);
    assert_eq!(devices_of(&results), Some(5));
    let asked = consent.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].app_label, "org.example.Remote");
    assert!(
        asked[0].body.contains(&DeviceKinds::from_wire(5).phrase()),
        "{}",
        asked[0].body
    );
    assert_eq!(
        service.select(&key(1), &types(7)).0,
        2,
        "devices cannot change after the person has allowed"
    );
}

#[tokio::test]
async fn no_choice_means_every_kind_and_bits_we_do_not_offer_are_dropped() {
    let (service, _, _) = service(vec![Answer::Granted, Answer::Granted]);
    service.create(key(1), "a");
    assert_eq!(devices_of(&service.begin(&key(1)).await.1), Some(7));
    service.create(key(2), "b");
    service.select(&key(2), &types(0xff));
    assert_eq!(devices_of(&service.begin(&key(2)).await.1), Some(7));
}

#[tokio::test]
async fn declining_ends_the_session_and_an_empty_script_declines() {
    let (service, _, eis) = service(vec![Answer::Denied]);
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
    let (service, consent, _) = service(vec![Answer::Granted]);
    service.create(key(1), "");
    service.begin(&key(1)).await;
    assert_eq!(consent.asked()[0].app_label, "An app");
}

#[tokio::test]
async fn a_socket_is_handed_out_only_after_the_person_allowed_it() {
    let (service, _, eis) = service(vec![Answer::Granted]);
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
    let asked = eis.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1, DeviceKinds::from_wire(3));

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
    let (service, _, eis) = service(vec![Answer::Granted, Answer::Granted]);
    for n in [1, 2] {
        service.create(key(n), "a");
        service.begin(&key(n)).await;
        service.connect(&key(n)).await.expect("a socket");
    }
    // A run counts up from where it starts (NEW-PI-8), one number per session.
    let numbers: Vec<u64> = eis.asked().iter().map(|(n, _)| *n).collect();
    assert_eq!(numbers.len(), 2);
    assert_eq!(numbers[1], numbers[0] + 1, "{numbers:?}");
}

#[tokio::test]
async fn the_question_names_the_input_and_only_the_yes_button_is_a_yes() {
    let (service, asker, _) = service(vec![Answer::Granted, Answer::Denied, Answer::Dismissed]);
    let mut responses = Vec::new();
    for n in 1..=3 {
        service.create(key(n), "Remote");
        service.select(&key(n), &types(3));
        responses.push(service.begin(&key(n)).await.0);
    }
    assert_eq!(
        responses,
        [0, 1, 1],
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
    fn ask<'a>(&'a self, _: &'a Question) -> BoxFuture<'a, Result<Answer, ShellError>> {
        Box::pin(async { Err(ShellError::Unavailable) })
    }
}

#[tokio::test]
async fn with_no_shell_to_ask_nothing_is_granted_and_the_session_can_be_asked_again() {
    let eis = Arc::new(FakeEis::new());
    let service = RemoteDesktopService::new(Box::new(Absent), Box::new(eis.clone()));
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
    let (service, consent, _) = service(vec![Answer::Granted]);
    service.create(key(1), "a");
    service.select(&key(1), &types(0));
    assert_eq!(service.begin(&key(1)).await.0, 2);
    assert!(consent.asked().is_empty());
}

/// An asker that holds the question open until the test lets it go.
#[derive(Debug)]
struct Gate {
    entered: Arc<tokio::sync::Notify>,
    proceed: Arc<tokio::sync::Notify>,
}

impl Asker for Gate {
    fn ask<'a>(&'a self, _: &'a Question) -> BoxFuture<'a, Result<Answer, ShellError>> {
        Box::pin(async move {
            self.entered.notify_one();
            self.proceed.notified().await;
            Ok(Answer::Granted)
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
    let (service, _, eis) = service(vec![Answer::Granted]);
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
    let handed = eis.asked()[0].0;
    assert_eq!(eis.ended(), vec![handed], "the compositor was told to stop");
    let mut byte = [0_u8; 1];
    assert_eq!(
        client.read(&mut byte).expect("end of file"),
        0,
        "the compositor's end is closed, so the app's input stops"
    );
}

#[tokio::test]
async fn a_session_that_never_took_a_socket_has_nothing_to_take_back() {
    let (service, _, eis) = service(vec![Answer::Granted]);
    service.create(key(1), "a");
    service.begin(&key(1)).await;
    service.close(&key(1)).await;
    service.close(&key(2)).await;
    assert!(eis.ended().is_empty());
}

#[tokio::test]
async fn sessions_end_with_the_peer_they_hang_on_and_their_sockets_are_taken_back() {
    let (service, _, eis) = service(vec![Answer::Granted, Answer::Granted]);
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

    let handed: Vec<u64> = eis.asked().iter().map(|(n, _)| *n).collect();
    assert_eq!(service.release_peer(":1.7").await, vec![key(1)]);
    assert_eq!(service.len(), 1, "the other caller's session stays");
    assert_eq!(eis.ended(), handed[..1]);

    // The frontend leaving ends what is left, whoever called.
    assert_eq!(service.release_peer(":1.2").await, vec![key(2)]);
    assert!(service.is_empty());
    assert_eq!(eis.ended(), handed);
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

/// NEW-PI-8: the compositor outlives the portal, so a restarted portal must not reuse the numbers
/// of the run before (an end called for number 1 would hit another run's session), and it asks the
/// compositor to drop what the run before handed out.
#[tokio::test]
async fn a_restarted_portal_counts_above_the_last_run_and_drops_its_leftovers() {
    let mut numbers = Vec::new();
    for _run in 0..2 {
        let (service, _, eis) = service(vec![Answer::Granted]);
        service.forget_previous_run().await;
        assert_eq!(eis.ended_all(), 1, "the compositor is told at start");
        service.create(key(1), "a");
        service.begin(&key(1)).await;
        service.connect(&key(1)).await.expect("a socket");
        numbers.push(eis.asked()[0].0);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(
        numbers[0] > 1_000_000 && numbers[1] > numbers[0],
        "the second run starts above the first: {numbers:?}"
    );
}

/// Tells apps apart by the program the standing names, the way the real prover does.
#[derive(Debug)]
struct Told {
    standing: Standing,
    names: AppNames,
}

impl AppProver for Told {
    fn prove<'a>(&'a self, _session: &'a str, app_id: &'a str) -> BoxFuture<'a, AppProof> {
        Box::pin(async move { judge_app(&self.standing, app_id, &self.names, &TrustAll) })
    }
}

/// A service that remembers in `keeper`, whose caller runs `program_path` and whose only desktop
/// entry is the fake remote app's.
fn remembering(
    program_path: &str,
    answers: Vec<Answer>,
    keeper: &Arc<Keeper>,
) -> (RemoteDesktopService, Arc<FakeAsker>, tempfile::TempDir) {
    let (dir, names) = names_with_entries(&[(REMOTE_ID, REMOTE)]);
    let asker = Arc::new(FakeAsker::new(answers));
    let told = Told {
        standing: program(program_path),
        names: names.clone(),
    };
    let service =
        RemoteDesktopService::new(Box::new(asker.clone()), Box::new(Arc::new(FakeEis::new())))
            .with_names(names)
            .with_memory(Arc::clone(keeper), Arc::new(told));
    (service, asker, dir)
}

fn new_keeper() -> Arc<Keeper> {
    Arc::new(Keeper::new(Box::new(MemoryStore::default())))
}

/// The options of a `SelectDevices` that asks for `bits` and to be remembered until taken back,
/// offering `token` if there is one.
fn remember(bits: u32, token: Option<&RestoreToken>) -> Dict {
    let mut options = types(bits);
    options.insert("persist_mode".to_owned(), OwnedValue::from(2_u32));
    if let Some(token) = token {
        options.insert(
            "restore_data".to_owned(),
            restore_data(token).expect("restore data"),
        );
    }
    options
}

/// Opens a session for an app that gives no id, selects, and begins.
async fn begin_with(service: &RemoteDesktopService, n: u32, options: &Dict) -> (u32, Dict) {
    service.create(key(n), "");
    service.select(&key(n), options);
    service.begin(&key(n)).await
}

#[tokio::test]
async fn a_yes_to_remote_control_is_remembered_and_restored_for_the_same_devices_or_fewer() {
    let keeper = new_keeper();
    let answers = vec![Answer::Granted; 3];
    let (service, asker, _dir) = remembering(REMOTE_PROGRAM, answers, &keeper);

    // Asked once. The app gave no id; it is shown by its entry's name, remembered by its id.
    let (response, results) = begin_with(&service, 1, &remember(3, None)).await;
    assert_eq!(response, 0);
    assert_eq!(asker.asked().len(), 1);
    assert_eq!(asker.asked()[0].app_label, "Fake Remote");
    let kept = results
        .get("persist_mode")
        .and_then(|v| v.downcast_ref::<u32>().ok());
    assert_eq!(kept, Some(2), "until taken back");
    let first = restore_token(&results).expect("a token for next time");
    let listed = keeper.remembered().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].app_id.as_str(), listed[0].keep),
        (REMOTE_ID, Keep::UntilRevoked)
    );

    // Fewer devices: not asked, the token is replaced, and the yes still covers both devices.
    let (response, results) = begin_with(&service, 2, &remember(1, Some(&first))).await;
    assert_eq!((response, devices_of(&results)), (0, Some(1)));
    assert_eq!(asker.asked().len(), 1, "not asked again");
    let second = restore_token(&results).expect("a fresh token");
    assert_ne!(second, first);
    let gone =
        keeper
            .book()
            .await
            .resolve_input(&first, REMOTE_ID, DeviceKinds::from_wire(1), unix_now());
    assert!(gone.is_empty(), "the token it replaced is forgotten");
    let (response, results) = begin_with(&service, 3, &remember(3, Some(&second))).await;
    assert_eq!((response, devices_of(&results)), (0, Some(3)));
    assert_eq!(
        asker.asked().len(),
        1,
        "asking for fewer did not shrink the yes"
    );
    let third = restore_token(&results).expect("a fresh token");

    // More devices than were allowed: asked.
    let (response, results) = begin_with(&service, 4, &remember(7, Some(&third))).await;
    assert_eq!((response, devices_of(&results)), (0, Some(7)));
    assert_eq!(asker.asked().len(), 2);
    let fourth = restore_token(&results).expect("a fresh token");

    // Taken back in one go, whatever it covered: asked again.
    assert_eq!(keeper.revoke(REMOTE_ID).await, Some(1));
    begin_with(&service, 5, &remember(7, Some(&fourth))).await;
    assert_eq!(asker.asked().len(), 3);
}

#[tokio::test]
async fn an_app_that_is_not_shown_to_be_what_it_says_is_asked_every_time() {
    let keeper = new_keeper();
    let (proven, _, _entries) = remembering(REMOTE_PROGRAM, vec![Answer::Granted], &keeper);
    let (_, results) = begin_with(&proven, 1, &remember(3, None)).await;
    let token = restore_token(&results).expect("a token");

    // Another program says the remote app's id and offers its token.
    let (stranger, asker, _other) = remembering("/usr/bin/python3", vec![Answer::Granted], &keeper);
    stranger.create(key(2), REMOTE_ID);
    stranger.select(&key(2), &remember(3, Some(&token)));
    let (response, results) = stranger.begin(&key(2)).await;
    assert_eq!(response, 0);
    assert_eq!(asker.asked().len(), 1, "the token is no use to it");
    assert!(
        !results.contains_key("restore_data") && !results.contains_key("persist_mode"),
        "and nothing is remembered for it"
    );
    assert_eq!(
        keeper.remembered().await.len(),
        1,
        "only the proven app is listed"
    );
}
