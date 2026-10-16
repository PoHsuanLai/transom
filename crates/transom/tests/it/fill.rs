//! The tests the fill lanes un-ignore. Each is written now; the `#[ignore]` names its lane.

use transom::capture::{CaptureSource, FakeCapture, PhysicalSizeWire};
use transom::screencast::PipewireSink;
use transom::screencast::restore::{FileStore, RestoreStore};
use transom::screencast::stream::FrameSink;
use transom::settings::{AppearanceSource, DesktopAppearance};
use transom_core::PersistMode;
use transom_core::restore::Grant;
use transom_core::{CastSource, CursorMode, RestoreBook, RestoreToken};

#[test]
fn the_desktop_appearance_reads_without_error() {
    let mut source = DesktopAppearance::new();
    assert!(source.current().is_ok());
}

#[test]
fn the_file_store_round_trips_a_book_and_a_missing_file_is_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FileStore::new(dir.path().join("restore"));
    assert!(
        store
            .load()
            .expect("a missing file is an empty book")
            .is_empty()
    );
    let mut book = RestoreBook::new();
    book.remember(
        RestoreToken::mint([7; 16]),
        Grant {
            app_id: "org.example.Meet".into(),
            sources: vec![
                CastSource::Output("A".into()),
                CastSource::Window("a window, with: odd\tcharacters".into()),
            ],
            mode: PersistMode::UntilRevoked,
            granted_at: 1_800_000_000,
        },
    );
    store.save(&book).expect("save");
    assert_eq!(store.load().expect("load"), book);
}

#[test]
#[ignore = "needs a PipeWire daemon; the jailed portal chain (dev/accept/screencast-*.sh) runs it"]
fn the_pipewire_sink_accepts_a_run_of_frames() {
    let size = PhysicalSizeWire {
        width: 64,
        height: 32,
    };
    let mut sink = PipewireSink::open(size, CursorMode::Embedded).expect("open");
    let mut cap = FakeCapture::solid(size, 5);
    while let Ok(frame) = cap.frame() {
        sink.push(frame).expect("push");
    }
}
