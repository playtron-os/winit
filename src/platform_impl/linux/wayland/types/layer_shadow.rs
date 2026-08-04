//! Compositor-drawn shadow protocol.
//!
//! Lets a surface ask the compositor to draw a drop shadow behind it, instead
//! of the client padding its surface out and drawing one itself. For a surface
//! that is also blurred that is the only workable arrangement: a client-drawn
//! shadow needs transparent padding around the content, and the blurred region
//! then has to be inset to match, leaving two rectangles to keep in agreement.
//!
//! The compositor rounds the shadow to whatever corner radius the surface has
//! hinted, so this pairs with `cosmic_corner_radius`.

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
        wayland_scanner::generate_interfaces!("resources/protocols/layer_shadow.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/layer_shadow.xml");
}

pub use protocol::layer_shadow_manager_v1::LayerShadowManagerV1;
pub use protocol::layer_shadow_surface_v1::LayerShadowSurfaceV1;

/// Shadow manager (binds the compositor global).
#[derive(Debug, Clone)]
pub struct LayerShadowManager {
    manager: LayerShadowManagerV1,
}

/// Handle to the shadow of one surface.
#[derive(Debug)]
pub struct ShadowController {
    controller: LayerShadowSurfaceV1,
}

impl LayerShadowManager {
    /// Try to bind the shadow manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create a shadow object for a surface.
    ///
    /// Only call this once per surface; a second call is a protocol error.
    pub fn get_shadow(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> ShadowController {
        let controller = self.manager.get_shadow(surface, queue_handle, ());
        ShadowController { controller }
    }
}

impl ShadowController {
    /// Ask for a shadow. Takes effect on the next surface commit.
    pub fn enable(&self) {
        self.controller.enable();
    }

    /// Stop drawing the shadow. Takes effect on the next surface commit.
    pub fn disable(&self) {
        self.controller.disable();
    }
}

impl Drop for ShadowController {
    fn drop(&mut self) {
        self.controller.destroy();
    }
}

impl Dispatch<LayerShadowManagerV1, GlobalData, WinitState> for LayerShadowManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &LayerShadowManagerV1,
        _event: <LayerShadowManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<LayerShadowSurfaceV1, (), WinitState> for LayerShadowManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &LayerShadowSurfaceV1,
        _event: <LayerShadowSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the surface shadow object
    }
}

delegate_dispatch!(WinitState: [LayerShadowManagerV1: GlobalData] => LayerShadowManager);
delegate_dispatch!(WinitState: [LayerShadowSurfaceV1: ()] => LayerShadowManager);
