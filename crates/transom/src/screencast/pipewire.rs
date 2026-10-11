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
//! stream only has a buffer to fill when an app reads it, and a frame is written only when one is
//! waiting, so a picture that does not change sends nothing.

use std::io::Cursor;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use ::pipewire as pw;
use pw::properties::properties;
use pw::spa::param::ParamType;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{ChoiceValue, Object, Pod, Property, Value};
use pw::spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Direction, Fraction, Rectangle, SpaTypes};
use pw::stream::{StreamFlags, StreamRc, StreamState};
use transom_core::{CursorMode, PwNode};

use super::pixels::{self, Layout};
use super::stream::{FrameSink, SinkError};
use crate::capture::{CapturedFrame, FrameData, PhysicalSizeWire, PixelFormat};

/// How long `open` waits for PipeWire to give the stream a node.
const READY_WITHIN: Duration = Duration::from_secs(5);

/// The newest frame that has not been sent yet. A newer one replaces it.
type Waiting = Arc<Mutex<Option<Pending>>>;

/// A frame waiting for the next free buffer.
#[derive(Debug)]
struct Pending {
    size: PhysicalSizeWire,
    format: PixelFormat,
    stride: u32,
    bytes: Vec<u8>,
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
    waiting: Waiting,
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
        let waiting = Waiting::default();
        let thread = std::thread::Builder::new()
            .name("transom-pipewire".to_owned())
            .spawn({
                let waiting = Arc::clone(&waiting);
                move || run(size, &ready_tx, inbox, waiting)
            })
            .map_err(|error| SinkError::Pipewire(error.to_string()))?;
        let mut sink = Self {
            node: PwNode(0),
            cursor,
            waiting,
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
            .waiting
            .lock()
            .map_err(|_| SinkError::Pipewire("the frame slot was poisoned".to_owned()))?;
        *slot = Some(pending);
        Ok(())
    }

    fn resize(&mut self, size: PhysicalSizeWire) -> Result<(), SinkError> {
        if let Ok(mut slot) = self.waiting.lock() {
            *slot = None;
        }
        self.commands
            .send(Command::Resize(size))
            .map_err(|_| SinkError::NoReader)
    }
}

/// What the stream's callbacks share.
struct Negotiated {
    waiting: Waiting,
    format: VideoInfoRaw,
    ready: Option<mpsc::Sender<Result<u32, String>>>,
}

/// The loop thread: build the stream, report its node, run until told to stop.
fn run(
    size: PhysicalSizeWire,
    ready: &mpsc::Sender<Result<u32, String>>,
    inbox: pw::channel::Receiver<Command>,
    waiting: Waiting,
) {
    if let Err(message) = serve(size, ready, inbox, waiting) {
        let _ = ready.send(Err(message));
    }
}

fn serve(
    size: PhysicalSizeWire,
    ready: &mpsc::Sender<Result<u32, String>>,
    inbox: pw::channel::Receiver<Command>,
    waiting: Waiting,
) -> Result<(), String> {
    let failed = |error: pw::Error| error.to_string();
    pw::init();
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
            *pw::keys::NODE_NAME => "transom-screen-share",
            *pw::keys::NODE_DESCRIPTION => "Screen share",
        },
    )
    .map_err(failed)?;

    let state = Negotiated {
        waiting,
        format: VideoInfoRaw::new(),
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

/// The process step: if a frame is waiting and a buffer is free, write the frame into it.
///
/// With nothing waiting no buffer is taken, so nothing is sent for that cycle. A frame that
/// arrives while no buffer is free is dropped, as the sink's contract says.
fn fill_next_buffer(stream: &pw::stream::Stream, state: &mut Negotiated) {
    let Some(frame) = state.waiting.lock().ok().and_then(|mut slot| slot.take()) else {
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
    serialize(Value::Object(object))
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
    serialize(Value::Object(object))
}

fn serialize(value: Value) -> Result<Vec<u8>, String> {
    PodSerializer::serialize(Cursor::new(Vec::new()), &value)
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|error| format!("{error:?}"))
}
