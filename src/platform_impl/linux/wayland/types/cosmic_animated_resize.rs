//! COSMIC animated resize protocol implementation.
//!
//! This protocol allows clients to request smooth animated window resizes.
//! The compositor sends intermediate configure events for smooth animation.

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
        wayland_scanner::generate_interfaces!("resources/protocols/animated_resize.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/animated_resize.xml");
}

pub use protocol::zcosmic_animated_resize_manager_v1::ZcosmicAnimatedResizeManagerV1;
pub use protocol::zcosmic_animated_resize_v1::ZcosmicAnimatedResizeV1;

/// COSMIC animated resize manager.
#[derive(Debug, Clone)]
pub struct CosmicAnimatedResizeManager {
    manager: ZcosmicAnimatedResizeManagerV1,
}

/// Handle to an animated resize controller for a specific surface.
#[derive(Debug)]
pub struct AnimatedResizeController {
    controller: ZcosmicAnimatedResizeV1,
}

impl CosmicAnimatedResizeManager {
    /// Try to bind the animated resize manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create an animated resize controller for a surface.
    pub fn get_animated_resize(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> AnimatedResizeController {
        let controller = self.manager.get_animated_resize(surface, queue_handle, ());
        AnimatedResizeController { controller }
    }
}

impl AnimatedResizeController {
    /// Request the compositor to animate a resize from current size to target size.
    ///
    /// # Arguments
    /// * `width` - Target width in logical pixels (0 = don't change)
    /// * `height` - Target height in logical pixels (0 = don't change)
    /// * `duration_ms` - Animation duration in milliseconds
    pub fn resize_to(&self, width: i32, height: i32, duration_ms: u32) {
        self.controller.resize_to(width, height, duration_ms);
    }

    /// Request the compositor to animate a resize from current geometry to target geometry.
    ///
    /// # Arguments
    /// * `x` - Target x position in logical pixels
    /// * `y` - Target y position in logical pixels
    /// * `width` - Target width in logical pixels (0 = don't change)
    /// * `height` - Target height in logical pixels (0 = don't change)
    /// * `duration_ms` - Animation duration in milliseconds
    pub fn resize_to_with_position(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        duration_ms: u32,
    ) {
        self.controller.resize_to_with_position(x, y, width, height, duration_ms);
    }

    /// Destroy this controller.
    pub fn destroy(&self) {
        self.controller.destroy();
    }
}

impl Drop for AnimatedResizeController {
    fn drop(&mut self) {
        // The protocol requires explicit destroy
        self.controller.destroy();
    }
}

// Dispatch implementations

impl Dispatch<ZcosmicAnimatedResizeManagerV1, GlobalData, WinitState>
    for CosmicAnimatedResizeManager
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicAnimatedResizeManagerV1,
        _event: <ZcosmicAnimatedResizeManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<ZcosmicAnimatedResizeV1, (), WinitState> for CosmicAnimatedResizeManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicAnimatedResizeV1,
        event: <ZcosmicAnimatedResizeV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        match event {
            protocol::zcosmic_animated_resize_v1::Event::Done => {
                tracing::debug!("Animated resize completed");
            },
            protocol::zcosmic_animated_resize_v1::Event::Cancelled => {
                tracing::debug!("Animated resize cancelled");
            },
        }
    }
}

delegate_dispatch!(WinitState: [ZcosmicAnimatedResizeManagerV1: GlobalData] => CosmicAnimatedResizeManager);
delegate_dispatch!(WinitState: [ZcosmicAnimatedResizeV1: ()] => CosmicAnimatedResizeManager);
