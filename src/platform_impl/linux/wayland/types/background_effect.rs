//! Background effect protocol implementation.
//!
//! Asks the compositor to blur whatever is behind a surface, so a translucent
//! surface reads as frosted glass rather than a plain alpha blend.
//!
//! This is upstream's staging protocol: the client marks a region and the
//! compositor decides everything else -- there is no way to ask for a blur
//! strength, round the blurred area, or set the frosted-glass appearance.
//!
//! It therefore does not replace the KDE blur protocol, which carries all of
//! those. Compositors commonly implement only one of the two, so a client
//! should set both and let whichever is understood take effect.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_region::WlRegion;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};

use crate::platform_impl::wayland::state::WinitState;

// Generate the protocol bindings from the XML
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/ext-background-effect-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/ext-background-effect-v1.xml");
}

pub use protocol::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1;
pub use protocol::ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1;

/// Background effect manager (binds the compositor global).
#[derive(Debug, Clone)]
pub struct BackgroundEffectManager {
    manager: ExtBackgroundEffectManagerV1,
}

/// A surface's background effect.
///
/// The compositor raises a protocol error if a surface is given two of these,
/// so a caller must keep one per surface rather than creating them per use.
#[derive(Debug)]
pub struct BackgroundEffect {
    effect: ExtBackgroundEffectSurfaceV1,
}

impl BackgroundEffectManager {
    /// Try to bind the background effect manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create the background effect for a surface.
    ///
    /// Only call this once per surface; the compositor treats a second call as
    /// a protocol error.
    pub fn get_background_effect(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> BackgroundEffect {
        let effect = self.manager.get_background_effect(surface, queue_handle, ());
        BackgroundEffect { effect }
    }
}

impl BackgroundEffect {
    /// Blur the background within `region`, in surface-local coordinates.
    ///
    /// The protocol has no whole-surface mode, so a caller wanting the whole
    /// background blurred passes a surface-sized region and resends it on
    /// resize. Takes effect on the next surface commit.
    pub fn set_blur_region(&self, region: Option<&WlRegion>) {
        self.effect.set_blur_region(region);
    }

    /// Remove the effect.
    pub fn unset(&self) {
        self.effect.set_blur_region(None);
    }
}

impl Drop for BackgroundEffect {
    fn drop(&mut self) {
        self.effect.destroy();
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, GlobalData, WinitState> for BackgroundEffectManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ExtBackgroundEffectManagerV1,
        _event: <ExtBackgroundEffectManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // The only event is `capabilities`, and blur is the only capability
        // this protocol defines, so there is nothing to branch on yet.
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, (), WinitState> for BackgroundEffectManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ExtBackgroundEffectSurfaceV1,
        _event: <ExtBackgroundEffectSurfaceV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // This object has no events.
    }
}

delegate_dispatch!(WinitState: [ExtBackgroundEffectManagerV1: GlobalData] => BackgroundEffectManager);
delegate_dispatch!(WinitState: [ExtBackgroundEffectSurfaceV1: ()] => BackgroundEffectManager);
