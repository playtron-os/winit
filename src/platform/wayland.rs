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

// Re-export voice mode types for the public API
#[cfg(wayland_platform)]
pub use crate::platform_impl::wayland::types::cosmic_voice_mode::{
    OrbState as VoiceModeOrbState,
    VoiceModeEvent,
};

/// Additional methods on [`ActiveEventLoop`] that are specific to Wayland.
pub trait ActiveEventLoopExtWayland {
    /// True if the [`ActiveEventLoop`] uses Wayland.
    fn is_wayland(&self) -> bool;
}

impl ActiveEventLoopExtWayland for ActiveEventLoop {
    #[inline]
    fn is_wayland(&self) -> bool {
        self.p.is_wayland()
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

    /// Set exclusive mode for this window using the COSMIC protocol.
    ///
    /// When exclusive mode is enabled, all other toplevel windows on the same
    /// output are minimized by the compositor. When disabled, they are restored.
    ///
    /// This is useful for applications that need a clean, focused interface
    /// without distractions from other windows (e.g., AI assistants, system
    /// overlays, presentation modes).
    ///
    /// Returns `true` if the request was sent, `false` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the exclusive mode protocol
    ///
    /// # Arguments
    /// * `exclusive` - `true` to enable exclusive mode, `false` to disable
    fn set_exclusive_mode(&self, exclusive: bool) -> bool;

    /// Check if exclusive mode is currently enabled for this window.
    ///
    /// Returns `true` if exclusive mode is enabled, `false` otherwise.
    fn is_exclusive_mode(&self) -> bool;

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
    fn set_corner_radius(&self, top_left: u32, top_right: u32, bottom_right: u32, bottom_left: u32) -> bool;

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
    fn set_embed_geometry(
        &self,
        embed_id: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool;

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

    /// Register this window as a voice mode receiver.
    ///
    /// When registered, this window will receive voice mode events from the compositor
    /// via `WindowEvent::VoiceMode` when:
    /// - This window is focused and voice mode activates
    /// - This is the default receiver and no other receiver is focused
    ///
    /// Returns `true` if registration was successful, `false` if:
    /// - The window is not a Wayland window
    /// - The compositor doesn't support the voice mode protocol
    ///
    /// # Arguments
    /// * `is_default` - If true, this window becomes the default receiver for when
    ///   no other registered window is focused.
    fn register_voice_mode(&self, is_default: bool) -> bool;

    /// Unregister this window as a voice mode receiver.
    fn unregister_voice_mode(&self) -> bool;

    /// Set the audio level for voice mode visualization.
    ///
    /// # Arguments
    /// * `level` - Audio level from 0-1000, where 0 is silence and 1000 is maximum.
    fn set_voice_audio_level(&self, level: u32) -> bool;

    /// Acknowledge a will_stop event from the compositor.
    ///
    /// This responds to a will_stop event, telling the compositor whether to
    /// freeze the orb (transcription processing) or proceed with hiding.
    ///
    /// # Arguments
    /// * `serial` - The serial from the will_stop event
    /// * `freeze` - If true, freeze the orb in place. If false, proceed with hiding.
    ///
    /// Returns `true` if successful, `false` if this window is not a voice mode receiver.
    fn voice_ack_stop(&self, serial: u32, freeze: bool) -> bool;

    /// Dismiss the frozen voice orb.
    ///
    /// This tells the compositor to hide the orb when transcription completes
    /// without spawning a new window (e.g., empty result or error).
    ///
    /// Returns `true` if successful, `false` if this window is not a voice mode receiver.
    fn voice_dismiss(&self) -> bool;
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
    fn set_embed_geometry(
        &self,
        embed_id: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool {
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
            crate::platform_impl::Window::Wayland(window) => {
                window.set_embed_anchor(
                    embed_id,
                    anchor,
                    margin_top,
                    margin_right,
                    margin_bottom,
                    margin_left,
                    width,
                    height,
                )
            },
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
            crate::platform_impl::Window::Wayland(window) => {
                window.set_embed_corner_radius(embed_id, top_left, top_right, bottom_right, bottom_left)
            },
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
    fn set_exclusive_mode(&self, exclusive: bool) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.set_exclusive_mode(exclusive),
        }
    }

    #[inline]
    fn is_exclusive_mode(&self) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.is_exclusive_mode(),
        }
    }

    #[inline]
    fn set_corner_radius(&self, top_left: u32, top_right: u32, bottom_right: u32, bottom_left: u32) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.set_corner_radius(top_left, top_right, bottom_right, bottom_left),
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
    fn register_voice_mode(&self, is_default: bool) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.register_voice_mode(is_default),
        }
    }

    #[inline]
    fn unregister_voice_mode(&self) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.unregister_voice_mode(),
        }
    }

    #[inline]
    fn set_voice_audio_level(&self, level: u32) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.set_voice_audio_level(level),
        }
    }

    #[inline]
    fn voice_ack_stop(&self, serial: u32, freeze: bool) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.voice_ack_stop(serial, freeze),
        }
    }

    #[inline]
    fn voice_dismiss(&self) -> bool {
        match &self.window {
            #[cfg(x11_platform)]
            crate::platform_impl::Window::X(_) => false,
            #[cfg(wayland_platform)]
            crate::platform_impl::Window::Wayland(window) => window.voice_dismiss(),
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
}

impl WindowAttributesExtWayland for WindowAttributes {
    #[inline]
    fn with_name(mut self, general: impl Into<String>, instance: impl Into<String>) -> Self {
        self.platform_specific.name =
            Some(crate::platform_impl::ApplicationName::new(general.into(), instance.into()));
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
