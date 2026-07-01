//! Wayland drag-and-drop (DnD) implementation using `wl_data_device_manager`.
//!
//! This module provides DnD support through the standard Wayland data-device protocol.
//! It handles both the **source** side (initiating drags) and the **destination** side
//! (receiving drag-enter / motion / leave / drop events).

use std::os::unix::io::{AsRawFd, FromRawFd};
use std::sync::Arc;

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use wayland_backend::client::ObjectData;
use wayland_client::protocol::wl_data_device::WlDataDevice;
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::WlDataSource;

use crate::event::{DndWindowEvent, WindowEvent};
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::WindowId;

/// User-data attached to each `WlDataDevice`.
#[derive(Debug, Clone)]
pub struct DndDeviceData {
    /// The seat that owns this data device.
    pub seat: WlSeat,
}

/// User-data attached to each `WlDataOffer`.
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct DndOfferData {
    /// MIME types offered by the drag source.
    pub mime_types: Vec<String>,
}

/// User-data attached to each `WlDataSource`.
///
/// Contains the pre-serialized drag payload so the `Send` handler can write
/// data to the compositor without needing access to external state.
#[derive(Debug, Clone, Default)]
pub struct DndSourceData {
    /// MIME types offered by this source.
    pub mime_types: Vec<String>,
    /// Pre-serialized data for each MIME type (parallel to `mime_types`).
    pub data: Vec<Vec<u8>>,
}

/// Shared state for the current DnD offer.
///
/// This is wrapped in `Arc<Mutex<...>>` and shared between WinitState and WindowState
/// so that window methods can access the current offer to call accept/set_actions/finish.
#[derive(Debug, Default)]
pub struct SharedDndOfferState {
    /// The current drag offer, if any.
    pub current_offer: Option<WlDataOffer>,
    /// Pending data received from a request_data call.
    /// The event loop will drain this and emit DataReceived events.
    pub pending_data: Vec<(String, Vec<u8>)>,
    /// Payload of an OUTGOING drag WE started (mime → bytes), while it's active.
    /// For a self-drag (drop onto one of our own windows), `request_data` reads
    /// this DIRECTLY instead of the pipe: the source's `Send` runs on this same
    /// event-loop thread, so a blocking pipe read would deadlock the process.
    pub self_source: Option<Vec<(String, Vec<u8>)>>,
    /// The window that STARTED the current outgoing drag. Source-side events
    /// (DropPerformed/Finished/Cancelled) must be delivered here so the initiating
    /// window can clear its drag state — `focused_window` tracks the destination
    /// under the cursor (and is cleared on Leave), so it can't identify the source.
    pub source_window: Option<WindowId>,
}

/// Shared mutable state for the current DnD session.
///
/// This is stored in `WinitState` and modified by the Dispatch impls.
#[derive(Debug)]
pub struct DndSessionState {
    /// Shared reference to the current drag offer.
    /// This is wrapped in Arc<Mutex<>> so WindowState can also access it.
    pub shared_offer: std::sync::Arc<std::sync::Mutex<SharedDndOfferState>>,
    /// MIME types from the current offer.
    pub offer_mime_types: Vec<String>,
    /// The window currently under the drag cursor.
    pub focused_window: Option<WindowId>,
    /// The icon surface for the drag visual, if created.
    pub icon_surface: Option<WlSurface>,
    /// Set when a Drop event is received, prevents Leave from clearing the offer
    /// prematurely (the offer is needed for `finish()`).
    pub drop_received: bool,
    /// The window the Drop landed on, captured at Drop time. `Leave` clears
    /// `focused_window` before the requested data arrives, so the async
    /// `DataReceived` must be delivered here (not to an arbitrary fallback window)
    /// — critical when multiple windows are open.
    pub drop_window: Option<WindowId>,
}

impl Default for DndSessionState {
    fn default() -> Self {
        Self {
            shared_offer: std::sync::Arc::new(
                std::sync::Mutex::new(SharedDndOfferState::default()),
            ),
            offer_mime_types: Vec::new(),
            focused_window: None,
            icon_surface: None,
            drop_received: false,
            drop_window: None,
        }
    }
}

/// Manager for the `wl_data_device_manager` global.
#[derive(Debug, Clone)]
pub struct WaylandDndManager {
    manager: WlDataDeviceManager,
}

impl WaylandDndManager {
    /// Bind the `wl_data_device_manager` global (version 3).
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 3..=3, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create a `wl_data_device` for the given seat.
    pub fn get_data_device(
        &self,
        seat: &WlSeat,
        queue_handle: &QueueHandle<WinitState>,
    ) -> WlDataDevice {
        self.manager.get_data_device(seat, queue_handle, DndDeviceData { seat: seat.clone() })
    }

    /// Create a `wl_data_source` for starting a drag.
    pub fn create_data_source(
        &self,
        queue_handle: &QueueHandle<WinitState>,
        payload: DndSourceData,
    ) -> WlDataSource {
        self.manager.create_data_source(queue_handle, payload)
    }

    /// Get reference to the underlying `WlDataDeviceManager`.
    pub fn wl_manager(&self) -> &WlDataDeviceManager {
        &self.manager
    }
}

/// Create a small icon surface for drag visuals using SHM.
///
/// Returns a `WlSurface` backed by a 48×48 semi-transparent buffer that
/// can be passed as the icon to `wl_data_device.start_drag(…)`.
///
/// **Both** the surface and the buffer must be kept alive for the duration
/// of the drag — dropping the buffer would let the `SlotPool` reclaim its
/// backing memory.
pub fn create_dnd_icon_surface(
    compositor: &sctk::compositor::CompositorState,
    pool: &mut sctk::shm::slot::SlotPool,
    queue_handle: &QueueHandle<WinitState>,
) -> Option<(WlSurface, sctk::shm::slot::Buffer)> {
    let surface = compositor.create_surface(queue_handle);
    let (buffer, canvas) = pool
        .create_buffer(48, 48, 48 * 4, wayland_client::protocol::wl_shm::Format::Argb8888)
        .ok()?;

    // Draw a semi-transparent rounded rectangle icon.
    let (w, h) = (48i32, 48i32);
    let radius = 8i32;
    for y in 0..h {
        for x in 0..w {
            let offset = (y * w + x) as usize * 4;
            // Simple rounded-rect check.
            let inside = {
                let (cx, cy) = match (x < radius, y < radius, x >= w - radius, y >= h - radius) {
                    (true, true, _, _) => (radius, radius),
                    (_, true, true, _) => (w - radius - 1, radius),
                    (true, _, _, true) => (radius, h - radius - 1),
                    (_, _, true, true) => (w - radius - 1, h - radius - 1),
                    _ => (x, y), // Inside, not in a corner.
                };
                let dx = x - cx;
                let dy = y - cy;
                dx * dx + dy * dy <= radius * radius
            };
            if inside {
                // ARGB: semi-transparent white.
                canvas[offset] = 0xFF; // B
                canvas[offset + 1] = 0xFF; // G
                canvas[offset + 2] = 0xFF; // R
                canvas[offset + 3] = 0xB0; // A
            } else {
                canvas[offset] = 0;
                canvas[offset + 1] = 0;
                canvas[offset + 2] = 0;
                canvas[offset + 3] = 0;
            }
        }
    }

    surface.attach(Some(buffer.wl_buffer()), 0, 0);
    surface.damage_buffer(0, 0, w, h);
    surface.commit();

    Some((surface, buffer))
}

/// Create an icon surface from caller-provided ARGB pixel data.
///
/// `pixels` must be exactly `width * height * 4` bytes in pre-multiplied
/// ARGB (little-endian) format — the native Wayland `Argb8888` format.
///
/// Returns `(WlSurface, Buffer)` that must be kept alive for the drag.
pub fn create_dnd_icon_from_pixels(
    compositor: &sctk::compositor::CompositorState,
    pool: &mut sctk::shm::slot::SlotPool,
    queue_handle: &QueueHandle<WinitState>,
    width: u32,
    height: u32,
    pixels: &[u8],
    buffer_scale: i32,
) -> Option<(WlSurface, sctk::shm::slot::Buffer)> {
    let expected = (width * height * 4) as usize;
    if pixels.len() != expected {
        tracing::error!(expected, actual = pixels.len(), "DnD icon pixel data size mismatch");
        return None;
    }

    let surface = compositor.create_surface(queue_handle);
    let (buffer, canvas) = pool
        .create_buffer(
            width as i32,
            height as i32,
            (width * 4) as i32,
            wayland_client::protocol::wl_shm::Format::Argb8888,
        )
        .ok()?;

    canvas[..expected].copy_from_slice(pixels);

    // Set buffer scale for HiDPI support.
    if buffer_scale > 1 {
        surface.set_buffer_scale(buffer_scale);
    }

    surface.attach(Some(buffer.wl_buffer()), 0, 0);
    surface.damage_buffer(0, 0, width as i32, height as i32);
    surface.commit();

    Some((surface, buffer))
}

// ---------------------------------------------------------------------------
// Dispatch: WlDataDeviceManager
// ---------------------------------------------------------------------------

impl Dispatch<WlDataDeviceManager, GlobalData, WinitState> for WaylandDndManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &WlDataDeviceManager,
        _event: <WlDataDeviceManager as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events on the manager.
    }
}

// ---------------------------------------------------------------------------
// Dispatch: WlDataDevice
// ---------------------------------------------------------------------------

impl Dispatch<WlDataDevice, DndDeviceData, WinitState> for WaylandDndManager {
    fn event(
        state: &mut WinitState,
        _proxy: &WlDataDevice,
        event: <WlDataDevice as Proxy>::Event,
        _data: &DndDeviceData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        use wayland_client::protocol::wl_data_device::Event as DdEvent;

        let session = &mut state.dnd_session;

        match event {
            DdEvent::DataOffer { id } => {
                // A new data offer appeared — start of a new incoming drag. Store it
                // and reset per-drag destination state (a purely external drag never
                // fires our source-side cleanup, so these would otherwise leak into
                // the next drag and misroute its DataReceived).
                tracing::debug!("DnD: new data offer");
                session.drop_received = false;
                session.drop_window = None;
                session.shared_offer.lock().unwrap().current_offer = Some(id);
                session.offer_mime_types.clear();
            },
            DdEvent::Enter { surface, x, y, .. } => {
                let window_id = crate::platform_impl::wayland::make_wid(&surface);
                session.focused_window = Some(window_id);

                let mime_types = session.offer_mime_types.clone();
                tracing::info!(?window_id, x, y, ?mime_types, "DnD: enter");

                state.events_sink.push_window_event(
                    WindowEvent::Dnd(DndWindowEvent::Enter { x, y, mime_types }),
                    window_id,
                );
            },
            DdEvent::Motion { x, y, .. } => {
                if let Some(wid) = session.focused_window {
                    state
                        .events_sink
                        .push_window_event(WindowEvent::Dnd(DndWindowEvent::Motion { x, y }), wid);
                }
            },
            DdEvent::Leave => {
                if let Some(wid) = session.focused_window.take() {
                    tracing::debug!(?wid, drop_received = session.drop_received, "DnD: leave");
                    state
                        .events_sink
                        .push_window_event(WindowEvent::Dnd(DndWindowEvent::Leave), wid);
                }
                // Only clear the offer on leave if no drop was received.
                // After Drop, the offer must stay alive for the client to call finish().
                if !session.drop_received {
                    session.shared_offer.lock().unwrap().current_offer = None;
                    session.offer_mime_types.clear();
                }
            },
            DdEvent::Drop => {
                session.drop_received = true;
                // Capture the drop window now — the Leave that follows clears
                // `focused_window` before the requested data arrives.
                session.drop_window = session.focused_window;
                if let Some(wid) = session.focused_window {
                    tracing::debug!(?wid, "DnD: drop");
                    state
                        .events_sink
                        .push_window_event(WindowEvent::Dnd(DndWindowEvent::Drop), wid);
                }
            },
            DdEvent::Selection { .. } => {
                // Selection (clipboard) — not relevant for DnD.
            },
            _ => {},
        }
    }

    fn event_created_child(opcode: u16, qhandle: &QueueHandle<WinitState>) -> Arc<dyn ObjectData> {
        match opcode {
            // Opcode 0 = data_offer: creates a new wl_data_offer child object.
            0 => qhandle.make_data::<WlDataOffer, _>(()),
            _ => unreachable!("unknown wl_data_device child event opcode: {opcode}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch: WlDataOffer
// ---------------------------------------------------------------------------

impl Dispatch<WlDataOffer, (), WinitState> for WaylandDndManager {
    fn event(
        state: &mut WinitState,
        _proxy: &WlDataOffer,
        event: <WlDataOffer as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        use wayland_client::protocol::wl_data_offer::Event as OfferEvent;

        match event {
            OfferEvent::Offer { mime_type } => {
                tracing::trace!(mime_type, "DnD offer: mime type");
                state.dnd_session.offer_mime_types.push(mime_type);
            },
            OfferEvent::SourceActions { source_actions } => {
                tracing::trace!(?source_actions, "DnD offer: source actions");
            },
            OfferEvent::Action { dnd_action } => {
                let action_bits = dnd_action.into();
                if let Some(wid) = state.dnd_session.focused_window {
                    state.events_sink.push_window_event(
                        WindowEvent::Dnd(DndWindowEvent::SelectedAction(action_bits)),
                        wid,
                    );
                }
            },
            _ => {},
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch: WlDataSource
// ---------------------------------------------------------------------------

impl Dispatch<WlDataSource, DndSourceData, WinitState> for WaylandDndManager {
    fn event(
        state: &mut WinitState,
        proxy: &WlDataSource,
        event: <WlDataSource as Proxy>::Event,
        data: &DndSourceData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        use wayland_client::protocol::wl_data_source::Event as SrcEvent;

        // Source events belong to the window that STARTED the drag. Route to the
        // recorded source window (focused_window tracks the destination and is
        // cleared on Leave, so it can't identify the initiator); fall back to
        // focused/first only if unknown.
        let target_window = state
            .dnd_session
            .shared_offer
            .lock()
            .unwrap()
            .source_window
            .or(state.dnd_session.focused_window)
            .or_else(|| state.windows.borrow().keys().next().copied());

        match event {
            SrcEvent::Target { mime_type } => {
                tracing::debug!(?mime_type, "DnD source: target accepted");
            },
            SrcEvent::Send { mime_type, fd } => {
                tracing::debug!(mime_type, "DnD source: send requested");
                // Write pre-serialized data for the requested MIME type from user_data.
                if let Some(idx) = data.mime_types.iter().position(|m| m == &mime_type) {
                    if let Some(blob) = data.data.get(idx) {
                        tracing::debug!(bytes = blob.len(), "DnD source: writing data to fd");
                        // Write synchronously (small data).
                        // SAFETY: We borrow the fd for the write but must NOT close it —
                        // wayland-client owns the fd and will close it after this callback.
                        let mut file = unsafe { std::fs::File::from_raw_fd(fd.as_raw_fd()) };
                        let _ = std::io::Write::write_all(&mut file, blob);
                        // Leak the File so it does NOT close the fd on drop.
                        std::mem::forget(file);
                    } else {
                        tracing::error!("DnD source: no data for mime index {idx}");
                    }
                } else {
                    tracing::error!(
                        available = ?data.mime_types,
                        "DnD source: unknown mime_type requested"
                    );
                }
            },
            SrcEvent::Cancelled => {
                tracing::info!("DnD source: cancelled");
                if let Some(wid) = target_window {
                    state
                        .events_sink
                        .push_window_event(WindowEvent::Dnd(DndWindowEvent::SourceCancelled), wid);
                }
                // Destroy the source via the proxy reference.
                proxy.clone().destroy();
                // Clean up session state.
                state.dnd_session.icon_surface = None;
                state.dnd_session.drop_received = false;
                state.dnd_session.drop_window = None;
                // Clear the offer preserved for finish() and our own source payload.
                {
                    let mut shared = state.dnd_session.shared_offer.lock().unwrap();
                    shared.current_offer = None;
                    shared.self_source = None;
                    shared.source_window = None;
                }
                state.dnd_session.offer_mime_types.clear();
            },
            SrcEvent::DndDropPerformed => {
                tracing::info!("DnD source: drop performed");
                if let Some(wid) = target_window {
                    state.events_sink.push_window_event(
                        WindowEvent::Dnd(DndWindowEvent::SourceDropPerformed),
                        wid,
                    );
                }
                // Do NOT clear `self_source` here: for a self-drag, DndDropPerformed
                // fires BEFORE the destination's `dnd_request_data` reads the payload
                // — clearing now would force the blocking pipe read and DEADLOCK the
                // process. It's cleared on DndFinished (after the read) and
                // overwritten at the next start_drag.
            },
            SrcEvent::DndFinished => {
                tracing::info!("DnD source: finished");
                if let Some(wid) = target_window {
                    state
                        .events_sink
                        .push_window_event(WindowEvent::Dnd(DndWindowEvent::SourceFinished), wid);
                }
                // Destroy the source via the proxy reference.
                proxy.clone().destroy();
                // Clean up session state.
                state.dnd_session.icon_surface = None;
                state.dnd_session.drop_received = false;
                state.dnd_session.drop_window = None;
                {
                    let mut shared = state.dnd_session.shared_offer.lock().unwrap();
                    shared.self_source = None;
                    shared.source_window = None;
                }
            },
            SrcEvent::Action { dnd_action } => {
                let action_bits = dnd_action.into();
                tracing::trace!(action_bits, "DnD source: action");
                if let Some(wid) = target_window {
                    state.events_sink.push_window_event(
                        WindowEvent::Dnd(DndWindowEvent::SourceAction(action_bits)),
                        wid,
                    );
                }
            },
            _ => {},
        }
    }
}

// ---------------------------------------------------------------------------
// delegate_dispatch macros
// ---------------------------------------------------------------------------

delegate_dispatch!(WinitState: [WlDataDeviceManager: GlobalData] => WaylandDndManager);
delegate_dispatch!(WinitState: [WlDataDevice: DndDeviceData] => WaylandDndManager);
delegate_dispatch!(WinitState: [WlDataOffer: ()] => WaylandDndManager);
delegate_dispatch!(WinitState: [WlDataSource: DndSourceData] => WaylandDndManager);
