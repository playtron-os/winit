//! Special action protocol implementation (zcosmic_special_action_v1).
//!
//! The device's special key — the HUMAIN button — is a gesture the compositor
//! resolves, because it is usually bound to a modifier the compositor also needs
//! for its own chords. Clients register surfaces as receivers and are told the
//! meaning rather than the key: `activate` for a tap, `hold_start`/`hold_end`
//! around a hold.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use std::sync::{Arc, Mutex};

use crate::platform_impl::wayland::state::WinitState;

// Generate the protocol bindings from the XML
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/special_action.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/special_action.xml");
}

pub use protocol::zcosmic_special_action_manager_v1::ZcosmicSpecialActionManagerV1;
pub use protocol::zcosmic_special_action_v1::ZcosmicSpecialActionV1;

pub use crate::event::SpecialActionEvent;

/// Events queued for one receiver until the event loop drains them.
#[derive(Debug, Default)]
pub struct SpecialActionState {
    events: Mutex<Vec<SpecialActionEvent>>,
}

impl SpecialActionState {
    /// Take all pending events.
    pub fn take_events(&self) -> Vec<SpecialActionEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    fn push_event(&self, event: SpecialActionEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// The bound manager global.
#[derive(Debug, Clone)]
pub struct CosmicSpecialActionManager {
    manager: ZcosmicSpecialActionManagerV1,
}

/// A registered receiver for one surface.
#[derive(Debug)]
pub struct SpecialActionReceiver {
    receiver: ZcosmicSpecialActionV1,
    /// Shared queue the dispatch impl pushes into.
    pub state: Arc<SpecialActionState>,
}

impl CosmicSpecialActionManager {
    /// Try to bind the manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Register a surface as a receiver.
    ///
    /// `is_default` also makes it the fallback, used whenever no registered
    /// surface is focused. Only one fallback exists at a time.
    pub fn get_special_action(
        &self,
        surface: &WlSurface,
        is_default: bool,
        queue_handle: &QueueHandle<WinitState>,
    ) -> SpecialActionReceiver {
        let state = Arc::new(SpecialActionState::default());
        let receiver = self.manager.get_special_action(
            surface,
            u32::from(is_default),
            queue_handle,
            state.clone(),
        );
        SpecialActionReceiver { receiver, state }
    }
}

impl SpecialActionReceiver {
    /// Take all pending events.
    pub fn take_events(&self) -> Vec<SpecialActionEvent> {
        self.state.take_events()
    }

    /// Unregister this surface.
    pub fn destroy(&self) {
        self.receiver.destroy();
    }
}

impl Drop for SpecialActionReceiver {
    fn drop(&mut self) {
        self.receiver.destroy();
    }
}

impl Dispatch<ZcosmicSpecialActionManagerV1, GlobalData, WinitState>
    for CosmicSpecialActionManager
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicSpecialActionManagerV1,
        _event: <ZcosmicSpecialActionManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // The manager has no events.
    }
}

impl Dispatch<ZcosmicSpecialActionV1, Arc<SpecialActionState>, WinitState>
    for CosmicSpecialActionManager
{
    fn event(
        state: &mut WinitState,
        _proxy: &ZcosmicSpecialActionV1,
        event: <ZcosmicSpecialActionV1 as Proxy>::Event,
        data: &Arc<SpecialActionState>,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // Wake the event loop so the queued event is drained this iteration.
        state.dispatched_events = true;

        let resolved = match event {
            protocol::zcosmic_special_action_v1::Event::Activate => SpecialActionEvent::Activate,
            protocol::zcosmic_special_action_v1::Event::HoldStart => SpecialActionEvent::HoldStart,
            protocol::zcosmic_special_action_v1::Event::HoldEnd => SpecialActionEvent::HoldEnd,
            protocol::zcosmic_special_action_v1::Event::Cancel => SpecialActionEvent::Cancel,
        };
        tracing::debug!(?resolved, "Special action event");
        data.push_event(resolved);
    }
}

delegate_dispatch!(WinitState: [ZcosmicSpecialActionManagerV1: GlobalData] => CosmicSpecialActionManager);
delegate_dispatch!(WinitState: [ZcosmicSpecialActionV1: Arc<SpecialActionState>] => CosmicSpecialActionManager);
