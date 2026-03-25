//! COSMIC tooltip protocol implementation.
//!
//! This protocol allows clients to create compositor-driven tooltip surfaces
//! that follow the pointer cursor automatically. The compositor handles all
//! positioning, eliminating client-side reposition round-trips.

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
        wayland_scanner::generate_interfaces!("resources/protocols/tooltip.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/tooltip.xml");
}

pub use protocol::zcosmic_tooltip_manager_v1::ZcosmicTooltipManagerV1;
pub use protocol::zcosmic_tooltip_v1::ZcosmicTooltipV1;

/// COSMIC tooltip manager.
#[derive(Debug, Clone)]
pub struct CosmicTooltipManager {
    manager: ZcosmicTooltipManagerV1,
}

/// Data associated with a tooltip object.
#[derive(Debug, Clone)]
pub struct TooltipObjectData {
    pub tooltip_surface: WlSurface,
    pub parent_surface: WlSurface,
}

impl CosmicTooltipManager {
    /// Try to bind the tooltip manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create a tooltip controller for the given surfaces.
    ///
    /// The compositor will position `tooltip_surface` relative to the pointer
    /// cursor while the pointer is over `parent_surface`.
    pub fn get_tooltip(
        &self,
        tooltip_surface: &WlSurface,
        parent_surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> TooltipHandle {
        let data = TooltipObjectData {
            tooltip_surface: tooltip_surface.clone(),
            parent_surface: parent_surface.clone(),
        };
        let tooltip = self.manager.get_tooltip(tooltip_surface, parent_surface, queue_handle, data);
        TooltipHandle { tooltip }
    }
}

/// Handle for a compositor-driven tooltip object.
#[derive(Debug)]
pub struct TooltipHandle {
    tooltip: ZcosmicTooltipV1,
}

impl TooltipHandle {
    /// Set the offset from pointer to tooltip top-left corner.
    pub fn set_offset(&self, x: i32, y: i32) {
        self.tooltip.set_offset(x, y);
    }

    /// Set the anchor corner (0=TopLeft, 1=TopRight, 2=BottomLeft, 3=BottomRight).
    pub fn set_anchor(&self, anchor: u32) {
        use protocol::zcosmic_tooltip_v1::Anchor;
        let anchor_val = match anchor {
            1 => Anchor::TopRight,
            2 => Anchor::BottomLeft,
            3 => Anchor::BottomRight,
            _ => Anchor::TopLeft,
        };
        self.tooltip.set_anchor(anchor_val);
    }

    /// Set the delay in milliseconds before showing the tooltip.
    /// 0 = immediate (follows pointer), non-zero = delayed (fixed position).
    pub fn set_show_delay(&self, milliseconds: u32) {
        self.tooltip.set_show_delay(milliseconds);
    }

    /// Destroy the tooltip controller.
    pub fn destroy(&self) {
        self.tooltip.destroy();
    }
}

impl Drop for TooltipHandle {
    fn drop(&mut self) {
        self.tooltip.destroy();
    }
}

// Dispatch implementations

impl Dispatch<ZcosmicTooltipManagerV1, GlobalData, WinitState> for CosmicTooltipManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicTooltipManagerV1,
        _event: <ZcosmicTooltipManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<ZcosmicTooltipV1, TooltipObjectData, WinitState> for CosmicTooltipManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicTooltipV1,
        event: <ZcosmicTooltipV1 as Proxy>::Event,
        _data: &TooltipObjectData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        match event {
            protocol::zcosmic_tooltip_v1::Event::Reposition { x, y } => {
                tracing::trace!("Tooltip repositioned: ({}, {})", x, y);
            },
        }
    }
}

delegate_dispatch!(WinitState: [ZcosmicTooltipManagerV1: GlobalData] => CosmicTooltipManager);
delegate_dispatch!(WinitState: [ZcosmicTooltipV1: TooltipObjectData] => CosmicTooltipManager);
