//! The Wayland side shared by output and window capture: the connection, the copy session, and
//! the shared-memory buffer a frame is copied into.
//!
//! Frames are shared memory only. The buffer is a file the compositor maps and this side reads
//! back with plain reads, so no mapping (and no `unsafe`) is needed here. GPU buffers are not
//! asked for: the constraint events that offer them are ignored.

mod outputs;
mod wait;

use std::collections::BTreeMap;
use std::fs::File;
use std::os::fd::AsFd;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use transom_core::{CastSource, CursorMode};
use wayland_client::backend::WaylandError;
use wayland_client::globals::{BindError, GlobalList, GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_buffer, wl_callback, wl_output, wl_registry, wl_shm, wl_shm_pool,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
    ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_foreign_toplevel_image_capture_source_manager_v1::ExtForeignToplevelImageCaptureSourceManagerV1,
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

use self::outputs::SeenOutput;
use self::wait::{Readiness, Waited, Watchdog, readable_within};
use super::{
    Canceller, CaptureError, CapturedFrame, DisplayName, FrameData, PhysicalSizeWire, PixelFormat,
};

/// How long the compositor has to accept the connection and list its globals. It does both at
/// once; a longer wait means it is stalled (a GPU reset, a frozen session).
const CONNECT_DEADLINE: Duration = Duration::from_secs(5);

/// How long it has to answer one round trip.
const ROUNDTRIP_DEADLINE: Duration = Duration::from_secs(5);

/// How long it has to describe the buffers of a new copy session.
const CONSTRAINTS_DEADLINE: Duration = Duration::from_secs(10);

/// How long a one-picture request waits for its picture. A picture that never comes (an output
/// that never copies) is an error, not an hour of a blocked thread.
const FIRST_FRAME_DEADLINE: Duration = Duration::from_secs(10);

/// The longest edge of a captured picture, in pixels. Larger is a window or output resizing
/// itself to exhaust memory, or a bug.
const MAX_EDGE: u32 = 16_384;

/// The most bytes of one captured picture (8192 by 8192 pixels of 4 bytes).
const MAX_FRAME_BYTES: u64 = 256 * 1024 * 1024;

/// How many times a frame that fails for a reason that may pass is asked for again.
const FRAME_ATTEMPTS: u32 = 3;

/// The id of a bound `wl_output`, so its name event can be filed.
#[derive(Clone, Copy, Debug)]
pub(super) struct OutputId(u32);

/// A toplevel the compositor listed.
#[derive(Debug)]
struct SeenToplevel {
    handle: ExtForeignToplevelHandleV1,
    identifier: Option<String>,
    closed: bool,
}

/// What a copy session says a buffer must look like.
#[derive(Clone, Debug, Default)]
struct Constraints {
    size: Option<(u32, u32)>,
    formats: Vec<PixelFormat>,
}

/// Why a frame did not arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Ready,
    /// The buffer no longer fits; new constraints follow.
    Constraints,
    /// The compositor stopped the session.
    Stopped,
    /// For no stated reason.
    Unknown,
}

#[derive(Debug, Default)]
struct FrameState {
    presented_ns: u64,
    outcome: Option<Outcome>,
}

/// Everything the event handlers write to.
#[derive(Debug, Default)]
pub(super) struct Wl {
    outputs: BTreeMap<u32, SeenOutput>,
    toplevels: Vec<SeenToplevel>,
    pending: Constraints,
    current: Option<Constraints>,
    /// Bumped each time the session finishes sending constraints.
    generation: u32,
    stopped: bool,
    frame: FrameState,
}

/// A connection to the compositor with its event queue.
pub(super) struct Link {
    conn: Connection,
    globals: GlobalList,
    queue: EventQueue<Wl>,
    qh: QueueHandle<Wl>,
    wl: Wl,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").finish_non_exhaustive()
    }
}

fn broken(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Connection(error.to_string())
}

fn missing(_: BindError) -> CaptureError {
    CaptureError::Unsupported
}

impl Link {
    /// Connect to the display: the session's own when `display` is empty, else the socket at the
    /// absolute path it holds.
    pub fn connect(display: &DisplayName) -> Result<Self, CaptureError> {
        let conn = if display.0.is_empty() {
            Connection::connect_to_env().map_err(broken)?
        } else if Path::new(&display.0).is_absolute() {
            let stream = UnixStream::connect(&display.0).map_err(broken)?;
            Connection::from_socket(stream).map_err(broken)?
        } else {
            return Err(CaptureError::Connection(
                "a display name must be a socket path".to_owned(),
            ));
        };
        // Listing the globals is a round trip with no deadline of its own: a watchdog cuts the
        // connection if it takes too long, so a stalled compositor fails the call.
        let watchdog = Watchdog::arm(&conn, CONNECT_DEADLINE);
        let listed = registry_queue_init::<Wl>(&conn);
        drop(watchdog);
        let (globals, queue) = listed.map_err(broken)?;
        let qh = queue.handle();
        Ok(Self {
            conn,
            globals,
            queue,
            qh,
            wl: Wl::default(),
        })
    }

    /// Wait until the compositor has handled everything sent so far, for at most
    /// [`ROUNDTRIP_DEADLINE`].
    pub fn roundtrip(&mut self) -> Result<(), CaptureError> {
        let answered = Arc::new(OnceLock::<()>::new());
        let _callback = self.conn.display().sync(&self.qh, Arc::clone(&answered));
        let seen = Arc::clone(&answered);
        match self.wait_within(move |_| seen.get().is_some(), ROUNDTRIP_DEADLINE)? {
            Waited::Done => Ok(()),
            Waited::TimedOut => Err(CaptureError::TimedOut),
        }
    }

    /// Dispatch events until `done` holds, but give up once `wait` has passed. Events that arrive
    /// are dispatched either way, so a later call carries on from where this one stopped.
    fn wait_within(
        &mut self,
        done: impl Fn(&Wl) -> bool,
        wait: Duration,
    ) -> Result<Waited, CaptureError> {
        let deadline = Instant::now() + wait;
        loop {
            self.queue.dispatch_pending(&mut self.wl).map_err(broken)?;
            self.conn.flush().map_err(broken)?;
            if done(&self.wl) {
                return Ok(Waited::Done);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(Waited::TimedOut);
            }
            // Events already queued for us mean there is nothing to read yet: dispatch them.
            let Some(guard) = self.queue.prepare_read() else {
                continue;
            };
            match readable_within(guard.connection_fd(), left)? {
                Readiness::Silent => drop(guard),
                Readiness::Ready => match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(error))
                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(broken(error)),
                },
            }
        }
    }

    /// The output called `name`, once the compositor has told us the names.
    pub fn find_output(&mut self, name: &str) -> Result<wl_output::WlOutput, CaptureError> {
        let advertised: Vec<u32> = self.globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == "wl_output")
                .map(|g| g.name)
                .collect()
        });
        let mut bound = Vec::new();
        for global in advertised {
            let output = self
                .globals
                .bind::<wl_output::WlOutput, _, _>(&self.qh, 1..=4, OutputId(global))
                .map_err(missing)?;
            bound.push((global, output));
        }
        self.roundtrip()?;
        let found = bound.into_iter().find(|(global, _)| {
            self.wl
                .outputs
                .get(global)
                .is_some_and(|o| o.name.as_deref() == Some(name))
        });
        found.map(|(_, output)| output).ok_or(CaptureError::Ended)
    }

    /// Every output and toplevel the compositor lists now.
    pub fn live_sources(mut self) -> Result<Vec<CastSource>, CaptureError> {
        let advertised: Vec<u32> = self.globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == "wl_output")
                .map(|g| g.name)
                .collect()
        });
        let outputs: Vec<_> = advertised
            .iter()
            .filter_map(|&global| {
                self.globals
                    .bind::<wl_output::WlOutput, _, _>(&self.qh, 1..=4, OutputId(global))
                    .ok()
            })
            .collect();
        // Without the list global there are no windows to offer, only outputs.
        let list = self
            .globals
            .bind::<ExtForeignToplevelListV1, _, _>(&self.qh, 1..=1, ())
            .ok();
        self.roundtrip()?;
        self.roundtrip()?;
        let mut sources: Vec<CastSource> = self
            .wl
            .outputs
            .values()
            .filter_map(|o| o.name.clone())
            .map(CastSource::Output)
            .collect();
        sources.extend(
            self.wl
                .toplevels
                .iter()
                .filter(|t| !t.closed)
                .filter_map(|t| t.identifier.clone())
                .map(CastSource::Window),
        );
        if let Some(list) = list {
            list.stop();
        }
        drop(outputs);
        Ok(sources)
    }

    /// The source of the output called `name`.
    pub fn output_source(&mut self, name: &str) -> Result<ExtImageCaptureSourceV1, CaptureError> {
        let manager = self
            .globals
            .bind::<ExtOutputImageCaptureSourceManagerV1, _, _>(&self.qh, 1..=1, ())
            .map_err(missing)?;
        let output = self.find_output(name)?;
        let source = manager.create_source(&output, &self.qh, ());
        manager.destroy();
        Ok(source)
    }

    /// The source of the toplevel with this identifier. A window that is gone is `Ended`.
    pub fn toplevel_source(
        &mut self,
        identifier: &str,
    ) -> Result<ExtImageCaptureSourceV1, CaptureError> {
        let manager = self
            .globals
            .bind::<ExtForeignToplevelImageCaptureSourceManagerV1, _, _>(&self.qh, 1..=1, ())
            .map_err(missing)?;
        let list = self
            .globals
            .bind::<ExtForeignToplevelListV1, _, _>(&self.qh, 1..=1, ())
            .map_err(missing)?;
        self.roundtrip()?;
        self.roundtrip()?;
        let handle = self
            .wl
            .toplevels
            .iter()
            .find(|t| !t.closed && t.identifier.as_deref() == Some(identifier))
            .map(|t| t.handle.clone())
            .ok_or(CaptureError::Ended)?;
        let source = manager.create_source(&handle, &self.qh, ());
        manager.destroy();
        // Stop hearing about toplevels; the handle in use stays valid until it is destroyed.
        list.stop();
        Ok(source)
    }
}

/// The buffer a frame is copied into.
#[derive(Debug)]
struct ShmBuffer {
    buffer: wl_buffer::WlBuffer,
    file: File,
    width: u32,
    height: u32,
    stride: u32,
    len: usize,
    format: PixelFormat,
}

impl Drop for ShmBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
    }
}

/// A running copy session.
#[derive(Debug)]
pub(super) struct Copier {
    link: Link,
    session: ExtImageCopyCaptureSessionV1,
    shm: wl_shm::WlShm,
    buffer: Option<ShmBuffer>,
    generation: u32,
    size: PhysicalSizeWire,
    /// The frame asked for and not yet answered. It stays open while the picture is still.
    inflight: Option<ExtImageCopyCaptureFrameV1>,
    /// How many answers in a row said to try again.
    retries: u32,
}

impl Copier {
    /// Start copying `source` (consumed) and wait for the compositor's buffer constraints.
    pub fn start(
        link: Link,
        source: ExtImageCaptureSourceV1,
        cursor: CursorMode,
    ) -> Result<Self, CaptureError> {
        let manager = link
            .globals
            .bind::<ExtImageCopyCaptureManagerV1, _, _>(&link.qh, 1..=1, ())
            .map_err(missing)?;
        let shm = link
            .globals
            .bind::<wl_shm::WlShm, _, _>(&link.qh, 1..=1, ())
            .map_err(missing)?;
        let options = match cursor {
            CursorMode::Embedded => Options::PaintCursors,
            _ => Options::empty(),
        };
        let session = manager.create_session(&source, options, &link.qh, ());
        source.destroy();
        manager.destroy();
        let mut copier = Self {
            link,
            session,
            shm,
            buffer: None,
            generation: 0,
            size: PhysicalSizeWire {
                width: 0,
                height: 0,
            },
            inflight: None,
            retries: 0,
        };
        let described = copier
            .link
            .wait_within(|wl| wl.generation > 0 || wl.stopped, CONSTRAINTS_DEADLINE)?;
        if described == Waited::TimedOut {
            return Err(CaptureError::TimedOut);
        }
        copier.adopt_constraints()?;
        Ok(copier)
    }

    /// A handle that closes the connection from another thread, to unblock [`Copier::frame`].
    pub fn canceller(&self) -> Option<Canceller> {
        let backend = self.link.conn.backend();
        let fd = backend.poll_fd().try_clone_to_owned().ok()?;
        Some(Canceller::new(fd))
    }

    /// The size the next frame will have.
    pub fn size(&self) -> PhysicalSizeWire {
        self.size
    }

    /// Take up the constraints the session last finished sending, if they are new.
    fn adopt_constraints(&mut self) -> Result<(), CaptureError> {
        if self.link.wl.stopped {
            return Err(CaptureError::Ended);
        }
        if self.generation == self.link.wl.generation {
            return Ok(());
        }
        self.generation = self.link.wl.generation;
        let current = self.link.wl.current.clone().unwrap_or_default();
        let (width, height) = current.size.ok_or(CaptureError::Unsupported)?;
        let format = choose_format(&current.formats).ok_or(CaptureError::Unsupported)?;
        self.size = PhysicalSizeWire { width, height };
        self.buffer = Some(self.make_buffer(width, height, format)?);
        Ok(())
    }

    fn make_buffer(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
    ) -> Result<ShmBuffer, CaptureError> {
        if width == 0 || height == 0 {
            return Err(CaptureError::Unsupported);
        }
        if width > MAX_EDGE || height > MAX_EDGE {
            return Err(CaptureError::TooLarge);
        }
        let stride = width.checked_mul(4).ok_or(CaptureError::TooLarge)?;
        let bytes = u64::from(stride) * u64::from(height);
        if bytes > MAX_FRAME_BYTES {
            return Err(CaptureError::TooLarge);
        }
        let len = usize::try_from(bytes).map_err(|_| CaptureError::TooLarge)?;
        let file = scratch_file(len)?;
        let to_i32 = |v: u32| i32::try_from(v).map_err(|_| CaptureError::Unsupported);
        let pool = self.shm.create_pool(
            file.as_fd(),
            i32::try_from(len).map_err(|_| CaptureError::Unsupported)?,
            &self.link.qh,
            (),
        );
        let buffer = pool.create_buffer(
            0,
            to_i32(width)?,
            to_i32(height)?,
            to_i32(stride)?,
            shm_format(format),
            &self.link.qh,
            (),
        );
        pool.destroy();
        Ok(ShmBuffer {
            buffer,
            file,
            width,
            height,
            stride,
            len,
            format,
        })
    }

    /// Ask for the next frame and wait for it, for at most [`FIRST_FRAME_DEADLINE`]. A picture
    /// that has not changed sends none, but the first request is answered with the whole picture,
    /// so a deadline passing means the output is not copying at all.
    pub fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        self.frame_within(FIRST_FRAME_DEADLINE)?
            .ok_or(CaptureError::TimedOut)
    }

    /// As [`Copier::frame`], but `None` once `wait` has passed with the picture unchanged. The
    /// request stays open for the next call, so a change is never missed between calls.
    pub fn frame_within(&mut self, wait: Duration) -> Result<Option<CapturedFrame>, CaptureError> {
        let deadline = Instant::now() + wait;
        while self.retries < FRAME_ATTEMPTS {
            let frame = match self.inflight.take() {
                Some(frame) => frame,
                None => self.request()?,
            };
            let left = deadline.saturating_duration_since(Instant::now());
            let answered = self
                .link
                .wait_within(|wl| wl.frame.outcome.is_some() || wl.stopped, left)?;
            if answered == Waited::TimedOut {
                self.inflight = Some(frame);
                return Ok(None);
            }
            frame.destroy();
            match self.link.wl.frame.outcome.take() {
                None => return Err(CaptureError::Ended),
                Some(Outcome::Ready) => {
                    self.retries = 0;
                    return self.read_frame().map(Some);
                }
                Some(Outcome::Stopped) => return Err(CaptureError::Stopped),
                // New constraints arrive with or before this; the next request adopts them.
                Some(Outcome::Constraints | Outcome::Unknown) => self.retries += 1,
            }
        }
        self.retries = 0;
        Err(CaptureError::Stopped)
    }

    /// Ask the compositor for a frame into the current buffer.
    fn request(&mut self) -> Result<ExtImageCopyCaptureFrameV1, CaptureError> {
        self.adopt_constraints()?;
        let Some(shm) = self.buffer.as_ref() else {
            return Err(CaptureError::Unsupported);
        };
        let frame = self.session.create_frame(&self.link.qh, ());
        self.link.wl.frame = FrameState::default();
        frame.attach_buffer(&shm.buffer);
        frame.damage_buffer(
            0,
            0,
            i32::try_from(shm.width).unwrap_or(i32::MAX),
            i32::try_from(shm.height).unwrap_or(i32::MAX),
        );
        frame.capture();
        Ok(frame)
    }

    fn read_frame(&self) -> Result<CapturedFrame, CaptureError> {
        let Some(shm) = self.buffer.as_ref() else {
            return Err(CaptureError::Unsupported);
        };
        let mut bytes = vec![0; shm.len];
        shm.file.read_exact_at(&mut bytes, 0).map_err(broken)?;
        Ok(CapturedFrame {
            size: PhysicalSizeWire {
                width: shm.width,
                height: shm.height,
            },
            data: FrameData::Shm {
                format: shm.format,
                stride: shm.stride,
                bytes,
            },
            presented_ns: self.link.wl.frame.presented_ns,
        })
    }
}

impl Drop for Copier {
    fn drop(&mut self) {
        if let Some(frame) = self.inflight.take() {
            frame.destroy();
        }
        self.session.destroy();
        let _ = self.link.conn.flush();
    }
}

/// The layout to ask for among those the compositor offers: opaque ones first, since a picture
/// of a screen has no use for alpha.
fn choose_format(offered: &[PixelFormat]) -> Option<PixelFormat> {
    [
        PixelFormat::Xrgb8888,
        PixelFormat::Argb8888,
        PixelFormat::Xbgr8888,
        PixelFormat::Abgr8888,
    ]
    .into_iter()
    .find(|f| offered.contains(f))
}

fn shm_format(format: PixelFormat) -> wl_shm::Format {
    match format {
        PixelFormat::Argb8888 => wl_shm::Format::Argb8888,
        PixelFormat::Xrgb8888 => wl_shm::Format::Xrgb8888,
        PixelFormat::Abgr8888 => wl_shm::Format::Abgr8888,
        PixelFormat::Xbgr8888 => wl_shm::Format::Xbgr8888,
    }
}

fn pixel_format(format: wl_shm::Format) -> Option<PixelFormat> {
    match format {
        wl_shm::Format::Argb8888 => Some(PixelFormat::Argb8888),
        wl_shm::Format::Xrgb8888 => Some(PixelFormat::Xrgb8888),
        wl_shm::Format::Abgr8888 => Some(PixelFormat::Abgr8888),
        wl_shm::Format::Xbgr8888 => Some(PixelFormat::Xbgr8888),
        _ => None,
    }
}

/// An anonymous file of `len` bytes in memory: no name in any directory, so there is nothing for
/// another process to find, squat on or open, and nothing to clean up.
fn scratch_file(len: usize) -> Result<File, CaptureError> {
    let fd = rustix::fs::memfd_create("transom-frame", rustix::fs::MemfdFlags::CLOEXEC)
        .map_err(broken)?;
    let file = File::from(fd);
    file.set_len(len as u64).map_err(broken)?;
    Ok(file)
}

/// Events nobody here needs: an empty handler for each listed interface and user data.
macro_rules! ignore_events {
    ($($iface:ty, $data:ty);* $(;)?) => {$(
        impl Dispatch<$iface, $data> for Wl {
            fn event(
                _: &mut Self,
                _: &$iface,
                _: <$iface as Proxy>::Event,
                _: &$data,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
    )*};
}

ignore_events! {
    wl_registry::WlRegistry, GlobalListContents;
    wl_shm::WlShm, ();
    wl_shm_pool::WlShmPool, ();
    wl_buffer::WlBuffer, ();
    ExtImageCaptureSourceV1, ();
    ExtOutputImageCaptureSourceManagerV1, ();
    ExtForeignToplevelImageCaptureSourceManagerV1, ();
    ExtImageCopyCaptureManagerV1, ();
}

/// A round trip's callback: it fires once, when the compositor has handled everything before it.
impl Dispatch<wl_callback::WlCallback, Arc<OnceLock<()>>> for Wl {
    fn event(
        _: &mut Self,
        _: &wl_callback::WlCallback,
        event: wl_callback::Event,
        answered: &Arc<OnceLock<()>>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            let _ = answered.set(());
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for Wl {
    fn event(
        state: &mut Self,
        _: &ExtForeignToplevelListV1,
        event: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_list_v1::Event::Toplevel { toplevel } = event {
            state.toplevels.push(SeenToplevel {
                handle: toplevel,
                identifier: None,
                closed: false,
            });
        }
    }

    wayland_client::event_created_child!(Wl, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for Wl {
    fn event(
        state: &mut Self,
        handle: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(seen) = state
            .toplevels
            .iter_mut()
            .find(|t| t.handle.id() == handle.id())
        else {
            return;
        };
        match event {
            ext_foreign_toplevel_handle_v1::Event::Identifier { identifier } => {
                seen.identifier = Some(identifier);
            }
            ext_foreign_toplevel_handle_v1::Event::Closed => seen.closed = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for Wl {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => state.pending.size = Some((width, height)),
            Event::ShmFormat { format } => {
                if let Some(format) = format.into_result().ok().and_then(pixel_format) {
                    state.pending.formats.push(format);
                }
            }
            Event::Done => {
                state.current = Some(std::mem::take(&mut state.pending));
                state.generation += 1;
            }
            Event::Stopped => state.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, ()> for Wl {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::{Event, FailureReason};
        match event {
            Event::PresentationTime {
                tv_sec_hi,
                tv_sec_lo,
                tv_nsec,
            } => {
                let seconds = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
                state.frame.presented_ns = seconds
                    .saturating_mul(1_000_000_000)
                    .saturating_add(u64::from(tv_nsec));
            }
            Event::Ready => state.frame.outcome = Some(Outcome::Ready),
            Event::Failed { reason } => {
                state.frame.outcome = Some(match reason {
                    WEnum::Value(FailureReason::BufferConstraints) => Outcome::Constraints,
                    WEnum::Value(FailureReason::Stopped) => Outcome::Stopped,
                    _ => Outcome::Unknown,
                });
            }
            _ => {}
        }
    }
}
