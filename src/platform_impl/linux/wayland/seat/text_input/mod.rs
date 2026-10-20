use std::ops::Deref;

use sctk::globals::GlobalData;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};

use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Dispatch};
use sctk::reexports::protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3;
use sctk::reexports::protocols::wp::text_input::zv3::client::zwp_text_input_v3::{
    ChangeCause, ContentHint, ContentPurpose, Event as TextInputEvent, ZwpTextInputV3,
};

use crate::event::{Ime, WindowEvent};
use crate::platform::wayland::ImeSurroundingText;
use crate::platform_impl::wayland;
use crate::platform_impl::wayland::state::WinitState;
use crate::window::ImePurpose;

pub struct TextInputState {
    text_input_manager: ZwpTextInputManagerV3,
}

impl TextInputState {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let text_input_manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { text_input_manager })
    }
}

impl Deref for TextInputState {
    type Target = ZwpTextInputManagerV3;

    fn deref(&self) -> &Self::Target {
        &self.text_input_manager
    }
}

impl Dispatch<ZwpTextInputManagerV3, GlobalData, WinitState> for TextInputState {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZwpTextInputManagerV3,
        _event: <ZwpTextInputManagerV3 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
    }
}

impl Dispatch<ZwpTextInputV3, TextInputData, WinitState> for TextInputState {
    fn event(
        state: &mut WinitState,
        text_input: &ZwpTextInputV3,
        event: <ZwpTextInputV3 as Proxy>::Event,
        data: &TextInputData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        let windows = state.windows.get_mut();
        let mut text_input_data = data.inner.lock().unwrap();
        match event {
            TextInputEvent::Enter { surface } => {
                let window_id = wayland::make_wid(&surface);
                text_input_data.surface = Some(surface);

                let mut window = match windows.get(&window_id) {
                    Some(window) => window.lock().unwrap(),
                    None => return,
                };

                if window.ime_allowed() {
                    window.enable_text_input(text_input);
                    text_input.commit();
                    state.events_sink.push_window_event(WindowEvent::Ime(Ime::Enabled), window_id);
                }

                window.text_input_entered(text_input);
            },
            TextInputEvent::Leave { surface } => {
                text_input_data.surface = None;

                // Always issue a disable.
                text_input.disable();
                text_input.commit();

                let window_id = wayland::make_wid(&surface);

                // XXX this check is essential, because `leave` could have a
                // reference to nil surface...
                let mut window = match windows.get(&window_id) {
                    Some(window) => window.lock().unwrap(),
                    None => return,
                };

                window.text_input_left(text_input);

                state.events_sink.push_window_event(WindowEvent::Ime(Ime::Disabled), window_id);
            },
            TextInputEvent::PreeditString { text, cursor_begin, cursor_end } => {
                let text = text.unwrap_or_default();
                let cursor_begin = usize::try_from(cursor_begin)
                    .ok()
                    .and_then(|idx| text.is_char_boundary(idx).then_some(idx));
                let cursor_end = usize::try_from(cursor_end)
                    .ok()
                    .and_then(|idx| text.is_char_boundary(idx).then_some(idx));

                text_input_data.pending_preedit = Some(Preedit { text, cursor_begin, cursor_end })
            },
            TextInputEvent::CommitString { text } => {
                text_input_data.pending_preedit = None;
                text_input_data.pending_commit = text;
            },
            TextInputEvent::Done { .. } => {
                let window_id = match text_input_data.surface.as_ref() {
                    Some(surface) => wayland::make_wid(surface),
                    None => return,
                };

                // Clear preedit, unless all we'll be doing next is sending a new preedit.
                if text_input_data.pending_commit.is_some()
                    || text_input_data.pending_preedit.is_none()
                {
                    state.events_sink.push_window_event(
                        WindowEvent::Ime(Ime::Preedit(String::new(), None)),
                        window_id,
                    );
                }

                // Send `Commit`.
                if let Some(text) = text_input_data.pending_commit.take() {
                    if let Some(window) = windows.get(&window_id) {
                        window.lock().unwrap().ime_text_committed();
                    }
                    state
                        .events_sink
                        .push_window_event(WindowEvent::Ime(Ime::Commit(text)), window_id);
                }

                // Send preedit.
                if let Some(preedit) = text_input_data.pending_preedit.take() {
                    let cursor_range =
                        preedit.cursor_begin.map(|b| (b, preedit.cursor_end.unwrap_or(b)));

                    state.events_sink.push_window_event(
                        WindowEvent::Ime(Ime::Preedit(preedit.text, cursor_range)),
                        window_id,
                    );
                }
            },
            TextInputEvent::DeleteSurroundingText { .. } => {
                // Not handled.
            },
            _ => {},
        }
    }
}

pub trait ZwpTextInputV3Ext {
    fn set_content_type_by_purpose(&self, purpose: ImePurpose);

    fn set_surrounding(&self, surrounding: &SurroundingText);
}

impl ZwpTextInputV3Ext for ZwpTextInputV3 {
    fn set_content_type_by_purpose(&self, purpose: ImePurpose) {
        let (hint, purpose) = match purpose {
            ImePurpose::Normal => (ContentHint::None, ContentPurpose::Normal),
            ImePurpose::Password => (ContentHint::SensitiveData, ContentPurpose::Password),
            ImePurpose::Terminal => (ContentHint::None, ContentPurpose::Terminal),
        };
        self.set_content_type(hint, purpose);
    }

    fn set_surrounding(&self, surrounding: &SurroundingText) {
        self.set_surrounding_text(
            surrounding.text.clone(),
            surrounding.cursor as i32,
            surrounding.anchor as i32,
        );
    }
}

/// The most bytes of text `set_surrounding_text` may carry.
const SURROUNDING_TEXT_MAX: usize = 4000;

/// A field's surrounding text in the form `set_surrounding_text` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurroundingText {
    text: String,
    cursor: usize,
    anchor: usize,
}

impl SurroundingText {
    /// What a field of `purpose` may tell input methods: never a password field's text.
    pub fn for_purpose(
        purpose: ImePurpose,
        surrounding: Option<&ImeSurroundingText>,
    ) -> Option<Self> {
        surrounding.filter(|_| purpose != ImePurpose::Password).map(Self::new)
    }

    /// `surrounding` with its offsets on character boundaries, cut to at most
    /// [`SURROUNDING_TEXT_MAX`] bytes around the caret and as much of the selection as fits.
    fn new(surrounding: &ImeSurroundingText) -> Self {
        let text = surrounding.text.as_str();
        let floor = |mut i: usize| {
            i = i.min(text.len());
            while !text.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let ceil = |mut i: usize| {
            while !text.is_char_boundary(i) {
                i += 1;
            }
            i
        };
        let cursor = floor(surrounding.cursor);
        let anchor = floor(surrounding.anchor);
        if text.len() <= SURROUNDING_TEXT_MAX {
            return Self { text: text.to_owned(), cursor, anchor };
        }

        let (mut lo, mut hi) = (cursor.min(anchor), cursor.max(anchor));
        if hi - lo > SURROUNDING_TEXT_MAX {
            // Too long a selection: keep the end at the caret.
            if anchor < cursor {
                lo = cursor - SURROUNDING_TEXT_MAX;
            } else {
                hi = cursor + SURROUNDING_TEXT_MAX;
            }
        }
        let spare = SURROUNDING_TEXT_MAX - (hi - lo);
        let after = (spare - (spare / 2).min(lo)).min(text.len() - hi);
        let before = (spare - after).min(lo);
        let start = ceil(lo - before);
        let end = floor(hi + after);

        Self {
            text: text[start..end].to_owned(),
            cursor: cursor - start,
            anchor: anchor.clamp(start, end) - start,
        }
    }
}

/// What a text input needs when its field's surrounding text changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurroundingChange {
    /// Nothing changed.
    Unchanged,
    /// Send the new text, changed for this reason.
    Send(ChangeCause),
    /// The field stopped sharing its text. Only a new `enable` makes the input
    /// method forget what it was told.
    Reenable,
}

impl SurroundingChange {
    /// The change from `sent` to `next`; `committed` when the input method's own
    /// text is what last changed the field.
    pub fn between(
        sent: Option<&SurroundingText>,
        next: Option<&SurroundingText>,
        committed: bool,
    ) -> Self {
        match (sent, next) {
            (sent, next) if sent == next => Self::Unchanged,
            (Some(_), None) => Self::Reenable,
            (None, None) => Self::Unchanged,
            (_, Some(_)) if committed => Self::Send(ChangeCause::InputMethod),
            (_, Some(_)) => Self::Send(ChangeCause::Other),
        }
    }
}

/// The Data associated with the text input.
#[derive(Default)]
pub struct TextInputData {
    inner: std::sync::Mutex<TextInputDataInner>,
}

#[derive(Default)]
pub struct TextInputDataInner {
    /// The `WlSurface` we're performing input to.
    surface: Option<WlSurface>,

    /// The commit to submit on `done`.
    pending_commit: Option<String>,

    /// The preedit to submit on `done`.
    pending_preedit: Option<Preedit>,
}

/// The state of the preedit.
struct Preedit {
    text: String,
    cursor_begin: Option<usize>,
    cursor_end: Option<usize>,
}

delegate_dispatch!(WinitState: [ZwpTextInputManagerV3: GlobalData] => TextInputState);
delegate_dispatch!(WinitState: [ZwpTextInputV3: TextInputData] => TextInputState);

#[cfg(test)]
mod tests {
    use super::*;

    fn fitted(text: &str, cursor: usize, anchor: usize) -> SurroundingText {
        SurroundingText::new(&ImeSurroundingText { text: text.into(), cursor, anchor })
    }

    fn surrounding(text: &str) -> SurroundingText {
        fitted(text, text.len(), text.len())
    }

    #[test]
    fn short_text_is_sent_whole_with_offsets_on_character_boundaries() {
        assert_eq!(fitted("Find", 4, 4), SurroundingText {
            text: "Find".into(),
            cursor: 4,
            anchor: 4
        });
        // Inside "é" (bytes 3..5): both offsets fall back to its start; past the end, to the end.
        assert_eq!(fitted("café", 4, 99), SurroundingText {
            text: "café".into(),
            cursor: 3,
            anchor: 5
        });
    }

    #[test]
    fn long_text_is_cut_around_the_caret_and_selection() {
        let text: String = (0..6000u32)
            .map(|i| if i % 3 == 0 { 'é' } else { char::from(b'a' + (i % 26) as u8) })
            .collect();
        let len = text.len();
        let floor = |i: usize| (0..=i).rev().find(|&i| text.is_char_boundary(i)).unwrap();
        for (cursor, anchor) in [(0, 0), (4000, 4030), (len, len), (6000, 2500), (10, len - 10)] {
            let (cursor, anchor) = (floor(cursor), floor(anchor));
            let fit = fitted(&text, cursor, anchor);
            let kept = fit.text.len();
            assert!((SURROUNDING_TEXT_MAX - 1..=SURROUNDING_TEXT_MAX).contains(&kept), "{kept}");
            let start = cursor - fit.cursor;
            assert_eq!(&text[start..start + kept], fit.text, "{cursor}..{anchor}");
            assert!(fit.text.is_char_boundary(fit.anchor));
            if cursor.abs_diff(anchor) <= SURROUNDING_TEXT_MAX {
                assert_eq!(start + fit.anchor, anchor, "{cursor}..{anchor}: the selection is kept");
            }
        }
    }

    #[test]
    fn caret_context_is_shared_both_sides_when_there_is_room() {
        let text = format!("{}Find{}", "a".repeat(5000), "b".repeat(5000));
        let fit = fitted(&text, 5004, 5004);
        assert_eq!(fit.text.len(), SURROUNDING_TEXT_MAX);
        assert_eq!(fit.cursor, 2000);
        assert_eq!(&fit.text[1996..2000], "Find");
    }

    #[test]
    fn a_password_field_tells_nothing() {
        let text = ImeSurroundingText { text: "hunter2".into(), cursor: 7, anchor: 7 };
        assert_eq!(SurroundingText::for_purpose(ImePurpose::Password, Some(&text)), None);
        assert_eq!(
            SurroundingText::for_purpose(ImePurpose::Normal, Some(&text)),
            Some(surrounding("hunter2"))
        );
        assert_eq!(SurroundingText::for_purpose(ImePurpose::Terminal, None), None);
    }

    #[test]
    fn changes_say_why_and_a_field_that_stops_sharing_reenables() {
        let find = surrounding("Find");
        let found = surrounding("Find a voice");
        let change = SurroundingChange::between;
        assert_eq!(change(Some(&find), Some(&find), true), SurroundingChange::Unchanged);
        assert_eq!(change(None, None, false), SurroundingChange::Unchanged);
        assert_eq!(change(None, Some(&find), false), SurroundingChange::Send(ChangeCause::Other));
        assert_eq!(
            change(Some(&find), Some(&found), true),
            SurroundingChange::Send(ChangeCause::InputMethod)
        );
        assert_eq!(change(Some(&found), None, false), SurroundingChange::Reenable);
    }
}
