//! Kora Halo header protocol.
//!
//! On Kora, the compositor draws window chrome as the Halo: a pill floating at
//! the window's top edge. By default it reserves room above the client, so
//! the window grows by the pill's height. A client laid out for chromeless
//! windows — its first row already clear of the Halo — opts into `overlay`,
//! and the pill floats over its top edge instead. The request only counts
//! between creating the toplevel and its initial commit.

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
        wayland_scanner::generate_interfaces!("resources/protocols/kora-halo-header-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/kora-halo-header-v1.xml");
}

pub use protocol::kora_halo_header_manager_v1::{KoraHaloHeaderManagerV1, Mode};

/// The Halo header manager (binds the compositor global).
#[derive(Debug, Clone)]
pub struct KoraHaloHeaderManager {
    manager: KoraHaloHeaderManagerV1,
}

impl KoraHaloHeaderManager {
    /// Try to bind the Halo header manager global. Absent on compositors that
    /// draw no Halo, where a window keeps ordinary decoration geometry.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Let the Halo float over `surface`'s top edge rather than reserve room
    /// above it. Must be sent after the surface has its `xdg_toplevel` role and
    /// before its initial commit; later is a protocol error.
    pub fn set_overlay(&self, surface: &WlSurface) {
        self.manager.set_mode(surface, Mode::Overlay);
    }
}

// Dispatch implementations

impl Dispatch<KoraHaloHeaderManagerV1, GlobalData, WinitState> for KoraHaloHeaderManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &KoraHaloHeaderManagerV1,
        _event: <KoraHaloHeaderManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

delegate_dispatch!(WinitState: [KoraHaloHeaderManagerV1: GlobalData] => KoraHaloHeaderManager);
