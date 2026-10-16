//! The portal's questions to the shell, on a private bus: transom's real clients against a
//! stand-in shell that applies the owner check the real shell applies (a question is answered only
//! when its sender is the connection that owns the portal's name). Each side's own tests were green
//! while the pair was broken, because the client asked from a second connection of its own. The
//! stand-in is introduced as the shell, as the caller table would name it (`guard.rs` covers the
//! shell that is not the shell).

use std::time::Duration;

use transom::access::{Answer, Asker, Question, ShellAsker};
use transom::chooser::{Chooser, Shell1Proxy, ShellChooser};
use transom::screencast::restore::FileStore;
use transom::screenshot::{ColorPicker, ShellColorPicker};
use transom::settings::DesktopAppearance;
use transom::{BUS_NAME, Parts, serve_on};
use transom_core::{CastSource, ChoiceRequest, CursorMode, Multiple, SourceKinds};
use zbus::message::Header;
use zbus::names::BusName;
use zbus::{Connection, fdo};

use crate::bus::PrivateBus;

const SETTLE: Duration = Duration::from_secs(5);

/// The stand-in for the shell's side of `org.quire.Shell1`.
struct StandIn;

/// The shell's own rule: only the owner of the portal's name may ask.
async fn from_portal(connection: &Connection, header: &Header<'_>) -> fdo::Result<()> {
    let denied = || fdo::Error::AccessDenied("only the portal backend asks".to_owned());
    let name = BusName::try_from(BUS_NAME).map_err(|e| fdo::Error::Failed(e.to_string()))?;
    let owner = fdo::DBusProxy::new(connection)
        .await?
        .get_name_owner(name)
        .await
        .map_err(|_| denied())?;
    match header.sender() {
        Some(sender) if sender.as_str() == owner.as_str() => Ok(()),
        _ => Err(denied()),
    }
}

#[zbus::interface(name = "org.quire.Shell1")]
impl StandIn {
    async fn pick_capture_source(
        &self,
        _app_label: String,
        _kinds: u32,
        _multiple: u32,
        _cursor_modes: Vec<u32>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<(Vec<(String, String)>, u32)> {
        from_portal(connection, &header).await?;
        Ok((vec![("output".to_owned(), "DP-1".to_owned())], 1))
    }

    async fn pick_color(
        &self,
        _app_label: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<Vec<(f64, f64, f64)>> {
        from_portal(connection, &header).await?;
        Ok(vec![(0.25, 0.5, 0.75)])
    }

    #[allow(clippy::too_many_arguments)] // the shell's own signature
    async fn ask_permission(
        &self,
        _app_label: String,
        _title: String,
        _subtitle: String,
        _body: String,
        _grant_label: String,
        _deny_label: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> fdo::Result<u32> {
        from_portal(connection, &header).await?;
        Ok(0)
    }
}

async fn shell_on(bus: &PrivateBus) -> Connection {
    bus.builder()
        .name("org.quire.Shell")
        .and_then(|b| b.serve_at("/org/quire/Shell1", StandIn))
        .unwrap_or_else(|e| panic!("the stand-in shell: {e}"))
        .build()
        .await
        .unwrap_or_else(|e| panic!("the stand-in shell: {e}"))
}

fn request() -> ChoiceRequest {
    ChoiceRequest {
        kinds: SourceKinds::OUTPUTS,
        multiple: Multiple::One,
        cursor_modes: vec![CursorMode::Hidden],
        app_label: "Meet".to_owned(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_portal_asks_the_shell_from_the_connection_that_owns_its_name() {
    let Some(bus) = PrivateBus::start() else {
        return;
    };
    let shell = shell_on(&bus).await;
    let scratch = tempfile::tempdir().expect("scratch");
    let parts = Parts::real(
        FileStore::new(scratch.path().join("restore")),
        DesktopAppearance::new(),
    );
    let link = parts.link();
    let parts_link = parts.link();
    let chooser = ShellChooser::new(link.clone());
    let asker = ShellAsker::new(link.clone());
    let picker = ShellColorPicker::new(link);
    let _portal = serve_on(bus.builder(), parts)
        .await
        .expect("the portal serves");

    let peers = parts_link
        .peers()
        .expect("served portals can tell callers apart");
    crate::guard::as_shell(&peers, &shell);

    // The control: a connection of its own is a stranger to the shell's rule, so the rule is
    // live. Without this, the answers below would prove nothing.
    let stranger = bus.builder().build().await.expect("a second connection");
    let refused = Shell1Proxy::new(&stranger)
        .await
        .expect("a proxy")
        .pick_capture_source("Meet", 1, 0, vec![1])
        .await;
    assert!(refused.is_err(), "a stranger must be refused: {refused:?}");

    let picked = tokio::time::timeout(SETTLE, chooser.pick(&request()))
        .await
        .expect("answered in time")
        .expect("the shell answered the portal");
    assert_eq!(
        picked.map(|c| c.sources),
        Some(vec![CastSource::Output("DP-1".to_owned())])
    );

    let question = Question::from_call("org.example.Meet", "Allow?", "", "", &Default::default());
    let answered = tokio::time::timeout(SETTLE, asker.ask(&question))
        .await
        .expect("answered in time")
        .expect("the shell answered the portal");
    assert_eq!(answered, Answer::Granted);

    let colour = tokio::time::timeout(SETTLE, picker.pick("Meet"))
        .await
        .expect("answered in time")
        .expect("the shell answered the portal");
    assert!(colour.is_some());
}
