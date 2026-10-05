//! A window that publishes its commands and recent items to Kora's Halo, and
//! logs what is picked there. Ctrl+K asks for the Halo's palette.

#[cfg(wayland_platform)]
#[path = "util/fill.rs"]
mod fill;
#[cfg(wayland_platform)]
#[path = "util/tracing.rs"]
mod tracing;

#[cfg(wayland_platform)]
fn main() -> Result<(), impl std::error::Error> {
    use ::tracing::info;
    use winit::application::ApplicationHandler;
    use winit::event::{ElementState, KeyEvent, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::keyboard::{Key, ModifiersState};
    use winit::platform::wayland::{WindowAttributesExtWayland, WindowExtWayland};
    use winit::window::{AppCommand, AppCommandRequest, AppCommands, AppRecent, Window, WindowId};

    #[derive(Default)]
    struct App {
        window: Option<Window>,
        modifiers: ModifiersState,
        grid: bool,
    }

    impl App {
        fn publish(&self) {
            let Some(window) = self.window.as_ref() else { return };
            let command = |id: &str, name: &str, section: &str| AppCommand {
                id: id.into(),
                name: name.into(),
                section: section.into(),
                enabled: true,
                ..AppCommand::default()
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_millis() as u64);
            window.set_app_commands(&AppCommands {
                handles: vec![("copy".into(), true), ("settings".into(), true)],
                commands: vec![
                    AppCommand {
                        keys: "Ctrl+=".into(),
                        icon: "zoom-in".into(),
                        menu: true,
                        bound: true,
                        ..command("example.zoom-in", "Zoom in", "View")
                    },
                    AppCommand {
                        stateful: true,
                        active: self.grid,
                        menu: true,
                        ..command("example.grid", "Show the grid", "View")
                    },
                    command("example.export", "Export…", "File"),
                ],
                recents: vec![
                    AppRecent {
                        id: "notes".into(),
                        label: "Meeting notes".into(),
                        sublabel: "Documents".into(),
                        timestamp_ms: now.saturating_sub(5 * 60_000),
                    },
                    AppRecent {
                        id: "plan".into(),
                        label: "Launch plan".into(),
                        sublabel: "Projects".into(),
                        timestamp_ms: now.saturating_sub(3 * 3_600_000),
                    },
                ],
            });
        }
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let attributes =
                Window::default_attributes().with_title("Example").with_name("example", "");
            self.window = Some(event_loop.create_window(attributes).unwrap());
            self.publish();
        }

        fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
            match event {
                WindowEvent::AppCommand(request) => {
                    info!("picked in the Halo: {request:?}");
                    if request == AppCommandRequest::Invoke("example.grid".into()) {
                        self.grid = !self.grid;
                        self.publish();
                    }
                },
                WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            logical_key: Key::Character(c), state: ElementState::Pressed, ..
                        },
                    ..
                } if c.as_str() == "k" && self.modifiers.control_key() => {
                    if let Some(window) = self.window.as_ref() {
                        window.request_app_palette();
                    }
                },
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::RedrawRequested => {
                    if let Some(window) = self.window.as_ref() {
                        fill::fill_window(window);
                    }
                },
                _ => (),
            }
        }
    }

    tracing::init();
    EventLoop::new().unwrap().run_app(&mut App::default())
}

#[cfg(not(wayland_platform))]
fn main() {
    println!("This example needs Wayland.");
}
