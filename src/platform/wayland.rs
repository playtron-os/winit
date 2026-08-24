//! # Wayland
//!
//! **Note:** Windows don't appear on Wayland until you draw/present to them.
//!
//! By default, Winit loads system libraries using `dlopen`. This can be
//! disabled by disabling the `"wayland-dlopen"` cargo feature.
//!
//! ## Client-side decorations
//!
//! Winit provides client-side decorations by default, but the behaviour can
//! be controlled with the following feature flags:
//!
//! * `wayland-csd-adwaita` (default).
//! * `wayland-csd-adwaita-crossfont`.
//! * `wayland-csd-adwaita-notitle`.

use std::ffi::c_void;
use std::ptr::NonNull;

use crate::event_loop::{ActiveEventLoop, EventLoop, EventLoopBuilder};
use crate::monitor::MonitorHandle;
use crate::window::{Window, WindowAttributes};

pub use crate::window::Theme;

// Re-export popup types for the public API
#[cfg(wayland_platform)]
pub use crate::platform_impl::wayland::types::xdg_popup::{
    PopupAnchor, PopupEvent, PopupGravity, PopupId, PopupSettings,
};

/// Additional methods on [`ActiveEventLoop`] that are specific to Wayland.
pub trait ActiveEventLoopExtWayland {
    /// True if the [`ActiveEventLoop`] uses Wayland.
    fn is_wayland(&self) -> bool;

    /// Create an xdg_popup surface relative to a parent window.
    ///
    /// This creates a proper Wayland popup that can extend outside the parent window bounds
    /// and receives compositor popup semantics (auto-dismiss, input grab, layer stacking).
    ///
    /// Returns the popup ID on success, or None if:
    /// - The [`ActiveEventLoop`] is not using Wayland
    /// - The parent window doesn't exist
    /// - The compositor doesn't support xdg_popup
    #[cfg(wayland_platform)]
    fn create_popup(&self, settings: PopupSettings) -> Option<PopupId>;

    /// Destroy a popup surface.
    ///
    /// Returns true if the popup was found and destroyed.
    #[cfg(wayland_platform)]
    fn destroy_popup(&self, popup_id: PopupId) -> bool;

    /// Get the wl_surface for a popup (for rendering).
    ///
    /// Returns a raw pointer to the wl_surface that can be used for attaching
    /// graphical content. The popup must have been configured first.
    #[cfg(wayland_platform)]
    fn popup_wl_surface(&self, popup_id: PopupId) -> Option<NonNull<c_void>>;

    /// Get raw handles for a popup surface.
    /// Returns (surface_ptr, display_ptr) for creating a wgpu surface.
    #[cfg(wayland_platform)]
    fn popup_raw_handles(&self, popup_id: PopupId) -> Option<(NonNull<c_void>, NonNull<c_void>)>;

    /// Resize a popup surface.
    ///
    /// Updates the popup's surface size and viewport. The compositor will be
    /// notified via a surface commit. Returns true if the popup was found.
    #[cfg(wayland_platform)]
    fn resize_popup(&self, popup_id: PopupId, width: u32, height: u32) -> bool;

    /// Get pending popup events and clear the queue.
    #[cfg(wayland_platform)]
    fn take_popup_events(&self) -> Vec<PopupEvent>;
}

impl ActiveEventLoopExtWayland for ActiveEventLoop {
    #[inline]
    fn is_wayland(&self) -> bool {
        self.p.is_wayland()
    }

    #[cfg(wayland_platform)]
    fn create_popup(&self, settings: PopupSettings) -> Option<PopupId> {
        match &self.p {
            crate::platform_impl::ActiveEventLoop::Wayland(w) => w.create_popup(settings),
            #[cfg(x11_platform)]
            _ => None,
        }
    }

    #[cfg(wayland_platform)]
    fn destroy_popup(&self, popup_id: PopupId) -> bool {
        match &self.p {
            crate::platform_impl::ActiveEventLoop::Wayland(w) => w.destroy_popup(popup_id),
            #[cfg(x11_platform)]
            _ => false,
        }
    }

    #[cfg(wayland_platform)]
    fn popup_wl_surface(&self, popup_id: PopupId) -> Option<NonNull<c_void>> {
        match &self.p {
            crate::platform_impl::ActiveEventLoop::Wayland(w) => w.popup_wl_surface(popup_id),
            #[cfg(x11_platform)]
            _ => None,
        }
    }

    #[cfg(wayland_platform)]
    fn popup_raw_handles(&self, popup_id: PopupId) -> Option<(NonNull<c_void>, NonNull<c_void>)> {
        match &self.p {
            crate::platform_impl::ActiveEventLoop::Wayland(w) => w.popup_raw_handles(popup_id),
            #[cfg(x11_platform)]
            _ => None,
        }
    }

    #[cfg(wayland_platform)]
    fn resize_popup(&self, popup_id: PopupId, width: u32, height: u32) -> bool {
        match &self.p {
            crate::platform_impl::ActiveEventLoop::Wayland(w) => {
                w.resize_popup(popup_id, width, height)
            },
            #[cfg(x11_platform)]
            _ => false,
        }
    }

    #[cfg(wayland_platform)]
    fn take_popup_events(&self) -> Vec<PopupEvent> {
        match &self.p {
            crate::platform_impl::ActiveEventLoop::Wayland(w) => w.take_popup_events(),
            #[cfg(x11_platform)]
            _ => Vec::new(),
        }
    }
}

/// Additional methods on [`EventLoop`] that are specific to Wayland.
pub trait EventLoopExtWayland {
    /// True if the [`EventLoop`] uses Wayland.
    fn is_wayland(&self) -> bool;
}

impl<T: 'static> EventLoopExtWayland for EventLoop<T> {
    #[inline]
    fn is_wayland(&self) -> bool {
        self.event_loop.is_wayland()
    }
}

/// Additional methods on [`EventLoopBuilder`] that are specific to Wayland.
pub trait EventLoopBuilderExtWayland {
    /// Force using Wayland.
    fn with_wayland(&mut self) -> &mut Self;

    /// Whether to allow the event loop to be created off of the main thread.
    ///
    /// By default, the window is only allowed to be created on the main
    /// thread, to make platform compatibility easier.
    fn with_any_thread(&mut self, any_thread: bool) -> &mut Self;
}

impl<T> EventLoopBuilderExtWayland for EventLoopBuilder<T> {
    #[inline]
    fn with_wayland(&mut self) -> &mut Self {
        self.platform_specific.forced_backend = Some(crate::platform_impl::Backend::Wayland);
        self
    }

    #[inline]
    fn with_any_thread(&mut self, any_thread: bool) -> &mut Self {
        self.platform_specific.any_thread = any_thread;
        self
    }
}

/// Additional methods on [`Window`] that are specific to Wayland.
///
/// [`Window`]: crate::window::Window
pub trait WindowExtWayland {
    /// Returns `xdg_toplevel` of the window or [`None`] if the window is X11 window.
    fn xdg_toplevel(&self) -> Option<NonNull<c_void>>;

    /// Returns `wl_surface` of the window or [`None`] if the window is X11 window.
    ///
    /// The returned pointer is to the wayland-client `wl_surface` object.
    /// This can be used for protocols that need direct access to the surface,
    /// such as surface embedding protocols.
    fn wl_surface(&self) -> Option<NonNull<c_void>>;

    /// Request an animated resize to the target size using the COSMIC protocol.
    ///
    /// This uses the `zcosmic_animated_resize_v1` protocol to request smooth
    /// compositor-driven resize animation. The compositor will send intermediate
    /// configure events for smooth animation.
    ///
    /// Returns `true` if the request was sent, `false` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the animated resize protocol
    ///
    /// # Arguments
    /// * `width` - Target width in logical pixels
    /// * `height` - Target height in logical pixels  
    /// * `duration_ms` - Animation duration in milliseconds
    fn request_animated_resize(&self, width: i32, height: i32, duration_ms: u32) -> bool;

    /// Request an animated resize with explicit position using the COSMIC protocol.
    ///
    /// This uses the compositor's animated_resize protocol to smoothly animate
    /// the window from its current geometry to the target geometry.
    ///
    /// If the window is maximized, the position and size will be stored and used
    /// when the window is restored to normal state.
    ///
    /// Returns `true` if the request was sent, `false` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the animated resize protocol
    ///
    /// # Arguments
    /// * `x` - Target x position in logical pixels
    /// * `y` - Target y position in logical pixels
    /// * `width` - Target width in logical pixels
    /// * `height` - Target height in logical pixels
    /// * `duration_ms` - Animation duration in milliseconds
    fn request_animated_resize_with_position(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        duration_ms: u32,
    ) -> bool;

    /// Set corner radius for this window using the COSMIC protocol.
    ///
    /// Communicates the corner radius hint to the compositor so it can
    /// draw proper blur outlines and apply rounded corners to the window.
    ///
    /// Returns `true` if the request was sent, `false` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the corner radius protocol
    ///
    /// # Arguments
    /// * `top_left` - Top-left corner radius in logical pixels
    /// * `top_right` - Top-right corner radius in logical pixels
    /// * `bottom_right` - Bottom-right corner radius in logical pixels
    /// * `bottom_left` - Bottom-left corner radius in logical pixels
    fn set_corner_radius(
        &self,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) -> bool;

    /// Set the compositor-rendered backdrop color for this window.
    ///
    /// The compositor will render a colored rectangle behind the window content,
    /// using the window's corner radius. RGBA components are in the range 0-255.
    ///
    /// Returns `false` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the backdrop color protocol
    fn set_backdrop_color(&self, r: u32, g: u32, b: u32, a: u32) -> bool;

    /// Embed a toplevel by process ID into this window's surface.
    ///
    /// This uses the `zcosmic_surface_embed_manager_v1` protocol to embed a
    /// foreign toplevel window within this window's surface. The compositor
    /// will monitor for new toplevels from the specified PID and embed the
    /// first matching one.
    ///
    /// Returns an embed ID that can be used to update geometry or remove the
    /// embed, or `None` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the surface embed protocol
    ///
    /// # Arguments
    /// * `pid` - Process ID of the application to embed
    /// * `app_id` - Optional app_id hint for verification (can be empty)
    /// * `x` - X position within this window's surface
    /// * `y` - Y position within this window's surface
    /// * `width` - Width of the embed region
    /// * `height` - Height of the embed region
    /// * `interactive` - Whether input should be routed to the embedded surface
    // Arity follows the protocol request; a struct would obscure it.
    #[allow(clippy::too_many_arguments)]
    fn embed_toplevel_by_pid(
        &self,
        pid: u32,
        app_id: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        interactive: bool,
    ) -> Option<u64>;

    /// Update the geometry of an embedded surface.
    ///
    /// # Arguments
    /// * `embed_id` - The ID returned from `embed_toplevel_by_pid`
    /// * `x` - X position within this window's surface
    /// * `y` - Y position within this window's surface
    /// * `width` - Width of the embed region
    /// * `height` - Height of the embed region
    fn set_embed_geometry(&self, embed_id: u64, x: i32, y: i32, width: i32, height: i32) -> bool;

    /// Set anchor-based positioning for an embedded surface.
    ///
    /// Instead of specifying absolute (x, y) coordinates, this allows positioning
    /// relative to the parent window edges. The geometry is automatically
    /// recalculated by the compositor when the parent window resizes.
    ///
    /// # Arguments
    /// * `embed_id` - The ID returned from `embed_toplevel_by_pid`
    /// * `anchor` - Bitflags indicating which edges to anchor to (0=none, 1=top, 2=bottom, 4=left, 8=right)
    /// * `margin_top` - Margin from top edge
    /// * `margin_right` - Margin from right edge  
    /// * `margin_bottom` - Margin from bottom edge
    /// * `margin_left` - Margin from left edge
    /// * `width` - Width of embed region (0 to stretch between left/right anchors)
    /// * `height` - Height of embed region (0 to stretch between top/bottom anchors)
    // Arity follows the protocol request; a struct would obscure it.
    #[allow(clippy::too_many_arguments)]
    fn set_embed_anchor(
        &self,
        embed_id: u64,
        anchor: u32,
        margin_top: i32,
        margin_right: i32,
        margin_bottom: i32,
        margin_left: i32,
        width: i32,
        height: i32,
    ) -> bool;

    /// Set corner radius for an embedded surface.
    ///
    /// This allows the parent to specify rounded corners that match its own UI.
    /// Each corner can have a different radius. Values are in logical pixels.
    /// A value of 0 means no rounding for that corner.
    ///
    /// # Arguments
    /// * `embed_id` - The ID returned from `embed_toplevel_by_pid`
    /// * `top_left` - Top-left corner radius
    /// * `top_right` - Top-right corner radius
    /// * `bottom_right` - Bottom-right corner radius
    /// * `bottom_left` - Bottom-left corner radius
    fn set_embed_corner_radius(
        &self,
        embed_id: u64,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) -> bool;

    /// Set interactivity for an embedded surface.
    ///
    /// When interactive, pointer/keyboard/touch events within the embed
    /// region will be routed to the embedded toplevel.
    fn set_embed_interactive(&self, embed_id: u64, interactive: bool) -> bool;

    /// Remove an embedded surface.
    fn remove_embed(&self, embed_id: u64) -> bool;

    /// Register this window to receive the device's special key.
    ///
    /// The compositor owns the gesture — the key is usually a modifier it also
    /// needs for its own chords — and sends the resolved meaning as
    /// `WindowEvent::SpecialAction`: a tap asks the window to focus its input,
    /// a hold brackets push-to-talk.
    ///
    /// When `is_default` the window also becomes the fallback receiver, used
    /// whenever no registered surface is focused. Only one fallback exists at a
    /// time; registering a second replaces the first.
    ///
    /// Returns `false` if this is not a Wayland window, or the compositor does
    /// not implement `zcosmic_special_action_v1`.
    fn register_special_action(&self, is_default: bool) -> bool;

    /// Stop receiving the special key.
    fn unregister_special_action(&self) -> bool;

    /// Start a Wayland drag-and-drop operation from this window.
    ///
    /// Creates a `wl_data_source`, offers the given MIME types, and calls
    /// `wl_data_device.start_drag(…)`. The `data` vector should parallel
    /// `mime_types` — entry `i` is the pre-serialized data for
    /// `mime_types[i]`.
    ///
    /// `actions` is a bitfield of supported DnD actions:
    /// `1` = copy, `2` = move, `4` = ask.
    ///
    /// `icon` is optional pre-rendered icon pixel data as
    /// `(width, height, argb_pixels, buffer_scale)` in pre-multiplied ARGB format.
    /// The `buffer_scale` is used for HiDPI support (set to 2 for 2x rendering).
    /// If `None`, a default generic icon is used.
    ///
    /// Returns `(true, mime_types, data)` if the drag was started, or
    /// `(false, mime_types, data)` if it could not be started (e.g. not
    /// Wayland, no pointer serial, protocol unavailable).
    fn start_drag(
        &self,
        mime_types: Vec<String>,
        actions: u32,
        data: Vec<Vec<u8>>,
        icon: Option<(u32, u32, Vec<u8>, i32)>,
    ) -> (bool, Vec<String>, Vec<Vec<u8>>);

    /// Accept a MIME type from the current DnD offer.
    ///
    /// Call this from `DndWindowEvent::Enter` or `Motion` handlers to signal
    /// that you can accept the drag with the specified MIME type.
    /// Pass `None` to reject the current drag.
    ///
    /// This must be called on every `Motion` event to continue accepting.
    fn dnd_accept_mime_type(&self, mime_type: Option<&str>);

    /// Set the accepted DnD actions and preferred action.
    ///
    /// `actions` is a bitfield of acceptable actions (1=copy, 2=move, 4=ask).
    /// `preferred` is the single preferred action from that set.
    fn dnd_set_actions(&self, actions: u32, preferred: u32);

    /// Signal that the destination has finished processing the drop.
    ///
    /// Call this after processing `DndWindowEvent::Drop` to tell the source
    /// the transfer is complete. Required for the drag to finalize properly.
    fn dnd_finish(&self);

    /// Request data from the current DnD offer for the specified MIME type.
    ///
    /// Call this after receiving `DndWindowEvent::Drop` to retrieve the actual
    /// data from the drag source. The data will be delivered via a
    /// `DndWindowEvent::DataReceived` event.
    ///
    /// # Arguments
    /// * `mime_type` - The MIME type to request (must be one of the types
    ///   offered in the `DndWindowEvent::Enter` event).
    fn dnd_request_data(&self, mime_type: &str);

    /// Inhibit (or release) the compositor's global keyboard shortcuts for this
    /// window.
    ///
    /// While inhibited, the compositor delivers **all** key events — including
    /// its own reserved combos such as `Super` — to this window instead of
    /// handling them as global shortcuts. This is what a key-capture /
    /// shortcut-recording UI needs; release it as soon as capture ends.
    ///
    /// No-op on X11 (which already delivers all keys to the focused window) or
    /// if the compositor lacks `zwp_keyboard_shortcuts_inhibit_manager_v1`.
    fn set_keyboard_shortcuts_inhibit(&self, inhibit: bool);
}

impl WindowExtWayland for Window {
    #[inline]
    fn xdg_toplevel(&self) -> Option<NonNull<c_void>> {
        #[allow(clippy::single_match)]
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => None,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.xdg_toplevel(),
        }
    }

    #[inline]
    fn wl_surface(&self) -> Option<NonNull<c_void>> {
        #[allow(clippy::single_match)]
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => None,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.wl_surface_ptr(),
        }
    }

    #[inline]
    fn request_animated_resize(&self, width: i32, height: i32, duration_ms: u32) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.request_animated_resize(width, height, duration_ms)
            },
        }
    }

    #[inline]
    fn request_animated_resize_with_position(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        duration_ms: u32,
    ) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.request_animated_resize_with_position(x, y, width, height, duration_ms)
            },
        }
    }

    #[inline]
    fn embed_toplevel_by_pid(
        &self,
        pid: u32,
        app_id: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        interactive: bool,
    ) -> Option<u64> {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => None,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.embed_toplevel_by_pid(pid, app_id, x, y, width, height, interactive)
            },
        }
    }

    #[inline]
    fn set_embed_geometry(&self, embed_id: u64, x: i32, y: i32, width: i32, height: i32) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.set_embed_geometry(embed_id, x, y, width, height)
            },
        }
    }

    #[inline]
    fn set_embed_anchor(
        &self,
        embed_id: u64,
        anchor: u32,
        margin_top: i32,
        margin_right: i32,
        margin_bottom: i32,
        margin_left: i32,
        width: i32,
        height: i32,
    ) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.set_embed_anchor(
                embed_id,
                anchor,
                margin_top,
                margin_right,
                margin_bottom,
                margin_left,
                width,
                height,
            ),
        }
    }

    #[inline]
    fn set_embed_corner_radius(
        &self,
        embed_id: u64,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.set_embed_corner_radius(
                embed_id,
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            ),
        }
    }

    #[inline]
    fn set_embed_interactive(&self, embed_id: u64, interactive: bool) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.set_embed_interactive(embed_id, interactive)
            },
        }
    }

    #[inline]
    fn remove_embed(&self, embed_id: u64) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.remove_embed(embed_id),
        }
    }

    #[inline]
    fn set_corner_radius(
        &self,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.set_corner_radius(top_left, top_right, bottom_right, bottom_left)
            },
        }
    }

    #[inline]
    fn set_backdrop_color(&self, r: u32, g: u32, b: u32, a: u32) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.set_backdrop_color(r, g, b, a),
        }
    }

    #[inline]
    fn register_special_action(&self, is_default: bool) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.register_special_action(is_default)
            },
        }
    }

    #[inline]
    fn unregister_special_action(&self) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.unregister_special_action(),
        }
    }

    #[inline]
    fn start_drag(
        &self,
        mime_types: Vec<String>,
        actions: u32,
        data: Vec<Vec<u8>>,
        icon: Option<(u32, u32, Vec<u8>, i32)>,
    ) -> (bool, Vec<String>, Vec<Vec<u8>>) {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => (false, mime_types, data),
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.start_drag(mime_types, actions, data, icon)
            },
        }
    }

    #[inline]
    fn dnd_accept_mime_type(&self, mime_type: Option<&str>) {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => {},
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.dnd_accept_mime_type(mime_type);
            },
        }
    }

    #[inline]
    fn dnd_set_actions(&self, actions: u32, preferred: u32) {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => {},
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.dnd_set_actions(actions, preferred);
            },
        }
    }

    #[inline]
    fn dnd_finish(&self) {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => {},
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.dnd_finish();
            },
        }
    }

    #[inline]
    fn dnd_request_data(&self, mime_type: &str) {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => {},
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.dnd_request_data(mime_type);
            },
        }
    }

    #[inline]
    fn set_keyboard_shortcuts_inhibit(&self, inhibit: bool) {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => {},
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => {
                window.set_keyboard_shortcuts_inhibit(inhibit);
            },
        }
    }
}

/// Additional methods on [`WindowAttributes`] that are specific to Wayland.
pub trait WindowAttributesExtWayland {
    /// Build window with the given name.
    ///
    /// The `general` name sets an application ID, which should match the `.desktop`
    /// file distributed with your program. The `instance` is a `no-op`.
    ///
    /// For details about application ID conventions, see the
    /// [Desktop Entry Spec](https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html#desktop-file-id)
    fn with_name(self, general: impl Into<String>, instance: impl Into<String>) -> Self;

    /// Parent this window to a foreign toplevel exported by another client.
    ///
    /// `handle` is an xdg-foreign handle (the bare handle string, without any
    /// `wayland:` prefix) obtained by the other client via `zxdg_exporter_v2` —
    /// for example the `parent_window` an xdg-desktop-portal `FileChooser`
    /// backend receives. The window is imported via `zxdg_importer_v2` and set
    /// as a child of that toplevel, so the compositor places it as a dialog over
    /// the requesting application's window. No-op if the compositor lacks
    /// `zxdg_importer_v2`.
    fn with_wayland_parent(self, handle: impl Into<String>) -> Self;
}

impl WindowAttributesExtWayland for WindowAttributes {
    #[inline]
    fn with_name(mut self, general: impl Into<String>, instance: impl Into<String>) -> Self {
        self.platform_specific.name =
            Some(crate::platform_impl::ApplicationName::new(general.into(), instance.into()));
        self
    }

    #[inline]
    fn with_wayland_parent(mut self, handle: impl Into<String>) -> Self {
        self.platform_specific.wayland_parent = Some(handle.into());
        self
    }
}

/// Additional methods on `MonitorHandle` that are specific to Wayland.
pub trait MonitorHandleExtWayland {
    /// Returns the inner identifier of the monitor.
    fn native_id(&self) -> u32;
}

impl MonitorHandleExtWayland for MonitorHandle {
    #[inline]
    fn native_id(&self) -> u32 {
        self.inner.native_identifier()
    }
}
