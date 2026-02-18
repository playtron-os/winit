//! Backdrop color protocol implementation.
//!
//! This protocol allows clients to request a compositor-rendered backdrop
//! color behind their surface content.

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
        wayland_scanner::generate_interfaces!("resources/protocols/backdrop-color.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/backdrop-color.xml");
}

pub use protocol::backdrop_color_manager_v1::BackdropColorManagerV1;
pub use protocol::backdrop_color_surface_v1::BackdropColorSurfaceV1;

/// Backdrop color manager (binds the compositor global).
#[derive(Debug, Clone)]
pub struct CosmicBackdropColorManager {
    manager: BackdropColorManagerV1,
}

/// Handle to a backdrop color object for a specific surface.
#[derive(Debug)]
pub struct BackdropColorController {
    controller: BackdropColorSurfaceV1,
}

impl CosmicBackdropColorManager {
    /// Try to bind the backdrop color manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create a backdrop color controller for a surface.
    pub fn get_backdrop_color(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> BackdropColorController {
        let controller = self.manager.get_backdrop_color(surface, queue_handle, ());
        BackdropColorController { controller }
    }
}

impl BackdropColorController {
    /// Set the backdrop color (RGBA, each 0-255).
    pub fn set_color(&self, r: u32, g: u32, b: u32, a: u32) {
        self.controller.set_color(r, g, b, a);
    }

    /// Unset the backdrop color.
    pub fn unset_color(&self) {
        self.controller.unset_color();
    }
}

impl Drop for BackdropColorController {
    fn drop(&mut self) {
        self.controller.destroy();
    }
}

// Dispatch implementations

impl Dispatch<BackdropColorManagerV1, GlobalData, WinitState> for CosmicBackdropColorManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &BackdropColorManagerV1,
        _event: <BackdropColorManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<BackdropColorSurfaceV1, (), WinitState> for CosmicBackdropColorManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &BackdropColorSurfaceV1,
        _event: <BackdropColorSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the surface backdrop color object
    }
}

delegate_dispatch!(WinitState: [BackdropColorManagerV1: GlobalData] => CosmicBackdropColorManager);
delegate_dispatch!(WinitState: [BackdropColorSurfaceV1: ()] => CosmicBackdropColorManager);
