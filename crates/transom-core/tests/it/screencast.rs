use transom_core::{CastIn, CastOut, CastReply, CastSession, CastSource, PwNode, step};

fn out(name: &str) -> CastSource {
    CastSource::Output(name.to_owned())
}

fn win(id: &str) -> CastSource {
    CastSource::Window(id.to_owned())
}

#[test]
fn the_transition_table() {
    use CastIn as I;
    use CastOut as O;
    use CastSession as S;
    let a = out("A");
    let w = win("w1");
    let node = PwNode(42);
    let granted = |sources: Vec<CastSource>| O::Reply(CastReply::Granted { node, sources });

    let table: Vec<(&str, CastSession, CastIn, CastSession, Vec<CastOut>)> = vec![
        (
            "start without a token asks",
            S::Created,
            I::Start { restored: vec![] },
            S::Created,
            vec![O::AskChooser],
        ),
        (
            "start with a token skips the chooser",
            S::Created,
            I::Start {
                restored: vec![a.clone()],
            },
            S::SourcesSelected(vec![a.clone()]),
            vec![O::OpenStream(vec![a.clone()])],
        ),
        (
            "a pick opens the stream",
            S::Created,
            I::Picked(vec![a.clone(), w.clone()]),
            S::SourcesSelected(vec![a.clone(), w.clone()]),
            vec![O::OpenStream(vec![a.clone(), w.clone()])],
        ),
        (
            "an empty pick cancels",
            S::Created,
            I::Picked(vec![]),
            S::Closed,
            vec![O::Reply(CastReply::Cancelled)],
        ),
        (
            "close before start",
            S::Created,
            I::Close,
            S::Closed,
            vec![],
        ),
        (
            "the node arrives",
            S::SourcesSelected(vec![a.clone()]),
            I::NodeReady(node),
            S::Started { node },
            vec![granted(vec![a.clone()])],
        ),
        (
            "the stream fails to open",
            S::SourcesSelected(vec![a.clone()]),
            I::StreamFailed,
            S::Closed,
            vec![O::CloseStream, O::Reply(CastReply::Failed)],
        ),
        (
            "close while opening",
            S::SourcesSelected(vec![a.clone()]),
            I::Close,
            S::Closed,
            vec![O::CloseStream],
        ),
        (
            "close while sharing",
            S::Started { node },
            I::Close,
            S::Closed,
            vec![O::CloseStream],
        ),
        (
            "the source ends while sharing",
            S::Started { node },
            I::StreamEnded,
            S::Closed,
            vec![O::CloseStream],
        ),
        (
            "a second start is ignored",
            S::Started { node },
            I::Start { restored: vec![] },
            S::Started { node },
            vec![],
        ),
        (
            "a late pick is ignored",
            S::Started { node },
            I::Picked(vec![a.clone()]),
            S::Started { node },
            vec![],
        ),
        (
            "closed stays closed",
            S::Closed,
            I::NodeReady(node),
            S::Closed,
            vec![],
        ),
    ];
    for (name, from, input, want_state, want_out) in table {
        let (state, outs) = step(from, input);
        assert_eq!(state, want_state, "{name}: state");
        assert_eq!(outs, want_out, "{name}: outputs");
    }
}

#[test]
fn a_whole_session_from_pick_to_close() {
    let (s, _) = step(CastSession::Created, CastIn::Start { restored: vec![] });
    let (s, _) = step(s, CastIn::Picked(vec![win("w")]));
    let (s, _) = step(s, CastIn::NodeReady(PwNode(7)));
    assert_eq!(s, CastSession::Started { node: PwNode(7) });
    let (s, outs) = step(s, CastIn::Close);
    assert_eq!(s, CastSession::Closed);
    assert_eq!(outs, vec![CastOut::CloseStream]);
}
