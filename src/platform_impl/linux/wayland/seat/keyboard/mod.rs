//! The keyboard input handling.

use std::sync::Mutex;
use std::time::Duration;

use calloop::timer::{TimeoutAction, Timer};
use calloop::{LoopHandle, RegistrationToken};
use tracing::warn;

use sctk::reexports::client::protocol::wl_keyboard::{
    Event as WlKeyboardEvent, KeyState as WlKeyState, KeymapFormat as WlKeymapFormat, WlKeyboard,
};
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use crate::event::{ElementState, WindowEvent};
use crate::keyboard::ModifiersState;

use crate::platform_impl::common::xkb::Context;
use crate::platform_impl::wayland::event_loop::sink::EventSink;
use crate::platform_impl::wayland::state::WinitState;
use crate::platform_impl::wayland::types::xdg_popup::{PopupEvent, PopupId};
use crate::platform_impl::wayland::{self, DeviceId, WindowId};

impl Dispatch<WlKeyboard, KeyboardData, WinitState> for WinitState {
    fn event(
        state: &mut WinitState,
        wl_keyboard: &WlKeyboard,
        event: <WlKeyboard as Proxy>::Event,
        data: &KeyboardData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        let seat_state = match state.seats.get_mut(&data.seat.id()) {
            Some(seat_state) => seat_state,
            None => {
                warn!("Received keyboard event {event:?} without seat");
                return;
            },
        };
        let keyboard_state = match seat_state.keyboard_state.as_mut() {
            Some(keyboard_state) => keyboard_state,
            None => {
                warn!("Received keyboard event {event:?} without keyboard");
                return;
            },
        };

        match event {
            WlKeyboardEvent::Keymap { format, fd, size } => match format {
                WEnum::Value(format) => match format {
                    WlKeymapFormat::NoKeymap => {
                        warn!("non-xkb compatible keymap")
                    },
                    WlKeymapFormat::XkbV1 => {
                        let context = &mut keyboard_state.xkb_context;
                        context.set_keymap_from_fd(fd, size as usize);
                    },
                    _ => unreachable!(),
                },
                WEnum::Unknown(value) => {
                    warn!("unknown keymap format 0x{:x}", value)
                },
            },
            WlKeyboardEvent::Enter { surface, .. } => {
                // A popup given keyboard focus, like a grabbing menu; its surface is no window's.
                let popup = state
                    .popups
                    .iter()
                    .find(|(_, popup)| popup.popup.wl_surface() == &surface)
                    .map(|(id, _)| *id);
                if let Some(id) = popup {
                    keyboard_state.current_repeat = None;
                    if let Some(token) = keyboard_state.repeat_token.take() {
                        keyboard_state.loop_handle.remove(token);
                    }

                    *data.focus.lock().unwrap() = Some(KeyboardFocus::Popup(id));

                    let pending = std::mem::take(&mut seat_state.modifiers_pending)
                        .then_some(seat_state.modifiers);
                    state.popup_events.extend(popup_enter_events(id, pending));
                    // Popup events aren't in the sink the Wayland source checks.
                    state.dispatched_events = true;
                    return;
                }

                let window_id = wayland::make_wid(&surface);

                // Mark the window as focused.
                let was_unfocused = match state.windows.get_mut().get(&window_id) {
                    Some(window) => {
                        let mut window = window.lock().unwrap();
                        let was_unfocused = !window.has_focus();
                        window.add_seat_focus(data.seat.id());
                        was_unfocused
                    },
                    None => return,
                };

                // Drop the repeat, if there were any.
                keyboard_state.current_repeat = None;
                if let Some(token) = keyboard_state.repeat_token.take() {
                    keyboard_state.loop_handle.remove(token);
                }

                *data.focus.lock().unwrap() = Some(KeyboardFocus::Window(window_id));

                // The keyboard focus is considered as general focus.
                if was_unfocused {
                    state.events_sink.push_window_event(WindowEvent::Focused(true), window_id);
                }

                // HACK: this is just for GNOME not fixing their ordering issue of modifiers.
                if std::mem::take(&mut seat_state.modifiers_pending) {
                    state.events_sink.push_window_event(
                        WindowEvent::ModifiersChanged(seat_state.modifiers.into()),
                        window_id,
                    );
                }
            },
            WlKeyboardEvent::Leave { surface, .. } => {
                let window_id = wayland::make_wid(&surface);

                // NOTE: we should drop the repeat regardless whether it was for the present
                // window of for the window which just went gone.
                keyboard_state.current_repeat = None;
                if let Some(token) = keyboard_state.repeat_token.take() {
                    keyboard_state.loop_handle.remove(token);
                }

                // Not looked up by surface: a destroyed popup's leave names one winit dropped.
                if let Some(id) = take_popup_focus(&mut data.focus.lock().unwrap()) {
                    state.popup_events.extend(popup_leave_events(id));
                    state.dispatched_events = true;
                    return;
                }

                // NOTE: The check whether the window exists is essential as we might get a
                // nil surface, regardless of what protocol says.
                let focused = match state.windows.get_mut().get(&window_id) {
                    Some(window) => {
                        let mut window = window.lock().unwrap();
                        window.remove_seat_focus(&data.seat.id());
                        window.has_focus()
                    },
                    None => return,
                };

                // We don't need to update it above, because the next `Enter` will overwrite
                // anyway.
                *data.focus.lock().unwrap() = None;

                if !focused {
                    // Notify that no modifiers are being pressed.
                    state.events_sink.push_window_event(
                        WindowEvent::ModifiersChanged(ModifiersState::empty().into()),
                        window_id,
                    );

                    state.events_sink.push_window_event(WindowEvent::Focused(false), window_id);
                }
            },
            WlKeyboardEvent::Key { key, state: WEnum::Value(WlKeyState::Pressed), .. } => {
                let key = key + 8;

                state.dispatched_events |= key_input(
                    keyboard_state,
                    &mut state.events_sink,
                    &mut state.popup_events,
                    data,
                    key,
                    ElementState::Pressed,
                    false,
                );

                let delay = match keyboard_state.repeat_info {
                    RepeatInfo::Repeat { delay, .. } => delay,
                    RepeatInfo::Disable => return,
                };

                if !keyboard_state.xkb_context.keymap_mut().unwrap().key_repeats(key) {
                    return;
                }

                keyboard_state.current_repeat = Some(key);

                // NOTE terminate ongoing timer and start a new timer.

                if let Some(token) = keyboard_state.repeat_token.take() {
                    keyboard_state.loop_handle.remove(token);
                }

                let timer = Timer::from_duration(delay);
                let wl_keyboard = wl_keyboard.clone();
                keyboard_state.repeat_token = keyboard_state
                    .loop_handle
                    .insert_source(timer, move |_, _, state| {
                        // Required to handle the wakeups from the repeat sources.
                        state.dispatched_events = true;

                        let data = wl_keyboard.data::<KeyboardData>().unwrap();
                        let seat_state = match state.seats.get_mut(&data.seat.id()) {
                            Some(seat_state) => seat_state,
                            None => return TimeoutAction::Drop,
                        };

                        let keyboard_state = match seat_state.keyboard_state.as_mut() {
                            Some(keyboard_state) => keyboard_state,
                            None => return TimeoutAction::Drop,
                        };

                        // NOTE: The removed on event source is batched, but key change to `None`
                        // is instant.
                        let repeat_keycode = match keyboard_state.current_repeat {
                            Some(repeat_keycode) => repeat_keycode,
                            None => return TimeoutAction::Drop,
                        };

                        key_input(
                            keyboard_state,
                            &mut state.events_sink,
                            &mut state.popup_events,
                            data,
                            repeat_keycode,
                            ElementState::Pressed,
                            true,
                        );

                        // NOTE: the gap could change dynamically while repeat is going.
                        match keyboard_state.repeat_info {
                            RepeatInfo::Repeat { gap, .. } => TimeoutAction::ToDuration(gap),
                            RepeatInfo::Disable => TimeoutAction::Drop,
                        }
                    })
                    .ok();
            },
            WlKeyboardEvent::Key { key, state: WEnum::Value(WlKeyState::Released), .. } => {
                let key = key + 8;

                state.dispatched_events |= key_input(
                    keyboard_state,
                    &mut state.events_sink,
                    &mut state.popup_events,
                    data,
                    key,
                    ElementState::Released,
                    false,
                );

                if keyboard_state.repeat_info != RepeatInfo::Disable
                    && keyboard_state.xkb_context.keymap_mut().unwrap().key_repeats(key)
                    && Some(key) == keyboard_state.current_repeat
                {
                    keyboard_state.current_repeat = None;
                    if let Some(token) = keyboard_state.repeat_token.take() {
                        keyboard_state.loop_handle.remove(token);
                    }
                }
            },
            WlKeyboardEvent::Modifiers {
                mods_depressed, mods_latched, mods_locked, group, ..
            } => {
                let xkb_context = &mut keyboard_state.xkb_context;
                let xkb_state = match xkb_context.state_mut() {
                    Some(state) => state,
                    None => return,
                };

                xkb_state.update_modifiers(mods_depressed, mods_latched, mods_locked, 0, 0, group);
                seat_state.modifiers = xkb_state.modifiers().into();

                // HACK: part of the workaround from `WlKeyboardEvent::Enter`.
                let focus = match *data.focus.lock().unwrap() {
                    Some(focus) => focus,
                    None => {
                        seat_state.modifiers_pending = true;
                        return;
                    },
                };

                let event = WindowEvent::ModifiersChanged(seat_state.modifiers.into());
                state.dispatched_events |=
                    focus.push(event, &mut state.events_sink, &mut state.popup_events);
            },
            WlKeyboardEvent::RepeatInfo { rate, delay } => {
                keyboard_state.repeat_info = if rate == 0 {
                    // Stop the repeat once we get a disable event.
                    keyboard_state.current_repeat = None;
                    if let Some(repeat_token) = keyboard_state.repeat_token.take() {
                        keyboard_state.loop_handle.remove(repeat_token);
                    }
                    RepeatInfo::Disable
                } else {
                    let gap = Duration::from_micros(1_000_000 / rate as u64);
                    let delay = Duration::from_millis(delay as u64);
                    RepeatInfo::Repeat { gap, delay }
                };
            },
            _ => unreachable!(),
        }
    }
}

/// The state of the keyboard on the current seat.
#[derive(Debug)]
pub struct KeyboardState {
    /// The underlying WlKeyboard.
    pub keyboard: WlKeyboard,

    /// Loop handle to handle key repeat.
    pub loop_handle: LoopHandle<'static, WinitState>,

    /// The state of the keyboard.
    pub xkb_context: Context,

    /// The information about the repeat rate obtained from the compositor.
    pub repeat_info: RepeatInfo,

    /// The token of the current handle inside the calloop's event loop.
    pub repeat_token: Option<RegistrationToken>,

    /// The current repeat raw key.
    pub current_repeat: Option<u32>,
}

impl KeyboardState {
    pub fn new(keyboard: WlKeyboard, loop_handle: LoopHandle<'static, WinitState>) -> Self {
        Self {
            keyboard,
            loop_handle,
            xkb_context: Context::new().unwrap(),
            repeat_info: RepeatInfo::default(),
            repeat_token: None,
            current_repeat: None,
        }
    }
}

impl Drop for KeyboardState {
    fn drop(&mut self) {
        if self.keyboard.version() >= 3 {
            self.keyboard.release();
        }

        if let Some(token) = self.repeat_token.take() {
            self.loop_handle.remove(token);
        }
    }
}

/// The rate at which a pressed key is repeated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatInfo {
    /// Keys will be repeated at the specified rate and delay.
    Repeat {
        /// The time between the key repeats.
        gap: Duration,

        /// Delay (in milliseconds) between a key press and the start of repetition.
        delay: Duration,
    },

    /// Keys should not be repeated.
    Disable,
}

impl Default for RepeatInfo {
    /// The default repeat rate is 25 keys per second with the delay of 200ms.
    ///
    /// The values are picked based on the default in various compositors and Xorg.
    fn default() -> Self {
        Self::Repeat { gap: Duration::from_millis(40), delay: Duration::from_millis(200) }
    }
}

/// Keyboard user data.
#[derive(Debug)]
pub struct KeyboardData {
    /// The currently focused surface. Could be `None` on bugged compositors, like mutter.
    focus: Mutex<Option<KeyboardFocus>>,

    /// The seat used to create this keyboard.
    seat: WlSeat,
}

impl KeyboardData {
    pub fn new(seat: WlSeat) -> Self {
        Self { focus: Default::default(), seat }
    }
}

/// The surface holding keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyboardFocus {
    Window(WindowId),
    Popup(PopupId),
}

impl KeyboardFocus {
    /// Queues `event` for this surface. Returns whether it went to a popup: the Wayland source
    /// only counts the window sink as dispatched events.
    fn push(
        self,
        event: WindowEvent,
        windows: &mut EventSink,
        popups: &mut Vec<PopupEvent>,
    ) -> bool {
        match self {
            Self::Window(window_id) => {
                windows.push_window_event(event, window_id);
                false
            },
            Self::Popup(id) => {
                popups.push(PopupEvent::Window { id, event });
                true
            },
        }
    }
}

/// Takes the focus if a popup holds it, whatever surface the leave names.
fn take_popup_focus(focus: &mut Option<KeyboardFocus>) -> Option<PopupId> {
    match *focus {
        Some(KeyboardFocus::Popup(id)) => {
            *focus = None;
            Some(id)
        },
        _ => None,
    }
}

/// What a popup reports taking keyboard focus: always focused, since a popup's enter can't
/// repeat, then the modifiers held back while nothing had focus.
fn popup_enter_events(id: PopupId, pending: Option<ModifiersState>) -> Vec<PopupEvent> {
    std::iter::once(WindowEvent::Focused(true))
        .chain(pending.map(|modifiers| WindowEvent::ModifiersChanged(modifiers.into())))
        .map(|event| PopupEvent::Window { id, event })
        .collect()
}

/// What a popup reports losing keyboard focus, in the order a window does.
fn popup_leave_events(id: PopupId) -> [PopupEvent; 2] {
    [WindowEvent::ModifiersChanged(ModifiersState::empty().into()), WindowEvent::Focused(false)]
        .map(|event| PopupEvent::Window { id, event })
}

/// Returns whether the key went to a popup.
fn key_input(
    keyboard_state: &mut KeyboardState,
    event_sink: &mut EventSink,
    popup_events: &mut Vec<PopupEvent>,
    data: &KeyboardData,
    keycode: u32,
    state: ElementState,
    repeat: bool,
) -> bool {
    let focus = match *data.focus.lock().unwrap() {
        Some(focus) => focus,
        None => return false,
    };

    let device_id = crate::event::DeviceId(crate::platform_impl::DeviceId::Wayland(DeviceId));
    if let Some(mut key_context) = keyboard_state.xkb_context.key_context() {
        let event = key_context.process_key_event(keycode, state, repeat);
        let event = WindowEvent::KeyboardInput { device_id, event, is_synthetic: false };
        return focus.push(event, event_sink, popup_events);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        popup_enter_events, popup_leave_events, take_popup_focus, KeyboardFocus, PopupEvent,
        PopupId, WindowId,
    };
    use crate::event::{Event, WindowEvent};
    use crate::keyboard::ModifiersState;
    use crate::platform_impl::wayland::event_loop::sink::EventSink;

    /// Each popup event's popup and window event.
    fn window_events(events: &[PopupEvent]) -> Vec<(PopupId, WindowEvent)> {
        events
            .iter()
            .map(|event| match event {
                PopupEvent::Window { id, event } => (*id, event.clone()),
                other => panic!("expected a window event, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn window_focus_queues_in_the_window_sink() {
        let (mut windows, mut popups) = (EventSink::new(), Vec::new());
        let went_to_popup = KeyboardFocus::Window(WindowId(7)).push(
            WindowEvent::Focused(true),
            &mut windows,
            &mut popups,
        );
        assert!(!went_to_popup);
        assert!(popups.is_empty());
        assert!(matches!(windows.window_events.as_slice(), [Event::WindowEvent {
            event: WindowEvent::Focused(true),
            ..
        }]));
    }

    #[test]
    fn popup_focus_queues_as_a_popup_event() {
        let (mut windows, mut popups) = (EventSink::new(), Vec::new());
        let modifiers = WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into());
        let went_to_popup =
            KeyboardFocus::Popup(PopupId(3)).push(modifiers.clone(), &mut windows, &mut popups);
        assert!(went_to_popup, "flagged, since the Wayland source only checks the window sink");
        assert!(windows.is_empty());
        assert_eq!(window_events(&popups), vec![(PopupId(3), modifiers)]);
    }

    #[test]
    fn leave_takes_only_a_popups_focus() {
        let mut focus = Some(KeyboardFocus::Popup(PopupId(3)));
        assert_eq!(take_popup_focus(&mut focus), Some(PopupId(3)));
        assert_eq!(focus, None);

        let mut focus = Some(KeyboardFocus::Window(WindowId(7)));
        assert_eq!(take_popup_focus(&mut focus), None);
        assert_eq!(focus, Some(KeyboardFocus::Window(WindowId(7))), "the window path clears it");

        let mut focus = None;
        assert_eq!(take_popup_focus(&mut focus), None);
    }

    #[test]
    fn popup_enter_focuses_then_sends_held_modifiers() {
        let id = PopupId(3);
        assert_eq!(window_events(&popup_enter_events(id, None)), vec![(
            id,
            WindowEvent::Focused(true)
        )]);
        assert_eq!(window_events(&popup_enter_events(id, Some(ModifiersState::CONTROL))), vec![
            (id, WindowEvent::Focused(true)),
            (id, WindowEvent::ModifiersChanged(ModifiersState::CONTROL.into())),
        ]);
    }

    #[test]
    fn popup_leave_clears_modifiers_then_focus() {
        let id = PopupId(3);
        assert_eq!(window_events(&popup_leave_events(id)), vec![
            (id, WindowEvent::ModifiersChanged(ModifiersState::empty().into())),
            (id, WindowEvent::Focused(false)),
        ]);
    }
}
