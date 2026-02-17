//! Corner radius protocol implementation.
//!
//! This protocol allows clients to communicate corner radius hints to the
//! compositor for their surfaces. The compositor uses this to draw fitting
//! blur outlines and rounded corners.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};

use crate::platform_impl::wayland::state::WinitState;

// Generate the protocol bindings from the XML
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/corner_radius.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/corner_radius.xml");
}

pub use protocol::layer_corner_radius_manager_v1::LayerCornerRadiusManagerV1;
pub use protocol::layer_corner_radius_surface_v1::LayerCornerRadiusSurfaceV1;

/// Corner radius manager (binds the compositor global).
#[derive(Debug, Clone)]
pub struct CosmicCornerRadiusManager {
    manager: LayerCornerRadiusManagerV1,
}

/// Handle to a corner radius object for a specific surface.
#[derive(Debug)]
pub struct CornerRadiusController {
    controller: LayerCornerRadiusSurfaceV1,
}

impl CosmicCornerRadiusManager {
    /// Try to bind the corner radius manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create a corner radius controller for a surface.
    pub fn get_corner_radius(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> CornerRadiusController {
        let controller = self.manager.get_corner_radius(surface, queue_handle, ());
        CornerRadiusController { controller }
    }
}

impl CornerRadiusController {
    /// Set the corner radius for all four corners.
    pub fn set_radius(&self, top_left: u32, top_right: u32, bottom_right: u32, bottom_left: u32) {
        self.controller.set_radius(top_left, top_right, bottom_right, bottom_left);
    }

    /// Unset any previously hinted corner radius values.
    pub fn unset_radius(&self) {
        self.controller.unset_radius();
    }
}

impl Drop for CornerRadiusController {
    fn drop(&mut self) {
        self.controller.destroy();
    }
}

// Dispatch implementations

impl Dispatch<LayerCornerRadiusManagerV1, GlobalData, WinitState> for CosmicCornerRadiusManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &LayerCornerRadiusManagerV1,
        _event: <LayerCornerRadiusManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<LayerCornerRadiusSurfaceV1, (), WinitState> for CosmicCornerRadiusManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &LayerCornerRadiusSurfaceV1,
        _event: <LayerCornerRadiusSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the surface corner radius object
    }
}

delegate_dispatch!(WinitState: [LayerCornerRadiusManagerV1: GlobalData] => CosmicCornerRadiusManager);
delegate_dispatch!(WinitState: [LayerCornerRadiusSurfaceV1: ()] => CosmicCornerRadiusManager);
