//! Kora app commands: a window publishes its commands and recent items, and
//! the shell (the Halo) invokes them in that window.
//! XML mirrors cosmic-protocols 3a42592a's kora-app-commands-v1.xml.

use std::sync::{Arc, Mutex};

use sctk::globals::GlobalData;
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use wayland_client::globals::{BindError, GlobalList};
use wayland_client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;

use crate::platform_impl::wayland::state::WinitState;
use crate::window::{AppCommandRequest, AppCommands};

pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols::xdg::shell::client::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols::xdg::shell::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("resources/protocols/kora-app-commands-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("resources/protocols/kora-app-commands-v1.xml");
}

use protocol::kora_app_commands_handle_v1::{CommandFlags, Event, KoraAppCommandsHandleV1};
use protocol::kora_app_commands_v1::KoraAppCommandsV1;

/// The ids the shell owns. `handle` takes the first group; nothing may add them as commands.
const STANDARD_VERBS: &[&str] = &[
    "undo", "redo", "cut", "copy", "paste", "selall", "markv", "settings", "neww", "find", "info",
];
const SHELL_VERBS: &[&str] = &[
    "shot",
    "record",
    "closeall",
    "park",
    "fill",
    "fullscreen",
    "movews",
    "closew",
    "minimize",
    "maximize",
    "close",
];
const MAX_COMMANDS: usize = 256;
const MAX_RECENTS: usize = 10;

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn valid_text(text: &str, required: bool) -> bool {
    text.len() <= 256
        && (!required || !text.trim().is_empty())
        && !text.chars().any(char::is_control)
}

/// The catalog with everything the compositor would refuse taken out: a refused
/// value is a protocol error, and a protocol error ends the whole client.
fn sanitized(catalog: &AppCommands) -> AppCommands {
    let mut clean = AppCommands::default();
    for (id, enabled) in &catalog.handles {
        if STANDARD_VERBS.contains(&id.as_str()) {
            clean.handles.push((id.clone(), *enabled));
        } else {
            tracing::warn!("app command: `{id}` is not a standard verb, so it cannot be handled");
        }
    }
    for command in &catalog.commands {
        let ok = valid_id(&command.id)
            && command.id.contains('.')
            && !STANDARD_VERBS.contains(&command.id.as_str())
            && !SHELL_VERBS.contains(&command.id.as_str())
            && valid_text(&command.name, true)
            && valid_text(&command.keys, false)
            && valid_text(&command.section, false)
            && valid_text(&command.icon, false)
            && !command.icon.contains(['/', '\\'])
            && !clean.commands.iter().any(|c: &crate::window::AppCommand| c.id == command.id);
        if !ok {
            tracing::warn!("app command: `{}` breaks the catalog contract; dropped", command.id);
        } else if clean.commands.len() < MAX_COMMANDS {
            clean.commands.push(command.clone());
        }
    }
    let mut recents: Vec<_> = catalog
        .recents
        .iter()
        .filter(|recent| {
            valid_id(&recent.id)
                && valid_text(&recent.label, true)
                && valid_text(&recent.sublabel, false)
        })
        .cloned()
        .collect();
    // The newest ten, declaration order breaking ties, as the shell keeps them.
    recents.sort_by_key(|recent| std::cmp::Reverse(recent.timestamp_ms));
    recents.dedup_by(|a, b| a.id == b.id);
    recents.truncate(MAX_RECENTS);
    clean.recents = recents;
    clean
}

#[derive(Debug, Default)]
pub struct HandleState {
    /// The generation of the catalog last committed; invocations naming another are stale.
    committed: Option<u32>,
    events: Vec<AppCommandRequest>,
    closed: bool,
}

impl HandleState {
    fn event(&mut self, event: Event) {
        if self.closed {
            return;
        }
        match event {
            Event::Invoke { id, generation, .. } if self.committed == Some(generation) => {
                self.events.push(AppCommandRequest::Invoke(id));
            },
            Event::RecentSelected { id, generation, .. } if self.committed == Some(generation) => {
                self.events.push(AppCommandRequest::OpenRecent(id));
            },
            Event::Closed => self.closed = true,
            _ => (),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppCommandsManager {
    manager: KoraAppCommandsV1,
}

impl AppCommandsManager {
    /// Bind the global; absent on compositors without a Halo to publish to.
    pub fn new(globals: &GlobalList, queue: &QueueHandle<WinitState>) -> Result<Self, BindError> {
        Ok(Self { manager: globals.bind(queue, 1..=1, GlobalData)? })
    }

    pub fn get_commands(
        &self,
        toplevel: &XdgToplevel,
        queue: &QueueHandle<WinitState>,
    ) -> AppCommandsHandle {
        let state = Arc::new(Mutex::new(HandleState::default()));
        let handle = self.manager.get_commands(toplevel, queue, state.clone());
        AppCommandsHandle { handle, state, generation: 0 }
    }
}

#[derive(Debug)]
pub struct AppCommandsHandle {
    handle: KoraAppCommandsHandleV1,
    state: Arc<Mutex<HandleState>>,
    generation: u32,
}

impl AppCommandsHandle {
    /// Replace the published catalog with `catalog`, atomically.
    pub fn publish(&mut self, catalog: &AppCommands) {
        if self.state.lock().unwrap().closed {
            return;
        }
        let catalog = sanitized(catalog);
        let handle = &self.handle;
        handle.clear();
        for (id, enabled) in &catalog.handles {
            handle.handle(id.clone(), u32::from(*enabled));
        }
        for command in &catalog.commands {
            let mut flags = CommandFlags::empty();
            flags.set(CommandFlags::Menu, command.menu);
            flags.set(CommandFlags::Stateful, command.stateful);
            flags.set(CommandFlags::Bound, command.bound);
            handle.add_command(
                command.id.clone(),
                command.name.clone(),
                command.keys.clone(),
                command.section.clone(),
                command.icon.clone(),
                flags,
            );
            if !command.enabled {
                handle.set_enabled(command.id.clone(), 0);
            }
            if command.stateful && command.active {
                handle.set_state(command.id.clone(), 1);
            }
        }
        for recent in &catalog.recents {
            handle.add_recent(
                recent.id.clone(),
                recent.label.clone(),
                recent.sublabel.clone(),
                (recent.timestamp_ms >> 32) as u32,
                recent.timestamp_ms as u32,
            );
        }
        self.generation = self.generation.wrapping_add(1);
        handle.done(self.generation);
        self.state.lock().unwrap().committed = Some(self.generation);
    }

    /// Ask the shell for this window's command palette, for input `serial` on `seat`.
    pub fn request_palette(&self, seat: &WlSeat, serial: u32) {
        if !self.state.lock().unwrap().closed {
            self.handle.request_palette(seat, serial);
        }
    }

    pub fn take_events(&mut self) -> Vec<AppCommandRequest> {
        std::mem::take(&mut self.state.lock().unwrap().events)
    }
}

impl Drop for AppCommandsHandle {
    fn drop(&mut self) {
        self.handle.destroy();
    }
}

impl Dispatch<KoraAppCommandsV1, GlobalData, WinitState> for AppCommandsManager {
    fn event(
        _state: &mut WinitState,
        _proxy: &KoraAppCommandsV1,
        _event: <KoraAppCommandsV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _queue: &QueueHandle<WinitState>,
    ) {
    }
}

impl Dispatch<KoraAppCommandsHandleV1, Arc<Mutex<HandleState>>, WinitState> for AppCommandsManager {
    fn event(
        state: &mut WinitState,
        _proxy: &KoraAppCommandsHandleV1,
        event: Event,
        data: &Arc<Mutex<HandleState>>,
        _conn: &Connection,
        _queue: &QueueHandle<WinitState>,
    ) {
        data.lock().unwrap().event(event);
        state.dispatched_events = true;
    }
}

delegate_dispatch!(WinitState: [KoraAppCommandsV1: GlobalData] => AppCommandsManager);
delegate_dispatch!(WinitState: [KoraAppCommandsHandleV1: Arc<Mutex<HandleState>>] => AppCommandsManager);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::{AppCommand, AppRecent};

    fn command(id: &str) -> AppCommand {
        AppCommand { id: id.into(), name: "Name".into(), ..AppCommand::default() }
    }

    fn recent(id: &str, at: u64) -> AppRecent {
        AppRecent { id: id.into(), label: id.into(), timestamp_ms: at, ..AppRecent::default() }
    }

    /// The compositor ends a client that sends a refused value, so nothing refused is sent.
    #[test]
    fn a_catalog_the_shell_would_refuse_is_cleaned_before_it_is_sent() {
        let catalog = AppCommands {
            handles: vec![("copy".into(), true), ("term.copy".into(), true)],
            commands: vec![
                command("slate.zoom-in"),
                command("copy"),
                command("close"),
                command("nodot"),
                command("bad/id.x"),
                AppCommand { name: "  ".into(), ..command("slate.blank") },
                AppCommand { icon: "/etc/icon.svg".into(), ..command("slate.path") },
                command("slate.zoom-in"),
            ],
            recents: vec![recent("a", 1), recent("bad id", 2), recent("b", 3)],
        };
        let clean = sanitized(&catalog);
        assert_eq!(clean.handles, vec![("copy".to_owned(), true)]);
        let ids: Vec<_> = clean.commands.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["slate.zoom-in"]);
        let recents: Vec<_> = clean.recents.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(recents, ["b", "a"]);
    }

    #[test]
    fn only_the_newest_ten_recents_are_kept() {
        let catalog = AppCommands {
            recents: (0..14).map(|at| recent(&format!("r{at}"), at)).collect(),
            ..AppCommands::default()
        };
        let clean = sanitized(&catalog);
        assert_eq!(clean.recents.len(), 10);
        assert_eq!(clean.recents[0].id, "r13");
        assert_eq!(clean.recents[9].id, "r4");
    }

    /// A selection made from a catalog this window no longer publishes is dropped.
    #[test]
    fn invocations_from_an_older_catalog_are_ignored() {
        let mut state = HandleState { committed: Some(2), ..HandleState::default() };
        state.event(Event::Invoke { id: "slate.zoom-in".into(), generation: 1, serial: 7 });
        state.event(Event::Invoke { id: "slate.zoom-in".into(), generation: 2, serial: 8 });
        state.event(Event::RecentSelected { id: "doc".into(), generation: 2, serial: 9 });
        assert_eq!(state.events, vec![
            AppCommandRequest::Invoke("slate.zoom-in".into()),
            AppCommandRequest::OpenRecent("doc".into()),
        ]);
        state.event(Event::Closed);
        state.event(Event::Invoke { id: "slate.zoom-in".into(), generation: 2, serial: 10 });
        assert_eq!(state.events.len(), 2);
    }

    #[test]
    fn wire_shape_matches_the_shared_protocol() {
        use protocol::kora_app_commands_handle_v1::*;
        assert_eq!(KoraAppCommandsV1::interface().name, "kora_app_commands_v1");
        assert_eq!(KoraAppCommandsHandleV1::interface().name, "kora_app_commands_handle_v1");
        assert_eq!(REQ_DONE_OPCODE, 7);
        assert_eq!(REQ_REQUEST_PALETTE_OPCODE, 8);
        assert_eq!(EVT_INVOKE_OPCODE, 0);
        assert_eq!(EVT_CLOSED_OPCODE, 2);
    }
}
