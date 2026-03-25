//! COSMIC exclusive mode protocol implementation.
//!
//! This protocol allows clients to request exclusive mode for their window.
//! When exclusive mode is enabled, the compositor will minimize all other
//! toplevel windows on the same output. When disabled, they are restored.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use crate::platform_impl::wayland::state::WinitState;

// Generate the protocol bindings from the XML
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/exclusive_mode.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/exclusive_mode.xml");
}

pub use protocol::zcosmic_exclusive_mode_manager_v1::ZcosmicExclusiveModeManagerV1;
pub use protocol::zcosmic_exclusive_mode_v1::ZcosmicExclusiveModeV1;

/// COSMIC exclusive mode manager.
#[derive(Debug, Clone)]
pub struct CosmicExclusiveModeManager {
    manager: ZcosmicExclusiveModeManagerV1,
}

/// Shared state for tracking exclusive mode events
#[derive(Debug, Default)]
pub struct ExclusiveModeState {
    /// Whether exclusive mode is currently enabled
    pub enabled: AtomicBool,
    /// Number of windows affected by last operation
    pub affected_count: AtomicU32,
    /// Whether the last request failed
    pub failed: AtomicBool,
}

/// Handle to an exclusive mode controller for a specific surface.
#[derive(Debug)]
pub struct ExclusiveModeController {
    controller: ZcosmicExclusiveModeV1,
    /// Shared state for tracking events
    pub state: Arc<ExclusiveModeState>,
}

impl CosmicExclusiveModeManager {
    /// Try to bind the exclusive mode manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Create an exclusive mode controller for a surface.
    pub fn get_exclusive_mode(
        &self,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> ExclusiveModeController {
        let state = Arc::new(ExclusiveModeState::default());
        let controller = self.manager.get_exclusive_mode(surface, queue_handle, state.clone());
        ExclusiveModeController { controller, state }
    }
}

impl ExclusiveModeController {
    /// Enable or disable exclusive mode.
    ///
    /// When enabled (true):
    /// - All other toplevel windows on the same output are minimized
    /// - This window remains visible and gains focus
    ///
    /// When disabled (false):
    /// - Windows that were minimized by this exclusive mode are restored
    pub fn set_exclusive(&self, exclusive: bool) {
        self.controller.set_exclusive(if exclusive { 1 } else { 0 });
    }

    /// Check if exclusive mode is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.state.enabled.load(Ordering::SeqCst)
    }

    /// Get the number of windows affected by the last operation.
    pub fn affected_count(&self) -> u32 {
        self.state.affected_count.load(Ordering::SeqCst)
    }

    /// Check if the last request failed.
    pub fn has_failed(&self) -> bool {
        self.state.failed.load(Ordering::SeqCst)
    }

    /// Destroy this controller.
    /// If exclusive mode was enabled, this implicitly disables it.
    pub fn destroy(&self) {
        self.controller.destroy();
    }
}

impl Drop for ExclusiveModeController {
    fn drop(&mut self) {
        // The protocol requires explicit destroy
        self.controller.destroy();
    }
}

// Dispatch implementations

impl Dispatch<ZcosmicExclusiveModeManagerV1, GlobalData, WinitState>
    for CosmicExclusiveModeManager
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicExclusiveModeManagerV1,
        _event: <ZcosmicExclusiveModeManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<ZcosmicExclusiveModeV1, Arc<ExclusiveModeState>, WinitState>
    for CosmicExclusiveModeManager
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicExclusiveModeV1,
        event: <ZcosmicExclusiveModeV1 as Proxy>::Event,
        data: &Arc<ExclusiveModeState>,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        match event {
            protocol::zcosmic_exclusive_mode_v1::Event::Enabled { count } => {
                tracing::debug!(count, "Exclusive mode enabled");
                data.enabled.store(true, Ordering::SeqCst);
                data.affected_count.store(count, Ordering::SeqCst);
                data.failed.store(false, Ordering::SeqCst);
            },
            protocol::zcosmic_exclusive_mode_v1::Event::Disabled { count } => {
                tracing::debug!(count, "Exclusive mode disabled");
                data.enabled.store(false, Ordering::SeqCst);
                data.affected_count.store(count, Ordering::SeqCst);
                data.failed.store(false, Ordering::SeqCst);
            },
            protocol::zcosmic_exclusive_mode_v1::Event::Failed { reason } => {
                tracing::warn!(reason, "Exclusive mode request failed");
                data.failed.store(true, Ordering::SeqCst);
            },
        }
    }
}

delegate_dispatch!(WinitState: [ZcosmicExclusiveModeManagerV1: GlobalData] => CosmicExclusiveModeManager);
delegate_dispatch!(WinitState: [ZcosmicExclusiveModeV1: Arc<ExclusiveModeState>] => CosmicExclusiveModeManager);
