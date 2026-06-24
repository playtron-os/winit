//! Client binding for `zwp_keyboard_shortcuts_inhibit_manager_v1`.
//!
//! While a window holds an active inhibitor, the compositor forwards every key
//! (including its own reserved combos, e.g. Super) to that window instead of
//! handling them as global shortcuts. Used by key-capture / shortcut-recording
//! UIs that need to observe combos the compositor would otherwise swallow.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use sctk::reexports::protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1;
use sctk::reexports::protocols::wp::keyboard_shortcuts_inhibit::zv1::client::zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1;

use crate::platform_impl::wayland::state::WinitState;

pub struct KeyboardShortcutsInhibitState {
    manager: ZwpKeyboardShortcutsInhibitManagerV1,
}

impl KeyboardShortcutsInhibitState {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// A clone of the manager, for creating per-window inhibitors.
    pub fn manager(&self) -> ZwpKeyboardShortcutsInhibitManagerV1 {
        self.manager.clone()
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, GlobalData, WinitState>
    for KeyboardShortcutsInhibitState
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpKeyboardShortcutsInhibitManagerV1,
        _event: <ZwpKeyboardShortcutsInhibitManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // The manager is eventless.
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, GlobalData, WinitState>
    for KeyboardShortcutsInhibitState
{
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpKeyboardShortcutsInhibitorV1,
        _event: <ZwpKeyboardShortcutsInhibitorV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // `active` / `inactive` notifications — no action needed.
    }
}

delegate_dispatch!(WinitState: [ZwpKeyboardShortcutsInhibitManagerV1: GlobalData] => KeyboardShortcutsInhibitState);
delegate_dispatch!(WinitState: [ZwpKeyboardShortcutsInhibitorV1: GlobalData] => KeyboardShortcutsInhibitState);
