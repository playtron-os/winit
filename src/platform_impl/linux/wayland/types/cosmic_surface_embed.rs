//! COSMIC surface embedding protocol implementation.
//!
//! This protocol allows clients to embed foreign toplevel windows within their
//! own surfaces. The embedded surface can be interactive or display-only.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

use crate::platform_impl::wayland::state::WinitState;

// Generate the protocol bindings from the XML
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/surface_embed.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/surface_embed.xml");
}

pub use protocol::zcosmic_embedded_surface_v1::ZcosmicEmbeddedSurfaceV1;
pub use protocol::zcosmic_surface_embed_manager_v1::ZcosmicSurfaceEmbedManagerV1;

/// Events from an embedded surface
#[derive(Debug, Clone)]
pub enum EmbeddedSurfaceEvent {
    /// The embedded toplevel's preferred size changed
    Configure { width: i32, height: i32 },
    /// A new frame is available (screencopy mode)
    Frame,
    /// The embedded toplevel was closed
    Closed,
    /// Pointer entered the embed region
    Entered,
    /// Pointer left the embed region
    Left,
}

/// Data associated with an embedded surface
#[derive(Debug, Default)]
pub struct EmbeddedSurfaceData {
    /// Pending events
    pub events: Vec<EmbeddedSurfaceEvent>,
    /// Whether the embedded surface is still valid
    pub valid: bool,
}

impl EmbeddedSurfaceData {
    fn new() -> Self {
        Self { events: Vec::new(), valid: true }
    }
}

/// COSMIC surface embed manager.
#[derive(Debug, Clone)]
pub struct CosmicSurfaceEmbedManager {
    manager: ZcosmicSurfaceEmbedManagerV1,
}

/// Handle to an embedded surface
#[derive(Debug)]
pub struct EmbeddedSurface {
    embedded: ZcosmicEmbeddedSurfaceV1,
    data: Arc<Mutex<EmbeddedSurfaceData>>,
}

impl CosmicSurfaceEmbedManager {
    /// Try to bind the surface embed manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Embed a toplevel by its ID (from toplevel-info protocol).
    pub fn embed_toplevel(
        &self,
        parent: &WlSurface,
        toplevel_id: &str,
        queue_handle: &QueueHandle<WinitState>,
    ) -> EmbeddedSurface {
        let data = Arc::new(Mutex::new(EmbeddedSurfaceData::new()));
        let embedded = self.manager.embed_toplevel(
            parent,
            toplevel_id.to_string(),
            queue_handle,
            data.clone(),
        );
        info!("Created embedded surface for toplevel '{}'", toplevel_id);
        EmbeddedSurface { embedded, data }
    }

    /// Embed a toplevel by process ID.
    ///
    /// This is useful when the parent application spawns a child process
    /// and wants to embed its window without needing to discover it via
    /// the toplevel-list protocol.
    ///
    /// The compositor will monitor for new toplevels from the specified PID.
    /// When a matching toplevel appears, it will be embedded.
    ///
    /// # Arguments
    /// * `parent` - The surface to embed into
    /// * `pid` - Process ID of the application to embed
    /// * `app_id` - Optional app_id hint for verification (can be empty)
    pub fn embed_toplevel_by_pid(
        &self,
        parent: &WlSurface,
        pid: u32,
        app_id: &str,
        queue_handle: &QueueHandle<WinitState>,
    ) -> EmbeddedSurface {
        let data = Arc::new(Mutex::new(EmbeddedSurfaceData::new()));
        let embedded = self.manager.embed_toplevel_by_pid(
            parent,
            pid,
            app_id.to_string(),
            queue_handle,
            data.clone(),
        );
        info!("Created embedded surface for PID {} (app_id hint: '{}')", pid, app_id);
        EmbeddedSurface { embedded, data }
    }
}

impl EmbeddedSurface {
    /// Set the geometry of the embedded surface within the parent.
    ///
    /// # Arguments
    /// * `x` - X position in parent surface coordinates
    /// * `y` - Y position in parent surface coordinates
    /// * `width` - Width of the embed region (must be positive)
    /// * `height` - Height of the embed region (must be positive)
    pub fn set_geometry(&self, x: i32, y: i32, width: i32, height: i32) {
        debug!("Setting embed geometry: ({}, {}, {}, {})", x, y, width, height);
        self.embedded.set_geometry(x, y, width, height);
    }

    /// Enable or disable input routing to the embedded surface.
    ///
    /// When interactive, pointer/keyboard/touch events within the embed
    /// region will be routed to the embedded toplevel.
    pub fn set_interactive(&self, interactive: bool) {
        debug!("Setting embed interactive: {}", interactive);
        self.embedded.set_interactive(if interactive { 1 } else { 0 });
    }

    /// Set the render mode for the embedded surface.
    ///
    /// * Mode 0 (live): Compositor renders the surface directly
    /// * Mode 1 (screencopy): Frames are captured and sent to client
    pub fn set_render_mode(&self, mode: u32) {
        debug!("Setting embed render mode: {}", mode);
        self.embedded.set_render_mode(mode);
    }

    /// Set the corner radius for clipping the embedded surface.
    ///
    /// This allows the parent to specify rounded corners that match its own UI.
    /// Each corner can have a different radius. Values are in logical pixels.
    /// A value of 0 means no rounding for that corner.
    ///
    /// # Arguments
    /// * `top_left` - Top-left corner radius
    /// * `top_right` - Top-right corner radius
    /// * `bottom_right` - Bottom-right corner radius
    /// * `bottom_left` - Bottom-left corner radius
    pub fn set_corner_radius(
        &self,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) {
        debug!(
            "Setting embed corner radius: [{}, {}, {}, {}]",
            top_left, top_right, bottom_right, bottom_left
        );
        self.embedded.set_corner_radius(top_left, top_right, bottom_right, bottom_left);
    }

    /// Set anchor-based positioning for the embedded surface.
    ///
    /// Instead of specifying absolute (x, y) coordinates, this allows
    /// positioning relative to the parent window edges. The geometry is
    /// automatically recalculated when the parent window resizes.
    ///
    /// # Arguments
    /// * `anchor` - Bitflags indicating which edges to anchor to (see `set_anchor` protocol docs)
    ///   - 0: none (use absolute positioning)
    ///   - 1: top
    ///   - 2: bottom  
    ///   - 4: left
    ///   - 8: right
    ///   - Combinations like 9 (top | right) are valid
    /// * `margin_top` - Margin from top edge (when anchored to top)
    /// * `margin_right` - Margin from right edge (when anchored to right)
    /// * `margin_bottom` - Margin from bottom edge (when anchored to bottom)
    /// * `margin_left` - Margin from left edge (when anchored to left)
    /// * `width` - Width of embed region (0 to stretch between left/right anchors)
    /// * `height` - Height of embed region (0 to stretch between top/bottom anchors)
    pub fn set_anchor(
        &self,
        anchor: u32,
        margin_top: i32,
        margin_right: i32,
        margin_bottom: i32,
        margin_left: i32,
        width: i32,
        height: i32,
    ) {
        debug!(
            "Setting embed anchor: anchor={}, margins=[{}, {}, {}, {}], size=({}, {})",
            anchor, margin_top, margin_right, margin_bottom, margin_left, width, height
        );
        self.embedded.set_anchor(
            protocol::zcosmic_embedded_surface_v1::Anchor::from_bits_truncate(anchor),
            margin_top,
            margin_right,
            margin_bottom,
            margin_left,
            width,
            height,
        );
    }

    /// Commit pending changes to the embedded surface.
    pub fn commit(&self) {
        self.embedded.commit();
    }

    /// Check if the embedded surface is still valid.
    pub fn is_valid(&self) -> bool {
        self.data.lock().unwrap().valid
    }

    /// Take pending events from this embedded surface.
    pub fn take_events(&self) -> Vec<EmbeddedSurfaceEvent> {
        std::mem::take(&mut self.data.lock().unwrap().events)
    }

    /// Destroy this embedded surface.
    pub fn destroy(&self) {
        self.embedded.destroy();
    }
}

impl Drop for EmbeddedSurface {
    fn drop(&mut self) {
        self.destroy();
    }
}

// Dispatch implementations

impl Dispatch<ZcosmicSurfaceEmbedManagerV1, GlobalData, WinitState> for CosmicSurfaceEmbedManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicSurfaceEmbedManagerV1,
        _event: <ZcosmicSurfaceEmbedManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // Manager has no events
    }
}

impl Dispatch<ZcosmicEmbeddedSurfaceV1, Arc<Mutex<EmbeddedSurfaceData>>, WinitState>
    for CosmicSurfaceEmbedManager
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicEmbeddedSurfaceV1,
        event: <ZcosmicEmbeddedSurfaceV1 as Proxy>::Event,
        data: &Arc<Mutex<EmbeddedSurfaceData>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        let mut data = data.lock().unwrap();

        match event {
            protocol::zcosmic_embedded_surface_v1::Event::Configure { width, height } => {
                debug!("Embedded surface configure: {}x{}", width, height);
                data.events.push(EmbeddedSurfaceEvent::Configure { width, height });
            },
            protocol::zcosmic_embedded_surface_v1::Event::Frame => {
                debug!("Embedded surface frame");
                data.events.push(EmbeddedSurfaceEvent::Frame);
            },
            protocol::zcosmic_embedded_surface_v1::Event::Closed => {
                info!("Embedded surface closed");
                data.valid = false;
                data.events.push(EmbeddedSurfaceEvent::Closed);
            },
            protocol::zcosmic_embedded_surface_v1::Event::Entered => {
                debug!("Pointer entered embed region");
                data.events.push(EmbeddedSurfaceEvent::Entered);
            },
            protocol::zcosmic_embedded_surface_v1::Event::Left => {
                debug!("Pointer left embed region");
                data.events.push(EmbeddedSurfaceEvent::Left);
            },
        }
    }
}

delegate_dispatch!(WinitState: [ZcosmicSurfaceEmbedManagerV1: GlobalData] => CosmicSurfaceEmbedManager);
delegate_dispatch!(WinitState: [ZcosmicEmbeddedSurfaceV1: Arc<Mutex<EmbeddedSurfaceData>>] => CosmicSurfaceEmbedManager);
