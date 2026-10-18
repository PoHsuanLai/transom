//! The PipeWire side: a video source stream apps connect to, fed by a [`FrameSink`].
//!
//! The stream is a `Video/Source` node run by a PipeWire loop on its own thread. Frames are
//! offered as shared memory in one of four 32-bit layouts (the consumer picks; [`pixels`] converts
//! from whatever the capture produced). The pointer is drawn into the picture or left out by the
//! capture according to the session's [`CursorMode`]; a stream does not carry the pointer as
//! separate metadata yet, so that mode shows no pointer. GPU buffers are not negotiated yet; a
//! frame that arrives as one is refused.
//!
//! The node is not linked to anything: the app that asked for the share reads it by node id. The
//! stream only has a buffer to fill when an app reads it. The sink keeps the newest picture: a
//! picture that has not gone out is written to the first free buffer (so an app that links after
//! the first frame still gets it), and a picture that has gone out is written again once the
//! negotiated frame gap has passed (so a still screen still gives a stream). When to write is
//! `transom_core::decide`.

use std::io::Cursor;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ::pipewire as pw;
use pw::properties::properties;
use pw::spa::param::ParamType;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{ChoiceValue, Object, Pod, Property, Value};
use pw::spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Direction, Fraction, Rectangle, SpaTypes};
use pw::stream::{StreamFlags, StreamRc, StreamState};
use transom_core::{CursorMode, FrameRate, Held, PwNode, Step, decide};

use super::pixels::{self, Layout};
use super::stream::{FrameSink, SinkError};
use crate::capture::{CapturedFrame, FrameData, PhysicalSizeWire, PixelFormat};

/// How long `open` waits for PipeWire to give the stream a node.
const READY_WITHIN: Duration = Duration::from_secs(5);

/// The newest picture and when it last went out. A newer picture replaces it.
type Latest = Arc<Mutex<Slot>>;

/// The picture the stream shows, and whether it has been written to a buffer.
#[derive(Debug, Default)]
struct Slot {
    picture: Option<Arc<Pending>>,
    /// When the picture was last written to a buffer; `None` while it has not been.
    sent_at: Option<Instant>,
}

impl Slot {
    /// How the picture stands as of `now`.
    fn held(&self, now: Instant) -> Held {
        match (&self.picture, self.sent_at) {
            (None, _) => Held::Nothing,
            (Some(_), None) => Held::Unsent,
            (Some(_), Some(at)) => Held::Sent {
                ago_ns: u64::try_from(now.saturating_duration_since(at).as_nanos())
                    .unwrap_or(u64::MAX),
            },
        }
    }
}

/// A picture of the screen, as the capture produced it.
#[derive(Debug)]
struct Pending {
    size: PhysicalSizeWire,
    format: PixelFormat,
    stride: u32,
    bytes: crate::capture::PixelBuffer,
}

/// What the sink asks of the loop thread.
enum Command {
    /// Offer the stream again at a new size.
    Resize(PhysicalSizeWire),
    /// Stop the loop.
    Quit,
}

/// A live PipeWire stream.
pub struct PipewireSink {
    node: PwNode,
    cursor: CursorMode,
    latest: Latest,
    commands: pw::channel::Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for PipewireSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipewireSink")
            .field("node", &self.node)
            .field("cursor", &self.cursor)
            .finish_non_exhaustive()
    }
}

impl PipewireSink {
    /// Create the stream at `size` and wait until PipeWire has given it a node id.
    pub fn open(size: PhysicalSizeWire, cursor: CursorMode) -> Result<Self, SinkError> {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (commands, inbox) = pw::channel::channel::<Command>();
        let latest = Latest::default();
        let thread = std::thread::Builder::new()
            .name("transom-pipewire".to_owned())
            .spawn({
                let latest = Arc::clone(&latest);
                move || run(size, &ready_tx, inbox, latest)
            })
            .map_err(|error| SinkError::Pipewire(error.to_string()))?;
        let mut sink = Self {
            node: PwNode(0),
            cursor,
            latest,
            commands,
            thread: Some(thread),
        };
        match ready_rx.recv_timeout(READY_WITHIN) {
            Ok(Ok(node)) => {
                sink.node = PwNode(node);
                Ok(sink)
            }
            Ok(Err(message)) => Err(SinkError::Pipewire(message)),
            Err(_) => Err(SinkError::Pipewire(
                "the stream was not ready in time".to_owned(),
            )),
        }
    }

    /// How the pointer appears in this stream.
    pub fn cursor(&self) -> CursorMode {
        self.cursor
    }
}

impl Drop for PipewireSink {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl FrameSink for PipewireSink {
    fn node(&self) -> PwNode {
        self.node
    }

    fn push(&mut self, frame: CapturedFrame) -> Result<(), SinkError> {
        if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
            return Err(SinkError::NoReader);
        }
        let FrameData::Shm {
            format,
            stride,
            bytes,
        } = frame.data
        else {
            return Err(SinkError::Pipewire(
                "a GPU buffer cannot be offered yet".to_owned(),
            ));
        };
        let pending = Pending {
            size: frame.size,
            format,
            stride,
            bytes,
        };
        let mut slot = self
            .latest
            .lock()
            .map_err(|_| SinkError::Pipewire("the frame slot was poisoned".to_owned()))?;
        *slot = Slot {
            picture: Some(Arc::new(pending)),
            sent_at: None,
        };
        Ok(())
    }

    fn resize(&mut self, size: PhysicalSizeWire) -> Result<(), SinkError> {
        if let Ok(mut slot) = self.latest.lock() {
            *slot = Slot::default();
        }
        self.commands
            .send(Command::Resize(size))
            .map_err(|_| SinkError::NoReader)
    }
}

/// What the stream's callbacks share.
struct Negotiated {
    latest: Latest,
    format: VideoInfoRaw,
    /// The rate the reader settled on, which sets the gap between repeats of a still picture.
    pace: transom_core::Negotiated,
    ready: Option<mpsc::Sender<Result<u32, String>>>,
}

/// The loop thread: build the stream, report its node, run until told to stop.
fn run(
    size: PhysicalSizeWire,
    ready: &mpsc::Sender<Result<u32, String>>,
    inbox: pw::channel::Receiver<Command>,
    latest: Latest,
) {
    if let Err(message) = serve(size, ready, inbox, latest) {
        let _ = ready.send(Err(message));
    }
}

fn serve(
    size: PhysicalSizeWire,
    ready: &mpsc::Sender<Result<u32, String>>,
    inbox: pw::channel::Receiver<Command>,
    latest: Latest,
) -> Result<(), String> {
    let failed = |error: pw::Error| error.to_string();
    pw::init();
    let node_name = share_node_name();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(failed)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(failed)?;
    let core = context.connect_rc(None).map_err(failed)?;
    let stream = StreamRc::new(
        core,
        "transom screen share",
        properties! {
            *pw::keys::MEDIA_CLASS => "Video/Source",
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
            *pw::keys::NODE_NAME => node_name.as_str(),
            *pw::keys::NODE_DESCRIPTION => "Screen share",
        },
    )
    .map_err(failed)?;

    let state = Negotiated {
        latest,
        format: VideoInfoRaw::new(),
        pace: transom_core::Negotiated::Open { ceiling: None },
        ready: Some(ready.clone()),
    };
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(|stream, state, _old, new| match new {
            StreamState::Paused | StreamState::Streaming => {
                if let Some(ready) = state.ready.take() {
                    let _ = ready.send(Ok(stream.node_id()));
                }
            }
            StreamState::Error(message) => {
                if let Some(ready) = state.ready.take() {
                    let _ = ready.send(Err(message));
                }
            }
            _ => {}
        })
        .param_changed(|stream, state, id, param| {
            if id != ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else { return };
            if state.format.parse(param).is_err() {
                return;
            }
            state.pace = transom_core::Negotiated::from_wire(
                frame_rate(state.format.framerate()),
                frame_rate(state.format.max_framerate()),
            );
            let Ok(bytes) = buffers_pod(state.format.size()) else {
                return;
            };
            if let Some(pod) = Pod::from_bytes(&bytes) {
                let _ = stream.update_params(&mut [pod]);
            }
        })
        .process(fill_next_buffer)
        .register()
        .map_err(failed)?;

    let _inbox = inbox.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let stream = stream.clone();
        move |command| match command {
            Command::Quit => mainloop.quit(),
            Command::Resize(size) => {
                if let Ok(bytes) = format_pod(size)
                    && let Some(pod) = Pod::from_bytes(&bytes)
                {
                    let _ = stream.update_params(&mut [pod]);
                }
            }
        }
    });

    let bytes = format_pod(size)?;
    let pod = Pod::from_bytes(&bytes).ok_or_else(|| "the format offer is malformed".to_owned())?;
    stream
        .connect(
            Direction::Output,
            None,
            StreamFlags::MAP_BUFFERS,
            &mut [pod],
        )
        .map_err(failed)?;
    mainloop.run();
    let _ = stream.disconnect();
    Ok(())
}

/// A node name of this share alone. A fixed name would tell any other client of the graph which
/// node carries a screen; a random one is not a guarantee of secrecy (the graph lists nodes) but
/// it stops a share being found by name.
fn share_node_name() -> String {
    let unique = crate::random::hex::<6>().unwrap_or_else(|| std::process::id().to_string());
    format!("transom-share-{unique}")
}

/// The process step: if the picture is due (new, or old enough to repeat) and a buffer is free,
/// write it into the buffer.
///
/// A picture that is not due takes no buffer, so nothing is sent for that cycle. A picture that
/// is due but finds no free buffer stays due and is tried again next cycle. The picture stays
/// held after it is written: a still screen is sent again, a newer picture replaces it.
fn fill_next_buffer(stream: &pw::stream::Stream, state: &mut Negotiated) {
    let now = Instant::now();
    let (picture, step) = {
        let Ok(slot) = state.latest.lock() else {
            return;
        };
        let step = decide(slot.held(now), state.pace.repeat_gap_ns());
        (slot.picture.clone(), step)
    };
    let (Some(frame), Step::SendNew | Step::SendAgain) = (picture, step) else {
        return;
    };
    let size = state.format.size();
    if (frame.size.width, frame.size.height) != (size.width, size.height) {
        return;
    }
    let Some(target) = pixel_format(state.format.format()) else {
        return;
    };
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let stride = size.width as usize * 4;
    let rows = data.data().map_or(0, |out| {
        pixels::convert(
            &frame.bytes,
            Layout {
                format: frame.format,
                stride: frame.stride as usize,
            },
            out,
            Layout {
                format: target,
                stride,
            },
            size.width as usize,
            size.height as usize,
        )
    });
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = i32::try_from(stride).unwrap_or(0);
    *chunk.size_mut() = u32::try_from(stride * rows).unwrap_or(0);
    // Mark it sent only if it is still the picture shown: a newer one arrived meanwhile stays new.
    if let Ok(mut slot) = state.latest.lock()
        && slot
            .picture
            .as_ref()
            .is_some_and(|p| Arc::ptr_eq(p, &frame))
    {
        slot.sent_at = Some(now);
    }
}

/// A SPA fraction as the pure pacing logic reads it.
fn frame_rate(fraction: Fraction) -> FrameRate {
    FrameRate {
        num: fraction.num,
        denom: fraction.denom,
    }
}

/// The capture layout a negotiated SPA format stands for.
fn pixel_format(format: VideoFormat) -> Option<PixelFormat> {
    [
        (VideoFormat::BGRx, PixelFormat::Xrgb8888),
        (VideoFormat::BGRA, PixelFormat::Argb8888),
        (VideoFormat::RGBx, PixelFormat::Xbgr8888),
        (VideoFormat::RGBA, PixelFormat::Abgr8888),
    ]
    .into_iter()
    .find_map(|(spa, ours)| (spa == format).then_some(ours))
}

/// The formats this stream offers: four 32-bit layouts at one size, any frame rate.
fn format_pod(size: PhysicalSizeWire) -> Result<Vec<u8>, String> {
    use pw::spa::pod::property;
    let object = pw::spa::pod::object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        property!(FormatProperties::MediaType, Id, MediaType::Video),
        property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            VideoFormat::BGRx,
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA
        ),
        property!(
            FormatProperties::VideoSize,
            Rectangle,
            Rectangle {
                width: size.width,
                height: size.height
            }
        ),
        property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 0, denom: 1 },
            Fraction { num: 0, denom: 1 },
            Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );
    serialize(&Value::Object(object))
}

/// The buffers this stream wants at a negotiated size: one memory block per buffer, shared
/// memory, two to eight buffers.
fn buffers_pod(size: Rectangle) -> Result<Vec<u8>, String> {
    use pw::spa::sys;
    let stride = i32::try_from(size.width.saturating_mul(4)).map_err(|e| e.to_string())?;
    let bytes = stride
        .checked_mul(i32::try_from(size.height).map_err(|e| e.to_string())?)
        .ok_or_else(|| "the frame is too large".to_owned())?;
    let count = Choice(
        ChoiceFlags::empty(),
        ChoiceEnum::Range {
            default: 4,
            min: 2,
            max: 8,
        },
    );
    let object = Object {
        type_: SpaTypes::ObjectParamBuffers.as_raw(),
        id: ParamType::Buffers.as_raw(),
        properties: vec![
            Property::new(
                sys::SPA_PARAM_BUFFERS_buffers,
                Value::Choice(ChoiceValue::Int(count)),
            ),
            Property::new(sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
            Property::new(sys::SPA_PARAM_BUFFERS_size, Value::Int(bytes)),
            Property::new(sys::SPA_PARAM_BUFFERS_stride, Value::Int(stride)),
            Property::new(
                sys::SPA_PARAM_BUFFERS_dataType,
                Value::Int(1 << pw::spa::buffer::DataType::MemFd.as_raw()),
            ),
        ],
    };
    serialize(&Value::Object(object))
}

fn serialize(value: &Value) -> Result<Vec<u8>, String> {
    PodSerializer::serialize(Cursor::new(Vec::new()), value)
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|error| format!("{error:?}"))
}
