//! Background effect protocol implementation.
//!
//! Asks the compositor to blur whatever is behind a surface, so a translucent
//! surface reads as frosted glass rather than a plain alpha blend.
//!
//! Version 1 is the upstream staging protocol: the client marks a region and
//! the compositor decides everything else. Version 2 additionally lets the
//! client ask for a blur strength, round the blurred area to match the shape it
//! draws, and blur the whole surface without resending a region on every
//! resize. The manager binds `1..=2`, so a version 1 compositor still works and
//! the version 2 requests are simply unavailable -- check [`Self::version`]
//! before calling them.
//!
//! This does not replace the KDE blur protocol. Compositors that implement only
//! one of the two are common, so a client should set both.

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

/// The version at which the radius, corner and whole-surface requests appear.
const VERSION_WITH_RADIUS: u32 = 2;

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
    version: u32,
}

impl BackgroundEffectManager {
    /// Try to bind the background effect manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        // Accept a version 1 compositor: the base protocol still gets us a
        // blurred region, just without strength or corner control.
        let manager = globals.bind(queue_handle, 1..=VERSION_WITH_RADIUS, GlobalData)?;
        Ok(Self { manager })
    }

    /// The bound protocol version.
    pub fn version(&self) -> u32 {
        self.manager.version()
    }

    /// Whether the compositor supports the strength, corner and whole-surface
    /// requests.
    pub fn supports_radius(&self) -> bool {
        self.version() >= VERSION_WITH_RADIUS
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
        BackgroundEffect { effect, version: self.version() }
    }
}

impl BackgroundEffect {
    /// Blur the background within `region`, in surface-local coordinates.
    ///
    /// Supersedes a previous [`Self::blur_whole_surface`]. Takes effect on the
    /// next surface commit.
    pub fn set_blur_region(&self, region: Option<&WlRegion>) {
        self.effect.set_blur_region(region);
    }

    /// Remove the effect.
    pub fn unset(&self) {
        self.effect.set_blur_region(None);
    }

    /// Blur the whole surface, letting the compositor keep the area matched to
    /// it as it resizes.
    ///
    /// Silently does nothing before version 2; use [`Self::set_blur_region`]
    /// with a surface-sized region there instead.
    pub fn blur_whole_surface(&self) {
        if self.version >= VERSION_WITH_RADIUS {
            self.effect.blur_whole_surface();
        }
    }

    /// Ask for a blur strength, in surface-local coordinates. `0` means the
    /// compositor's default, not "no blur".
    ///
    /// This is a hint; the compositor may clamp or ignore it. Silently does
    /// nothing before version 2.
    pub fn set_blur_radius(&self, radius: u32) {
        if self.version >= VERSION_WITH_RADIUS {
            self.effect.set_blur_radius(radius);
        }
    }

    /// Round the corners of the blurred area, one entry per region rect,
    /// clockwise from top-left, so the backdrop follows the shape the surface
    /// draws.
    ///
    /// Entries are index-matched to the region's rectangles; a single entry
    /// rounds every rectangle the same way. Silently does nothing before
    /// version 2.
    pub fn set_region_radii(&self, radii: &[[u32; 4]]) {
        if self.version >= VERSION_WITH_RADIUS && !radii.is_empty() {
            let encoded = radii
                .iter()
                .flat_map(|corners| corners.iter().flat_map(|r| r.to_ne_bytes()))
                .collect();
            self.effect.set_region_radii(encoded);
        }
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
