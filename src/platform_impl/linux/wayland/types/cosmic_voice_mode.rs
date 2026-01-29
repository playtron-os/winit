//! COSMIC voice mode protocol implementation.
//!
//! This protocol allows clients to receive voice input events from the compositor.
//! The compositor controls the voice mode orb overlay - clients register surfaces
//! (windows) as receivers and get notified when voice input starts/stops.

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
        wayland_scanner::generate_interfaces!("resources/protocols/voice_mode.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/voice_mode.xml");
}

pub use protocol::zcosmic_voice_mode_manager_v1::ZcosmicVoiceModeManagerV1;
pub use protocol::zcosmic_voice_mode_v1::ZcosmicVoiceModeV1;

/// Orb display state from the compositor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrbState {
    /// Orb is not visible
    Hidden,
    /// Orb is floating (default receiver active)
    Floating,
    /// Orb is attached to a window
    Attached,
    /// Orb is frozen in place (processing)
    Frozen,
    /// Orb is transitioning to attached (non-interruptible)
    Transitioning,
}

impl From<u32> for OrbState {
    fn from(value: u32) -> Self {
        match value {
            0 => OrbState::Hidden,
            1 => OrbState::Floating,
            2 => OrbState::Attached,
            3 => OrbState::Frozen,
            4 => OrbState::Transitioning,
            _ => OrbState::Hidden,
        }
    }
}

/// Voice mode event sent from the compositor
#[derive(Debug, Clone)]
pub enum VoiceModeEvent {
    /// Voice input started
    Start {
        /// Where the orb is displayed
        orb_state: OrbState,
    },
    /// Voice input stopped normally
    Stop,
    /// Voice input cancelled
    Cancel,
    /// Orb attached to this receiver's window
    OrbAttached { x: i32, y: i32, width: i32, height: i32 },
    /// Orb detached from this receiver's window
    OrbDetached,
    /// Voice input is about to stop - client must respond with ack_stop
    WillStop {
        /// Serial to echo back in ack_stop
        serial: u32,
    },
}

/// Shared state for voice mode events
#[derive(Debug, Default)]
pub struct VoiceModeState {
    /// Queued events to be processed
    pub events: Mutex<Vec<VoiceModeEvent>>,
    /// Whether this is the default receiver
    pub is_default: Mutex<bool>,
}

impl VoiceModeState {
    /// Take all pending events
    pub fn take_events(&self) -> Vec<VoiceModeEvent> {
        let mut events = self.events.lock().unwrap();
        std::mem::take(&mut *events)
    }

    /// Push a new event
    fn push_event(&self, event: VoiceModeEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// COSMIC voice mode manager.
#[derive(Debug, Clone)]
pub struct CosmicVoiceModeManager {
    manager: ZcosmicVoiceModeManagerV1,
}

/// Voice mode receiver handle for a specific surface (window)
#[derive(Debug)]
pub struct VoiceModeReceiver {
    receiver: ZcosmicVoiceModeV1,
    /// Shared state for events
    pub state: Arc<VoiceModeState>,
    /// Whether this is the default receiver
    is_default: bool,
}

impl CosmicVoiceModeManager {
    /// Try to bind the voice mode manager global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Register a surface (window) as a voice input receiver.
    ///
    /// # Arguments
    /// * `surface` - The wl_surface to register for voice input
    /// * `is_default` - If true, this surface becomes the default receiver
    ///   for when no other registered surface is focused
    pub fn get_voice_mode(
        &self,
        surface: &WlSurface,
        is_default: bool,
        queue_handle: &QueueHandle<WinitState>,
    ) -> VoiceModeReceiver {
        let state = Arc::new(VoiceModeState::default());
        *state.is_default.lock().unwrap() = is_default;
        let receiver = self.manager.get_voice_mode(
            surface,
            if is_default { 1 } else { 0 },
            queue_handle,
            state.clone(),
        );
        VoiceModeReceiver { receiver, state, is_default }
    }
}

impl VoiceModeReceiver {
    /// Take all pending voice mode events.
    pub fn take_events(&self) -> Vec<VoiceModeEvent> {
        self.state.take_events()
    }

    /// Check if this is the default receiver.
    pub fn is_default(&self) -> bool {
        self.is_default
    }

    /// Set the audio level for voice mode visualization.
    ///
    /// # Arguments
    /// * `level` - Audio level from 0-1000, where 0 is silence and 1000 is maximum.
    pub fn set_audio_level(&self, level: u32) {
        self.receiver.set_audio_level(level);
    }

    /// Acknowledge a will_stop event from the compositor.
    ///
    /// # Arguments
    /// * `serial` - The serial from the will_stop event
    /// * `freeze` - If true, freeze the orb in place (transcription processing).
    ///              If false, proceed with hiding the orb.
    pub fn ack_stop(&self, serial: u32, freeze: bool) {
        self.receiver.ack_stop(serial, if freeze { 1 } else { 0 });
    }

    /// Dismiss the frozen orb.
    ///
    /// This tells the compositor to hide the orb when transcription completes
    /// without spawning a new window (e.g., empty result or error).
    /// Only valid when orb is in frozen state.
    pub fn dismiss(&self) {
        self.receiver.dismiss();
    }

    /// Destroy this receiver.
    pub fn destroy(&self) {
        self.receiver.destroy();
    }
}

impl Drop for VoiceModeReceiver {
    fn drop(&mut self) {
        self.receiver.destroy();
    }
}

// Dispatch implementations

impl Dispatch<ZcosmicVoiceModeManagerV1, GlobalData, WinitState> for CosmicVoiceModeManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZcosmicVoiceModeManagerV1,
        _event: <ZcosmicVoiceModeManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // No events defined for the manager
    }
}

impl Dispatch<ZcosmicVoiceModeV1, Arc<VoiceModeState>, WinitState> for CosmicVoiceModeManager {
    fn event(
        state: &mut WinitState,
        _proxy: &ZcosmicVoiceModeV1,
        event: <ZcosmicVoiceModeV1 as Proxy>::Event,
        data: &Arc<VoiceModeState>,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        use wayland_client::WEnum;

        // Mark that we have events to dispatch so the event loop wakes up
        state.dispatched_events = true;

        match event {
            protocol::zcosmic_voice_mode_v1::Event::Start { orb_state } => {
                let orb_state = match orb_state {
                    WEnum::Value(protocol::zcosmic_voice_mode_v1::OrbState::Hidden) => {
                        OrbState::Hidden
                    },
                    WEnum::Value(protocol::zcosmic_voice_mode_v1::OrbState::Floating) => {
                        OrbState::Floating
                    },
                    WEnum::Value(protocol::zcosmic_voice_mode_v1::OrbState::Attached) => {
                        OrbState::Attached
                    },
                    WEnum::Value(protocol::zcosmic_voice_mode_v1::OrbState::Frozen) => {
                        OrbState::Frozen
                    },
                    WEnum::Value(protocol::zcosmic_voice_mode_v1::OrbState::Transitioning) => {
                        OrbState::Transitioning
                    },
                    _ => OrbState::Hidden,
                };
                tracing::debug!(?orb_state, "Voice mode started");
                data.push_event(VoiceModeEvent::Start { orb_state });
            },
            protocol::zcosmic_voice_mode_v1::Event::Stop => {
                tracing::debug!("Voice mode stopped");
                data.push_event(VoiceModeEvent::Stop);
            },
            protocol::zcosmic_voice_mode_v1::Event::Cancel => {
                tracing::debug!("Voice mode cancelled");
                data.push_event(VoiceModeEvent::Cancel);
            },
            protocol::zcosmic_voice_mode_v1::Event::OrbAttached { x, y, width, height } => {
                tracing::debug!(x, y, width, height, "Voice orb attached");
                data.push_event(VoiceModeEvent::OrbAttached { x, y, width, height });
            },
            protocol::zcosmic_voice_mode_v1::Event::OrbDetached => {
                tracing::debug!("Voice orb detached");
                data.push_event(VoiceModeEvent::OrbDetached);
            },
            protocol::zcosmic_voice_mode_v1::Event::WillStop { serial } => {
                tracing::debug!(serial, "Voice mode will_stop received");
                data.push_event(VoiceModeEvent::WillStop { serial });
            },
        }
    }
}

delegate_dispatch!(WinitState: [ZcosmicVoiceModeManagerV1: GlobalData] => CosmicVoiceModeManager);
delegate_dispatch!(WinitState: [ZcosmicVoiceModeV1: Arc<VoiceModeState>] => CosmicVoiceModeManager);
