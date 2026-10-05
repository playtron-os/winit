//! Kora identity lookup for this client's xdg_toplevel.
//! XML mirrors cosmic-protocols 40e5fd10's kora-toplevel-identity-v1.xml.

use std::sync::{Arc, Mutex};

use sctk::globals::GlobalData;
use wayland_client::globals::{BindError, GlobalList};
use wayland_client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

use crate::platform_impl::wayland::state::WinitState;
use crate::window::Identity;

pub mod protocol {
    use wayland_client;
    use wayland_protocols::ext::foreign_toplevel_list::v1::client::*;
    use wayland_protocols::xdg::shell::client::*;

    pub mod __interfaces {
        use wayland_protocols::ext::foreign_toplevel_list::v1::client::__interfaces::*;
        use wayland_protocols::xdg::shell::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/kora-toplevel-identity-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/kora-toplevel-identity-v1.xml");
}

use protocol::kora_toplevel_identity_handle_v1::{Event, KoraToplevelIdentityHandleV1};
use protocol::kora_toplevel_identity_v1::KoraToplevelIdentityV1;

#[derive(Debug, Default)]
pub struct IdentityState {
    identifier: Option<String>,
    workspace: Option<String>,
    current: Option<Identity>,
    events: Vec<Option<Identity>>,
    done: bool,
    closed: bool,
    had_identity: bool,
}

impl IdentityState {
    fn event(&mut self, event: Event) {
        if self.closed {
            return;
        }
        match event {
            Event::Identifier { identifier } if !self.done => self.identifier = Some(identifier),
            Event::Workspace { workspace } if !self.done => self.workspace = Some(workspace),
            Event::Done if !self.done => {
                self.done = true;
                if let (Some(identifier), Some(workspace)) =
                    (self.identifier.take(), self.workspace.take())
                {
                    if (1..=32).contains(&identifier.len())
                        && identifier.bytes().all(|byte| (32..=126).contains(&byte))
                    {
                        let identity = Identity { identifier, workspace };
                        self.current = Some(identity.clone());
                        self.events.push(Some(identity));
                        self.had_identity = true;
                    }
                }
            },
            Event::Closed => {
                self.closed = true;
                self.identifier = None;
                self.workspace = None;
                self.current = None;
                self.events.push(None);
            },
            _ => (),
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdentityManager {
    manager: KoraToplevelIdentityV1,
}

impl IdentityManager {
    pub fn new(globals: &GlobalList, queue: &QueueHandle<WinitState>) -> Result<Self, BindError> {
        Ok(Self { manager: globals.bind(queue, 1..=1, GlobalData)? })
    }

    pub fn get_identity(
        &self,
        toplevel: &XdgToplevel,
        queue: &QueueHandle<WinitState>,
    ) -> IdentityHandle {
        let state = Arc::new(Mutex::new(IdentityState::default()));
        let handle = self.manager.get_identity(toplevel, queue, state.clone());
        IdentityHandle {
            manager: self.clone(),
            toplevel: toplevel.clone(),
            queue: queue.clone(),
            handle,
            state,
        }
    }
}

#[derive(Debug)]
pub struct IdentityHandle {
    manager: IdentityManager,
    toplevel: XdgToplevel,
    queue: QueueHandle<WinitState>,
    handle: KoraToplevelIdentityHandleV1,
    state: Arc<Mutex<IdentityState>>,
}

impl IdentityHandle {
    pub fn current(&self) -> Option<Identity> {
        self.state.lock().unwrap().current.clone()
    }

    pub fn take_events(&mut self) -> Vec<Option<Identity>> {
        let (events, retry) = {
            let mut state = self.state.lock().unwrap();
            (std::mem::take(&mut state.events), state.closed && state.had_identity)
        };
        // After a real unmap, a fresh handle waits for the next mapping. A lookup
        // that never produced an identity is not retried indefinitely.
        if retry && self.toplevel.is_alive() {
            *self = self.manager.get_identity(&self.toplevel, &self.queue);
        }
        events
    }
}

impl Drop for IdentityHandle {
    fn drop(&mut self) {
        self.handle.destroy();
    }
}

impl Dispatch<KoraToplevelIdentityV1, GlobalData, WinitState> for IdentityManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &KoraToplevelIdentityV1,
        _event: <KoraToplevelIdentityV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _queue: &QueueHandle<WinitState>,
    ) {
    }
}

impl Dispatch<KoraToplevelIdentityHandleV1, Arc<Mutex<IdentityState>>, WinitState>
    for IdentityManager
{
    fn event(
        state: &mut WinitState,
        _proxy: &KoraToplevelIdentityHandleV1,
        event: Event,
        data: &Arc<Mutex<IdentityState>>,
        _conn: &Connection,
        _queue: &QueueHandle<WinitState>,
    ) {
        data.lock().unwrap().event(event);
        state.dispatched_events = true;
    }
}

delegate_dispatch!(WinitState: [KoraToplevelIdentityV1: GlobalData] => IdentityManager);
delegate_dispatch!(WinitState: [KoraToplevelIdentityHandleV1: Arc<Mutex<IdentityState>>] => IdentityManager);

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use wayland_client::protocol::wl_registry::WlRegistry;

    struct Proxies;
    wayland_client::delegate_noop!(Proxies: ignore WlRegistry);
    wayland_client::delegate_noop!(Proxies: ignore XdgToplevel);
    wayland_client::delegate_noop!(Proxies: ignore KoraToplevelIdentityV1);

    fn handle() -> (IdentityHandle, UnixStream, Connection) {
        let (client, server) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let proxies = connection.new_event_queue::<Proxies>();
        let registry = connection.display().get_registry(&proxies.handle(), ());
        let manager = IdentityManager { manager: registry.bind(1, 1, &proxies.handle(), ()) };
        let toplevel = registry.bind(2, 1, &proxies.handle(), ());
        let queue = connection.new_event_queue::<WinitState>();
        (manager.get_identity(&toplevel, &queue.handle()), server, connection)
    }

    fn pair(state: &mut IdentityState, identifier: &str, workspace: &str) {
        state.event(Event::Identifier { identifier: identifier.into() });
        state.event(Event::Workspace { workspace: workspace.into() });
    }

    #[test]
    fn identity_is_atomic_and_machine_workspace_is_explicit() {
        let mut state = IdentityState::default();
        pair(&mut state, "one", "");
        assert!(state.current.is_none());
        assert!(state.events.is_empty());
        state.event(Event::Done);
        let expected = Identity { identifier: "one".into(), workspace: "".into() };
        assert_eq!(state.current, Some(expected.clone()));
        assert_eq!(state.events, vec![Some(expected)]);
        state.event(Event::Done);
        assert_eq!(state.events.len(), 1);
    }

    #[test]
    fn missing_or_invalid_identity_never_becomes_machine_attribution() {
        let mut missing = IdentityState::default();
        missing.event(Event::Identifier { identifier: "one".into() });
        missing.event(Event::Done);
        assert!(missing.current.is_none());
        assert!(missing.events.is_empty());
        for invalid in ["", "identifier\n", "💡", "123456789012345678901234567890123"] {
            let mut state = IdentityState::default();
            pair(&mut state, invalid, "");
            state.event(Event::Done);
            assert!(state.current.is_none());
        }
    }

    #[test]
    fn closed_discards_pending_data_and_revokes_committed_data() {
        for complete in [false, true] {
            let mut state = IdentityState::default();
            pair(&mut state, "one", "project");
            if complete {
                state.event(Event::Done);
                state.events.clear();
            }
            state.event(Event::Closed);
            assert_eq!(state.current, None);
            assert_eq!(state.events, vec![None]);
            assert_eq!(state.had_identity, complete);
            pair(&mut state, "stale", "project");
            state.event(Event::Done);
            state.event(Event::Closed);
            assert_eq!(state.current, None);
            assert_eq!(state.events, vec![None]);
        }
    }

    #[test]
    fn lookup_wire_shape_matches_the_shared_protocol() {
        use protocol::kora_toplevel_identity_v1::*;
        assert_eq!(KoraToplevelIdentityV1::interface().name, "kora_toplevel_identity_v1");
        assert_eq!(REQ_GET_IDENTITY_OPCODE, 1);
        assert_eq!(REQ_GET_IDENTITY_SINCE, 1);
        assert_eq!(REQ_GET_FOREIGN_IDENTITY_OPCODE, 2);
        assert_eq!(KoraToplevelIdentityHandleV1::interface().version, 1);
    }

    #[test]
    fn a_revoked_mapped_identity_gets_a_fresh_handle_for_remapping() {
        let (mut handle, _server, _connection) = handle();
        let old_proxy = handle.handle.clone();
        let old_state = handle.state.clone();
        {
            let mut state = old_state.lock().unwrap();
            pair(&mut state, "old-map", "project");
            state.event(Event::Done);
        }
        assert_eq!(handle.current().unwrap().identifier, "old-map");
        assert_eq!(handle.take_events().len(), 1);
        old_state.lock().unwrap().event(Event::Closed);
        assert_eq!(handle.current(), None);
        assert_eq!(handle.take_events(), vec![None]);
        assert_ne!(handle.handle, old_proxy);
        assert!(!Arc::ptr_eq(&handle.state, &old_state));
        old_state.lock().unwrap().event(Event::Done);
        assert_eq!(handle.current(), None);
        {
            let mut state = handle.state.lock().unwrap();
            pair(&mut state, "new-map", "project");
            state.event(Event::Done);
        }
        assert_eq!(handle.current().unwrap().identifier, "new-map");
    }

    #[test]
    fn an_unavailable_lookup_does_not_retry_forever() {
        let (mut handle, _server, _connection) = handle();
        let original = handle.handle.clone();
        handle.state.lock().unwrap().event(Event::Closed);
        assert_eq!(handle.take_events(), vec![None]);
        for _ in 0..3 {
            assert!(handle.take_events().is_empty());
            assert_eq!(handle.handle, original);
            assert_eq!(handle.current(), None);
        }
    }
}
