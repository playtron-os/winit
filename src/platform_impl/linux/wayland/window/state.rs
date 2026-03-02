//! The state of the window, which is shared with the event-loop.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use ahash::HashSet;
use tracing::{info, warn};

use sctk::reexports::client::backend::ObjectId;
use sctk::reexports::client::protocol::wl_seat::WlSeat;
use sctk::reexports::client::protocol::wl_shm::WlShm;
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{Connection, Proxy, QueueHandle};
use sctk::reexports::csd_frame::{
    DecorationsFrame, FrameAction, FrameClick, ResizeEdge, WindowState as XdgWindowState,
};
use sctk::reexports::protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use sctk::reexports::protocols::wp::text_input::zv3::client::zwp_text_input_v3::ZwpTextInputV3;
use sctk::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use sctk::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge as XdgResizeEdge;

use sctk::compositor::{CompositorState, Region, SurfaceData, SurfaceDataExt};
use sctk::seat::pointer::{PointerDataExt, ThemedPointer};
use sctk::shell::xdg::window::{DecorationMode, Window, WindowConfigure};
use sctk::shell::xdg::XdgSurface;
use sctk::shell::WaylandSurface;
use sctk::shm::slot::SlotPool;
use sctk::shm::Shm;
use sctk::subcompositor::SubcompositorState;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur::OrgKdeKwinBlur;

use crate::cursor::CustomCursor as RootCustomCursor;
use crate::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize, Size};
use crate::error::{ExternalError, NotSupportedError};
use crate::platform_impl::wayland::logical_to_physical_rounded;
use crate::platform_impl::wayland::types::cosmic_animated_resize::{
    AnimatedResizeController, CosmicAnimatedResizeManager,
};
use crate::platform_impl::wayland::types::cosmic_backdrop_color::{
    BackdropColorController, CosmicBackdropColorManager,
};
use crate::platform_impl::wayland::types::cosmic_corner_radius::{
    CornerRadiusController, CosmicCornerRadiusManager,
};
use crate::platform_impl::wayland::types::cosmic_exclusive_mode::{
    CosmicExclusiveModeManager, ExclusiveModeController,
};
use crate::platform_impl::wayland::types::cosmic_surface_embed::{
    CosmicSurfaceEmbedManager, EmbeddedSurface,
};
use crate::platform_impl::wayland::types::cosmic_voice_mode::{
    CosmicVoiceModeManager, VoiceModeReceiver,
};
use crate::platform_impl::wayland::types::cursor::{CustomCursor, SelectedCursor};
use crate::platform_impl::wayland::types::kwin_blur::KWinBlurManager;
use crate::platform_impl::wayland::types::wayland_dnd::{SharedDndOfferState, WaylandDndManager};
use crate::platform_impl::{PlatformCustomCursor, WindowId};
use crate::window::{CursorGrabMode, CursorIcon, ImePurpose, ResizeDirection, Theme};

use crate::platform_impl::wayland::seat::{
    PointerConstraintsState, WinitPointerData, WinitPointerDataExt, ZwpTextInputV3Ext,
};
use crate::platform_impl::wayland::state::{WindowCompositorUpdate, WinitState};

#[cfg(feature = "sctk-adwaita")]
pub type WinitFrame = sctk_adwaita::AdwaitaFrame<WinitState>;
#[cfg(not(feature = "sctk-adwaita"))]
pub type WinitFrame = sctk::shell::xdg::fallback_frame::FallbackFrame<WinitState>;

// Minimum window inner size.
const MIN_WINDOW_SIZE: LogicalSize<u32> = LogicalSize::new(2, 1);

/// The state of the window which is being updated from the [`WinitState`].
pub struct WindowState {
    /// The connection to Wayland server.
    pub connection: Connection,

    /// The `Shm` to set cursor.
    pub shm: WlShm,

    // A shared pool where to allocate custom cursors.
    custom_cursor_pool: Arc<Mutex<SlotPool>>,

    /// The last received configure.
    pub last_configure: Option<WindowConfigure>,

    /// The pointers observed on the window.
    pub pointers: Vec<Weak<ThemedPointer<WinitPointerData>>>,

    selected_cursor: SelectedCursor,

    /// Whether the cursor is visible.
    pub cursor_visible: bool,

    /// Pointer constraints to lock/confine pointer.
    pub pointer_constraints: Option<Arc<PointerConstraintsState>>,

    /// Queue handle.
    pub queue_handle: QueueHandle<WinitState>,

    /// Theme variant.
    theme: Option<Theme>,

    /// The current window title.
    title: String,

    /// Whether the frame is resizable.
    resizable: bool,

    // NOTE: we can't use simple counter, since it's racy when seat getting destroyed and new
    // is created, since add/removed stuff could be delivered a bit out of order.
    /// Seats that has keyboard focus on that window.
    seat_focus: HashSet<ObjectId>,

    /// The scale factor of the window.
    scale_factor: f64,

    /// Whether the window is transparent.
    transparent: bool,

    /// The state of the compositor to create WlRegions.
    compositor: Arc<CompositorState>,

    /// The current cursor grabbing mode.
    cursor_grab_mode: GrabState,

    /// Whether the IME input is allowed for that window.
    ime_allowed: bool,

    /// The current IME purpose.
    ime_purpose: ImePurpose,

    /// The text inputs observed on the window.
    text_inputs: Vec<ZwpTextInputV3>,

    /// The inner size of the window, as in without client side decorations.
    size: LogicalSize<u32>,

    /// Whether the CSD fail to create, so we don't try to create them on each iteration.
    csd_fails: bool,

    /// Whether we should decorate the frame.
    decorate: bool,

    /// Min size.
    min_inner_size: LogicalSize<u32>,
    max_inner_size: Option<LogicalSize<u32>>,

    /// The size of the window when no states were applied to it. The primary use for it
    /// is to fallback to original window size, before it was maximized, if the compositor
    /// sends `None` for the new size in the configure.
    stateless_size: LogicalSize<u32>,

    /// Initial window size provided by the user. Removed on the first
    /// configure.
    initial_size: Option<Size>,

    /// The state of the frame callback.
    frame_callback_state: FrameCallbackState,

    viewport: Option<WpViewport>,
    fractional_scale: Option<WpFractionalScaleV1>,
    blur: Option<OrgKdeKwinBlur>,
    blur_manager: Option<KWinBlurManager>,

    /// COSMIC animated resize manager.
    animated_resize_manager: Option<CosmicAnimatedResizeManager>,
    /// Active animated resize controller for this window.
    animated_resize_controller: Option<AnimatedResizeController>,

    /// COSMIC corner radius manager.
    corner_radius_manager: Option<CosmicCornerRadiusManager>,
    /// Active corner radius controller for this window.
    corner_radius_controller: Option<CornerRadiusController>,

    /// COSMIC backdrop color manager.
    backdrop_color_manager: Option<CosmicBackdropColorManager>,
    /// Active backdrop color controller for this window.
    backdrop_color_controller: Option<BackdropColorController>,

    /// COSMIC exclusive mode manager.
    exclusive_mode_manager: Option<CosmicExclusiveModeManager>,
    /// Active exclusive mode controller for this window.
    exclusive_mode_controller: Option<ExclusiveModeController>,

    /// COSMIC surface embed manager.
    surface_embed_manager: Option<CosmicSurfaceEmbedManager>,
    /// Active embedded surfaces in this window (keyed by a client-provided ID).
    embedded_surfaces: std::collections::HashMap<u64, EmbeddedSurface>,
    /// Next embed ID for tracking.
    next_embed_id: u64,

    /// COSMIC voice mode manager.
    voice_mode_manager: Option<CosmicVoiceModeManager>,
    /// Active voice mode receiver for this window.
    voice_mode_receiver: Option<VoiceModeReceiver>,

    /// Wayland DnD manager (cloned from WinitState).
    dnd_manager: Option<WaylandDndManager>,
    /// Per-seat data devices for DnD (cloned from WinitState).
    dnd_data_devices: Vec<sctk::reexports::client::protocol::wl_data_device::WlDataDevice>,
    /// Shared DnD offer state (shared with WinitState).
    dnd_shared_offer: std::sync::Arc<std::sync::Mutex<SharedDndOfferState>>,
    /// The icon surface + backing buffer for the current drag.
    /// Both must stay alive while the drag is active.
    dnd_icon:
        Option<(sctk::reexports::client::protocol::wl_surface::WlSurface, sctk::shm::slot::Buffer)>,

    /// Whether the client side decorations have pending move operations.
    ///
    /// The value is the serial of the event triggered moved.
    has_pending_move: Option<u32>,

    /// The underlying SCTK window.
    pub window: Window,

    // NOTE: The spec says that destroying parent(`window` in our case), will unmap the
    // subsurfaces. Thus to achieve atomic unmap of the client, drop the decorations
    // frame after the `window` is dropped. To achieve that we rely on rust's struct
    // field drop order guarantees.
    /// The window frame, which is created from the configure request.
    frame: Option<WinitFrame>,
}

impl WindowState {
    /// Create new window state.
    pub fn new(
        connection: Connection,
        queue_handle: &QueueHandle<WinitState>,
        winit_state: &WinitState,
        initial_size: Size,
        window: Window,
        theme: Option<Theme>,
    ) -> Self {
        let compositor = winit_state.compositor_state.clone();
        let pointer_constraints = winit_state.pointer_constraints.clone();
        let viewport = winit_state
            .viewporter_state
            .as_ref()
            .map(|state| state.get_viewport(window.wl_surface(), queue_handle));
        let fractional_scale = winit_state
            .fractional_scaling_manager
            .as_ref()
            .map(|fsm| fsm.fractional_scaling(window.wl_surface(), queue_handle));

        Self {
            blur: None,
            blur_manager: winit_state.kwin_blur_manager.clone(),
            animated_resize_manager: winit_state.animated_resize_manager.clone(),
            animated_resize_controller: None,
            corner_radius_manager: winit_state.corner_radius_manager.clone(),
            corner_radius_controller: None,
            backdrop_color_manager: winit_state.backdrop_color_manager.clone(),
            backdrop_color_controller: None,
            exclusive_mode_manager: winit_state.exclusive_mode_manager.clone(),
            exclusive_mode_controller: None,
            surface_embed_manager: winit_state.surface_embed_manager.clone(),
            embedded_surfaces: std::collections::HashMap::new(),
            next_embed_id: 1,
            voice_mode_manager: winit_state.voice_mode_manager.clone(),
            voice_mode_receiver: None,
            dnd_manager: winit_state.dnd_manager.clone(),
            dnd_data_devices: winit_state.dnd_data_devices.clone(),
            dnd_shared_offer: winit_state.dnd_session.shared_offer.clone(),
            dnd_icon: None,
            compositor,
            connection,
            csd_fails: false,
            cursor_grab_mode: GrabState::new(),
            selected_cursor: Default::default(),
            cursor_visible: true,
            decorate: true,
            fractional_scale,
            frame: None,
            frame_callback_state: FrameCallbackState::None,
            seat_focus: Default::default(),
            has_pending_move: None,
            ime_allowed: false,
            ime_purpose: ImePurpose::Normal,
            last_configure: None,
            max_inner_size: None,
            min_inner_size: MIN_WINDOW_SIZE,
            pointer_constraints,
            pointers: Default::default(),
            queue_handle: queue_handle.clone(),
            resizable: true,
            scale_factor: 1.,
            shm: winit_state.shm.wl_shm().clone(),
            custom_cursor_pool: winit_state.custom_cursor_pool.clone(),
            size: initial_size.to_logical(1.),
            stateless_size: initial_size.to_logical(1.),
            initial_size: Some(initial_size),
            text_inputs: Vec::new(),
            theme,
            title: String::default(),
            transparent: false,
            viewport,
            window,
        }
    }

    /// Apply closure on the given pointer.
    fn apply_on_pointer<F: FnMut(&ThemedPointer<WinitPointerData>, &WinitPointerData)>(
        &self,
        mut callback: F,
    ) {
        self.pointers.iter().filter_map(Weak::upgrade).for_each(|pointer| {
            let data = pointer.pointer().winit_data();
            callback(pointer.as_ref(), data);
        })
    }

    /// Get the current state of the frame callback.
    pub fn frame_callback_state(&self) -> FrameCallbackState {
        self.frame_callback_state
    }

    /// The frame callback was received, but not yet sent to the user.
    pub fn frame_callback_received(&mut self) {
        self.frame_callback_state = FrameCallbackState::Received;
    }

    /// Reset the frame callbacks state.
    pub fn frame_callback_reset(&mut self) {
        self.frame_callback_state = FrameCallbackState::None;
    }

    /// Request a frame callback if we don't have one for this window in flight.
    pub fn request_frame_callback(&mut self) {
        let surface = self.window.wl_surface();
        match self.frame_callback_state {
            FrameCallbackState::None | FrameCallbackState::Received => {
                self.frame_callback_state = FrameCallbackState::Requested;
                surface.frame(&self.queue_handle, surface.clone());
            },
            FrameCallbackState::Requested => (),
        }
    }

    pub fn configure(
        &mut self,
        configure: WindowConfigure,
        shm: &Shm,
        subcompositor: &Option<Arc<SubcompositorState>>,
    ) -> bool {
        // NOTE: when using fractional scaling or wl_compositor@v6 the scaling
        // should be delivered before the first configure, thus apply it to
        // properly scale the physical sizes provided by the users.
        if let Some(initial_size) = self.initial_size.take() {
            self.size = initial_size.to_logical(self.scale_factor());
            self.stateless_size = self.size;
        }

        if let Some(subcompositor) = subcompositor.as_ref().filter(|_| {
            configure.decoration_mode == DecorationMode::Client
                && self.frame.is_none()
                && !self.csd_fails
        }) {
            match WinitFrame::new(
                &self.window,
                shm,
                #[cfg(feature = "sctk-adwaita")]
                self.compositor.clone(),
                subcompositor.clone(),
                self.queue_handle.clone(),
                #[cfg(feature = "sctk-adwaita")]
                into_sctk_adwaita_config(self.theme),
            ) {
                Ok(mut frame) => {
                    frame.set_title(&self.title);
                    frame.set_scaling_factor(self.scale_factor);
                    // Hide the frame if we were asked to not decorate.
                    frame.set_hidden(!self.decorate);
                    self.frame = Some(frame);
                },
                Err(err) => {
                    warn!("Failed to create client side decorations frame: {err}");
                    self.csd_fails = true;
                },
            }
        } else if configure.decoration_mode == DecorationMode::Server {
            // Drop the frame for server side decorations to save resources.
            self.frame = None;
        }

        let stateless = Self::is_stateless(&configure);

        tracing::trace!(
            "configure: stateless={}, configure.new_size={:?}, current_size={:?}, stateless_size={:?}",
            stateless,
            configure.new_size,
            self.size,
            self.stateless_size
        );

        let (mut new_size, constrain) = if let Some(frame) = self.frame.as_mut() {
            // Configure the window states.
            frame.update_state(configure.state);

            match configure.new_size {
                (Some(width), Some(height)) => {
                    let (width, height) = frame.subtract_borders(width, height);
                    let width = width.map(|w| w.get()).unwrap_or(1);
                    let height = height.map(|h| h.get()).unwrap_or(1);
                    ((width, height).into(), false)
                },
                (..) if stateless => (self.stateless_size, true),
                _ => (self.size, true),
            }
        } else {
            match configure.new_size {
                (Some(width), Some(height)) => ((width.get(), height.get()).into(), false),
                _ if stateless => (self.stateless_size, true),
                _ => (self.size, true),
            }
        };

        tracing::debug!("configure: chosen new_size={:?}, constrain={}", new_size, constrain);

        // Apply configure bounds only when compositor let the user decide what size to pick.
        if constrain {
            let bounds = self.inner_size_bounds(&configure);
            new_size.width =
                bounds.0.map(|bound_w| new_size.width.min(bound_w.get())).unwrap_or(new_size.width);
            new_size.height = bounds
                .1
                .map(|bound_h| new_size.height.min(bound_h.get()))
                .unwrap_or(new_size.height);
        }

        let new_state = configure.state;
        let old_state = self.last_configure.as_ref().map(|configure| configure.state);

        let state_change_requires_resize = old_state
            .map(|old_state| {
                !old_state
                    .symmetric_difference(new_state)
                    .difference(XdgWindowState::ACTIVATED | XdgWindowState::SUSPENDED)
                    .is_empty()
            })
            // NOTE: `None` is present for the initial configure, thus we must always resize.
            .unwrap_or(true);

        // NOTE: Set the configure before doing a resize, since we query it during it.
        self.last_configure = Some(configure);

        if state_change_requires_resize || new_size != self.inner_size() {
            self.resize(new_size);
            true
        } else {
            false
        }
    }

    /// Compute the bounds for the inner size of the surface.
    fn inner_size_bounds(
        &self,
        configure: &WindowConfigure,
    ) -> (Option<NonZeroU32>, Option<NonZeroU32>) {
        let configure_bounds = match configure.suggested_bounds {
            Some((width, height)) => (NonZeroU32::new(width), NonZeroU32::new(height)),
            None => (None, None),
        };

        if let Some(frame) = self.frame.as_ref() {
            let (width, height) = frame.subtract_borders(
                configure_bounds.0.unwrap_or(NonZeroU32::new(1).unwrap()),
                configure_bounds.1.unwrap_or(NonZeroU32::new(1).unwrap()),
            );
            (configure_bounds.0.and(width), configure_bounds.1.and(height))
        } else {
            configure_bounds
        }
    }

    #[inline]
    fn is_stateless(configure: &WindowConfigure) -> bool {
        !(configure.is_maximized() || configure.is_fullscreen() || configure.is_tiled())
    }

    /// Start interacting drag resize.
    pub fn drag_resize_window(&self, direction: ResizeDirection) -> Result<(), ExternalError> {
        let xdg_toplevel = self.window.xdg_toplevel();

        // TODO(kchibisov) handle touch serials.
        self.apply_on_pointer(|_, data| {
            let serial = data.latest_button_serial();
            let seat = data.seat();
            xdg_toplevel.resize(seat, serial, direction.into());
        });

        Ok(())
    }

    /// Start the window drag.
    pub fn drag_window(&self) -> Result<(), ExternalError> {
        let xdg_toplevel = self.window.xdg_toplevel();
        // TODO(kchibisov) handle touch serials.
        self.apply_on_pointer(|_, data| {
            let serial = data.latest_button_serial();
            let seat = data.seat();
            xdg_toplevel._move(seat, serial);
        });

        Ok(())
    }

    /// Tells whether the window should be closed.
    #[allow(clippy::too_many_arguments)]
    pub fn frame_click(
        &mut self,
        click: FrameClick,
        pressed: bool,
        seat: &WlSeat,
        serial: u32,
        timestamp: Duration,
        window_id: WindowId,
        updates: &mut Vec<WindowCompositorUpdate>,
    ) -> Option<bool> {
        match self.frame.as_mut()?.on_click(timestamp, click, pressed)? {
            FrameAction::Minimize => self.window.set_minimized(),
            FrameAction::Maximize => self.window.set_maximized(),
            FrameAction::UnMaximize => self.window.unset_maximized(),
            FrameAction::Close => WinitState::queue_close(updates, window_id),
            FrameAction::Move => self.has_pending_move = Some(serial),
            FrameAction::Resize(edge) => {
                let edge = match edge {
                    ResizeEdge::None => XdgResizeEdge::None,
                    ResizeEdge::Top => XdgResizeEdge::Top,
                    ResizeEdge::Bottom => XdgResizeEdge::Bottom,
                    ResizeEdge::Left => XdgResizeEdge::Left,
                    ResizeEdge::TopLeft => XdgResizeEdge::TopLeft,
                    ResizeEdge::BottomLeft => XdgResizeEdge::BottomLeft,
                    ResizeEdge::Right => XdgResizeEdge::Right,
                    ResizeEdge::TopRight => XdgResizeEdge::TopRight,
                    ResizeEdge::BottomRight => XdgResizeEdge::BottomRight,
                    _ => return None,
                };
                self.window.resize(seat, serial, edge);
            },
            FrameAction::ShowMenu(x, y) => self.window.show_window_menu(seat, serial, (x, y)),
            _ => (),
        };

        Some(false)
    }

    pub fn frame_point_left(&mut self) {
        if let Some(frame) = self.frame.as_mut() {
            frame.click_point_left();
        }
    }

    // Move the point over decorations.
    pub fn frame_point_moved(
        &mut self,
        seat: &WlSeat,
        surface: &WlSurface,
        timestamp: Duration,
        x: f64,
        y: f64,
    ) -> Option<CursorIcon> {
        // Take the serial if we had any, so it doesn't stick around.
        let serial = self.has_pending_move.take();

        if let Some(frame) = self.frame.as_mut() {
            let cursor = frame.click_point_moved(timestamp, &surface.id(), x, y);
            // If we have a cursor change, that means that cursor is over the decorations,
            // so try to apply move.
            if let Some(serial) = cursor.is_some().then_some(serial).flatten() {
                self.window.move_(seat, serial);
                None
            } else {
                cursor
            }
        } else {
            None
        }
    }

    /// Get the stored resizable state.
    #[inline]
    pub fn resizable(&self) -> bool {
        self.resizable
    }

    /// Set the resizable state on the window.
    ///
    /// Returns `true` when the state was applied.
    #[inline]
    pub fn set_resizable(&mut self, resizable: bool) -> bool {
        if self.resizable == resizable {
            return false;
        }

        self.resizable = resizable;
        if resizable {
            // Restore min/max sizes of the window.
            self.reload_min_max_hints();
        } else {
            self.set_min_inner_size(Some(self.size));
            self.set_max_inner_size(Some(self.size));
        }

        // Reload the state on the frame as well.
        if let Some(frame) = self.frame.as_mut() {
            frame.set_resizable(resizable);
        }

        true
    }

    /// Whether the window is focused by any seat.
    #[inline]
    pub fn has_focus(&self) -> bool {
        !self.seat_focus.is_empty()
    }

    /// Whether the IME is allowed.
    #[inline]
    pub fn ime_allowed(&self) -> bool {
        self.ime_allowed
    }

    /// Get the size of the window.
    #[inline]
    pub fn inner_size(&self) -> LogicalSize<u32> {
        self.size
    }

    /// Whether the window received initial configure event from the compositor.
    #[inline]
    pub fn is_configured(&self) -> bool {
        self.last_configure.is_some()
    }

    #[inline]
    pub fn is_decorated(&mut self) -> bool {
        let csd = self
            .last_configure
            .as_ref()
            .map(|configure| configure.decoration_mode == DecorationMode::Client)
            .unwrap_or(false);
        if let Some(frame) = csd.then_some(self.frame.as_ref()).flatten() {
            !frame.is_hidden()
        } else {
            // Server side decorations.
            true
        }
    }

    /// Get the outer size of the window.
    #[inline]
    pub fn outer_size(&self) -> LogicalSize<u32> {
        self.frame
            .as_ref()
            .map(|frame| frame.add_borders(self.size.width, self.size.height).into())
            .unwrap_or(self.size)
    }

    /// Register pointer on the top-level.
    pub fn pointer_entered(&mut self, added: Weak<ThemedPointer<WinitPointerData>>) {
        self.pointers.push(added);
        self.reload_cursor_style();

        let mode = self.cursor_grab_mode.user_grab_mode;
        let _ = self.set_cursor_grab_inner(mode);
    }

    /// Pointer has left the top-level.
    pub fn pointer_left(&mut self, removed: Weak<ThemedPointer<WinitPointerData>>) {
        let mut new_pointers = Vec::new();
        for pointer in self.pointers.drain(..) {
            if let Some(pointer) = pointer.upgrade() {
                if pointer.pointer() != removed.upgrade().unwrap().pointer() {
                    new_pointers.push(Arc::downgrade(&pointer));
                }
            }
        }

        self.pointers = new_pointers;
    }

    /// Refresh the decorations frame if it's present returning whether the client should redraw.
    pub fn refresh_frame(&mut self) -> bool {
        if let Some(frame) = self.frame.as_mut() {
            if !frame.is_hidden() && frame.is_dirty() {
                return frame.draw();
            }
        }

        false
    }

    /// Reload the cursor style on the given window.
    pub fn reload_cursor_style(&mut self) {
        if self.cursor_visible {
            match &self.selected_cursor {
                SelectedCursor::Named(icon) => self.set_cursor(*icon),
                SelectedCursor::Custom(cursor) => self.apply_custom_cursor(cursor),
            }
        } else {
            self.set_cursor_visible(self.cursor_visible);
        }
    }

    /// Reissue the transparency hint to the compositor.
    pub fn reload_transparency_hint(&self) {
        let surface = self.window.wl_surface();

        if self.transparent {
            surface.set_opaque_region(None);
        } else if let Ok(region) = Region::new(&*self.compositor) {
            region.add(0, 0, i32::MAX, i32::MAX);
            surface.set_opaque_region(Some(region.wl_region()));
        } else {
            warn!("Failed to mark window opaque.");
        }
    }

    /// Try to resize the window when the user can do so.
    pub fn request_inner_size(&mut self, inner_size: Size) -> PhysicalSize<u32> {
        let logical_size = inner_size.to_logical(self.scale_factor());
        let is_stateless = self.last_configure.as_ref().map(Self::is_stateless).unwrap_or(true);

        tracing::trace!(
            "request_inner_size: inner_size={:?}, logical_size={:?}, is_stateless={}, current_stateless_size={:?}",
            inner_size,
            logical_size,
            is_stateless,
            self.stateless_size
        );

        if is_stateless {
            // Window is floating/normal - resize immediately
            tracing::trace!("request_inner_size: window is stateless, calling resize()");
            self.resize(logical_size);
        } else {
            // Window is maximized/fullscreen/tiled - store as the restore size
            // so when unmaximized it will restore to this size
            tracing::trace!(
                "request_inner_size: window is NOT stateless (maximized/tiled), storing {:?} as stateless_size for restore",
                logical_size
            );
            self.stateless_size = logical_size;

            // Send the resize hint to the compositor via animated_resize protocol
            // The compositor will update the restore geometry for when window is unmaximized
            self.request_animated_resize(logical_size.width as i32, logical_size.height as i32, 0);
        }
        logical_to_physical_rounded(self.inner_size(), self.scale_factor())
    }

    /// Resize the window to the new inner size.
    fn resize(&mut self, inner_size: LogicalSize<u32>) {
        self.size = inner_size;

        // Update the stateless size.
        if Some(true) == self.last_configure.as_ref().map(Self::is_stateless) {
            self.stateless_size = inner_size;
        }

        // Update the inner frame.
        let ((x, y), outer_size) = if let Some(frame) = self.frame.as_mut() {
            // Resize only visible frame.
            if !frame.is_hidden() {
                frame.resize(
                    NonZeroU32::new(self.size.width).unwrap(),
                    NonZeroU32::new(self.size.height).unwrap(),
                );
            }

            (frame.location(), frame.add_borders(self.size.width, self.size.height).into())
        } else {
            ((0, 0), self.size)
        };

        // Reload the hint.
        self.reload_transparency_hint();

        // Set the window geometry.
        self.window.xdg_surface().set_window_geometry(
            x,
            y,
            outer_size.width as i32,
            outer_size.height as i32,
        );

        // Update the target viewport, this is used if and only if fractional scaling is in use.
        if let Some(viewport) = self.viewport.as_ref() {
            // Set inner size without the borders.
            viewport.set_destination(self.size.width as _, self.size.height as _);
        }
    }

    /// Get the scale factor of the window.
    #[inline]
    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// Set the cursor icon.
    pub fn set_cursor(&mut self, cursor_icon: CursorIcon) {
        self.selected_cursor = SelectedCursor::Named(cursor_icon);

        if !self.cursor_visible {
            return;
        }

        self.apply_on_pointer(|pointer, _| {
            if pointer.set_cursor(&self.connection, cursor_icon).is_err() {
                warn!("Failed to set cursor to {:?}", cursor_icon);
            }
        })
    }

    /// Set the custom cursor icon.
    pub(crate) fn set_custom_cursor(&mut self, cursor: RootCustomCursor) {
        let cursor = match cursor {
            RootCustomCursor { inner: PlatformCustomCursor::Wayland(cursor) } => cursor.0,
            #[cfg(x11_platform)]
            RootCustomCursor { inner: PlatformCustomCursor::X(_) } => {
                tracing::error!("passed a X11 cursor to Wayland backend");
                return;
            },
        };

        let cursor = {
            let mut pool = self.custom_cursor_pool.lock().unwrap();
            CustomCursor::new(&mut pool, &cursor)
        };

        if self.cursor_visible {
            self.apply_custom_cursor(&cursor);
        }

        self.selected_cursor = SelectedCursor::Custom(cursor);
    }

    fn apply_custom_cursor(&self, cursor: &CustomCursor) {
        self.apply_on_pointer(|pointer, data| {
            let surface = pointer.surface();

            let scale = if let Some(viewport) = data.viewport() {
                let scale = self.scale_factor();
                let size = PhysicalSize::new(cursor.w, cursor.h).to_logical(scale);
                viewport.set_destination(size.width, size.height);
                scale
            } else {
                let scale = surface.data::<SurfaceData>().unwrap().surface_data().scale_factor();
                surface.set_buffer_scale(scale);
                scale as f64
            };

            surface.attach(Some(cursor.buffer.wl_buffer()), 0, 0);
            if surface.version() >= 4 {
                surface.damage_buffer(0, 0, cursor.w, cursor.h);
            } else {
                let size = PhysicalSize::new(cursor.w, cursor.h).to_logical(scale);
                surface.damage(0, 0, size.width, size.height);
            }
            surface.commit();

            let serial = pointer
                .pointer()
                .data::<WinitPointerData>()
                .and_then(|data| data.pointer_data().latest_enter_serial())
                .unwrap();

            let hotspot =
                PhysicalPosition::new(cursor.hotspot_x, cursor.hotspot_y).to_logical(scale);
            pointer.pointer().set_cursor(serial, Some(surface), hotspot.x, hotspot.y);
        });
    }

    /// Set maximum inner window size.
    pub fn set_min_inner_size(&mut self, size: Option<LogicalSize<u32>>) {
        // Ensure that the window has the right minimum size.
        let mut size = size.unwrap_or(MIN_WINDOW_SIZE);
        size.width = size.width.max(MIN_WINDOW_SIZE.width);
        size.height = size.height.max(MIN_WINDOW_SIZE.height);

        // Add the borders.
        let size = self
            .frame
            .as_ref()
            .map(|frame| frame.add_borders(size.width, size.height).into())
            .unwrap_or(size);

        self.min_inner_size = size;
        self.window.set_min_size(Some(size.into()));
    }

    /// Set maximum inner window size.
    pub fn set_max_inner_size(&mut self, size: Option<LogicalSize<u32>>) {
        let size = size.map(|size| {
            self.frame
                .as_ref()
                .map(|frame| frame.add_borders(size.width, size.height).into())
                .unwrap_or(size)
        });

        self.max_inner_size = size;
        self.window.set_max_size(size.map(Into::into));
    }

    /// Set the CSD theme.
    pub fn set_theme(&mut self, theme: Option<Theme>) {
        self.theme = theme;
        #[cfg(feature = "sctk-adwaita")]
        if let Some(frame) = self.frame.as_mut() {
            frame.set_config(into_sctk_adwaita_config(theme))
        }
    }

    /// The current theme for CSD decorations.
    #[inline]
    pub fn theme(&self) -> Option<Theme> {
        self.theme
    }

    /// Set the cursor grabbing state on the top-level.
    pub fn set_cursor_grab(&mut self, mode: CursorGrabMode) -> Result<(), ExternalError> {
        if self.cursor_grab_mode.user_grab_mode == mode {
            return Ok(());
        }

        self.set_cursor_grab_inner(mode)?;
        // Update user grab on success.
        self.cursor_grab_mode.user_grab_mode = mode;
        Ok(())
    }

    /// Reload the hints for minimum and maximum sizes.
    pub fn reload_min_max_hints(&mut self) {
        self.set_min_inner_size(Some(self.min_inner_size));
        self.set_max_inner_size(self.max_inner_size);
    }

    /// Set the grabbing state on the surface.
    fn set_cursor_grab_inner(&mut self, mode: CursorGrabMode) -> Result<(), ExternalError> {
        let pointer_constraints = match self.pointer_constraints.as_ref() {
            Some(pointer_constraints) => pointer_constraints,
            None if mode == CursorGrabMode::None => return Ok(()),
            None => return Err(ExternalError::NotSupported(NotSupportedError::new())),
        };

        let mut unset_old = false;
        match self.cursor_grab_mode.current_grab_mode {
            CursorGrabMode::None => unset_old = true,
            CursorGrabMode::Confined => self.apply_on_pointer(|_, data| {
                data.unconfine_pointer();
                unset_old = true;
            }),
            CursorGrabMode::Locked => {
                self.apply_on_pointer(|_, data| {
                    data.unlock_pointer();
                    unset_old = true;
                });
            },
        }

        // In case we haven't unset the old mode, it means that we don't have a cursor above
        // the window, thus just wait for it to re-appear.
        if !unset_old {
            return Ok(());
        }

        let mut set_mode = false;
        let surface = self.window.wl_surface();
        match mode {
            CursorGrabMode::Locked => self.apply_on_pointer(|pointer, data| {
                let pointer = pointer.pointer();
                data.lock_pointer(pointer_constraints, surface, pointer, &self.queue_handle);
                set_mode = true;
            }),
            CursorGrabMode::Confined => self.apply_on_pointer(|pointer, data| {
                let pointer = pointer.pointer();
                data.confine_pointer(pointer_constraints, surface, pointer, &self.queue_handle);
                set_mode = true;
            }),
            CursorGrabMode::None => {
                // Current lock/confine was already removed.
                set_mode = true;
            },
        }

        // Replace the current grab mode after we've ensure that it got updated.
        if set_mode {
            self.cursor_grab_mode.current_grab_mode = mode;
        }

        Ok(())
    }

    pub fn show_window_menu(&self, position: LogicalPosition<u32>) {
        // TODO(kchibisov) handle touch serials.
        self.apply_on_pointer(|_, data| {
            let serial = data.latest_button_serial();
            let seat = data.seat();
            self.window.show_window_menu(seat, serial, position.into());
        });
    }

    /// Set the position of the cursor.
    pub fn set_cursor_position(&self, position: LogicalPosition<f64>) -> Result<(), ExternalError> {
        if self.pointer_constraints.is_none() {
            return Err(ExternalError::NotSupported(NotSupportedError::new()));
        }

        // Position can be set only for locked cursor.
        if self.cursor_grab_mode.current_grab_mode != CursorGrabMode::Locked {
            return Err(ExternalError::Os(os_error!(crate::platform_impl::OsError::Misc(
                "cursor position can be set only for locked cursor."
            ))));
        }

        self.apply_on_pointer(|_, data| {
            data.set_locked_cursor_position(position.x, position.y);
        });

        Ok(())
    }

    /// Set the visibility state of the cursor.
    pub fn set_cursor_visible(&mut self, cursor_visible: bool) {
        self.cursor_visible = cursor_visible;

        if self.cursor_visible {
            match &self.selected_cursor {
                SelectedCursor::Named(icon) => self.set_cursor(*icon),
                SelectedCursor::Custom(cursor) => self.apply_custom_cursor(cursor),
            }
        } else {
            for pointer in self.pointers.iter().filter_map(|pointer| pointer.upgrade()) {
                let latest_enter_serial = pointer.pointer().winit_data().latest_enter_serial();

                pointer.pointer().set_cursor(latest_enter_serial, None, 0, 0);
            }
        }
    }

    /// Whether show or hide client side decorations.
    #[inline]
    pub fn set_decorate(&mut self, decorate: bool) {
        if decorate == self.decorate {
            return;
        }

        self.decorate = decorate;

        match self.last_configure.as_ref().map(|configure| configure.decoration_mode) {
            Some(DecorationMode::Server) if !self.decorate => {
                // To disable decorations we should request client and hide the frame.
                self.window.request_decoration_mode(Some(DecorationMode::Client))
            },
            _ if self.decorate => self.window.request_decoration_mode(Some(DecorationMode::Server)),
            _ => (),
        }

        if let Some(frame) = self.frame.as_mut() {
            frame.set_hidden(!decorate);
            // Force the resize.
            self.resize(self.size);
        }
    }

    /// Add seat focus for the window.
    #[inline]
    pub fn add_seat_focus(&mut self, seat: ObjectId) {
        self.seat_focus.insert(seat);
    }

    /// Remove seat focus from the window.
    #[inline]
    pub fn remove_seat_focus(&mut self, seat: &ObjectId) {
        self.seat_focus.remove(seat);
    }

    /// Returns `true` if the requested state was applied.
    pub fn set_ime_allowed(&mut self, allowed: bool) -> bool {
        self.ime_allowed = allowed;

        let mut applied = false;
        for text_input in &self.text_inputs {
            applied = true;
            if allowed {
                text_input.enable();
                text_input.set_content_type_by_purpose(self.ime_purpose);
            } else {
                text_input.disable();
            }
            text_input.commit();
        }

        applied
    }

    /// Set the IME position.
    pub fn set_ime_cursor_area(&self, position: LogicalPosition<u32>, size: LogicalSize<u32>) {
        // FIXME: This won't fly unless user will have a way to request IME window per seat, since
        // the ime windows will be overlapping, but winit doesn't expose API to specify for
        // which seat we're setting IME position.
        let (x, y) = (position.x as i32, position.y as i32);
        let (width, height) = (size.width as i32, size.height as i32);
        for text_input in self.text_inputs.iter() {
            text_input.set_cursor_rectangle(x, y, width, height);
            text_input.commit();
        }
    }

    /// Set the IME purpose.
    pub fn set_ime_purpose(&mut self, purpose: ImePurpose) {
        self.ime_purpose = purpose;

        for text_input in &self.text_inputs {
            text_input.set_content_type_by_purpose(purpose);
            text_input.commit();
        }
    }

    /// Get the IME purpose.
    pub fn ime_purpose(&self) -> ImePurpose {
        self.ime_purpose
    }

    /// Set the scale factor for the given window.
    #[inline]
    pub fn set_scale_factor(&mut self, scale_factor: f64) {
        self.scale_factor = scale_factor;

        // NOTE: When fractional scaling is not used update the buffer scale.
        if self.fractional_scale.is_none() {
            let _ = self.window.set_buffer_scale(self.scale_factor as _);
        }

        if let Some(frame) = self.frame.as_mut() {
            frame.set_scaling_factor(scale_factor);
        }
    }

    /// Make window background blurred
    #[inline]
    pub fn set_blur(&mut self, blurred: bool) {
        if blurred && self.blur.is_none() {
            if let Some(blur_manager) = self.blur_manager.as_ref() {
                let blur = blur_manager.blur(self.window.wl_surface(), &self.queue_handle);
                blur.commit();
                self.blur = Some(blur);
            } else {
                info!("Blur manager unavailable, unable to change blur")
            }
        } else if !blurred && self.blur.is_some() {
            self.blur_manager.as_ref().unwrap().unset(self.window.wl_surface());
            self.blur.take().unwrap().release();
        }
    }

    /// Request an animated resize to the target size.
    ///
    /// This uses the COSMIC animated resize protocol to request smooth
    /// compositor-driven resize animation. The compositor will send
    /// intermediate configure events for smooth animation.
    ///
    /// Returns `true` if the request was sent, `false` if the protocol
    /// is not available.
    ///
    /// # Arguments
    /// * `width` - Target width in logical pixels
    /// * `height` - Target height in logical pixels
    /// * `duration_ms` - Animation duration in milliseconds
    #[inline]
    pub fn request_animated_resize(&mut self, width: i32, height: i32, duration_ms: u32) -> bool {
        // Create controller if we don't have one yet
        if self.animated_resize_controller.is_none() {
            if let Some(manager) = self.animated_resize_manager.as_ref() {
                let controller =
                    manager.get_animated_resize(self.window.wl_surface(), &self.queue_handle);
                self.animated_resize_controller = Some(controller);
            } else {
                tracing::trace!("Animated resize manager unavailable");
                return false;
            }
        }

        if let Some(controller) = self.animated_resize_controller.as_ref() {
            tracing::trace!(width, height, duration_ms, "Requesting animated resize to compositor");
            controller.resize_to(width, height, duration_ms);
            true
        } else {
            false
        }
    }

    /// Request an animated resize with explicit position via the animated resize protocol.
    ///
    /// This uses the compositor's animated_resize protocol to smoothly animate
    /// the window from its current geometry to the target geometry.
    ///
    /// If the window is maximized, the position and size will be stored and used
    /// when the window is restored to normal state.
    ///
    /// # Arguments
    /// * `x` - Target x position in logical pixels
    /// * `y` - Target y position in logical pixels
    /// * `width` - Target width in logical pixels
    /// * `height` - Target height in logical pixels
    /// * `duration_ms` - Animation duration in milliseconds
    #[inline]
    pub fn request_animated_resize_with_position(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        duration_ms: u32,
    ) -> bool {
        // Create controller if we don't have one yet
        if self.animated_resize_controller.is_none() {
            if let Some(manager) = self.animated_resize_manager.as_ref() {
                let controller =
                    manager.get_animated_resize(self.window.wl_surface(), &self.queue_handle);
                self.animated_resize_controller = Some(controller);
            } else {
                tracing::trace!("Animated resize manager unavailable");
                return false;
            }
        }

        if let Some(controller) = self.animated_resize_controller.as_ref() {
            tracing::trace!(
                x,
                y,
                width,
                height,
                duration_ms,
                "Requesting animated resize with position to compositor"
            );
            controller.resize_to_with_position(x, y, width, height, duration_ms);
            true
        } else {
            false
        }
    }

    /// Set exclusive mode for this window.
    ///
    /// When exclusive mode is enabled, all other toplevel windows on the same
    /// output are minimized. When disabled, they are restored.
    ///
    /// Returns `true` if the request was sent, `false` if the protocol
    /// is not available.
    ///
    /// # Arguments
    /// * `exclusive` - `true` to enable exclusive mode, `false` to disable
    #[inline]
    pub fn set_exclusive_mode(&mut self, exclusive: bool) -> bool {
        // Create controller if we don't have one yet
        if self.exclusive_mode_controller.is_none() {
            if let Some(manager) = self.exclusive_mode_manager.as_ref() {
                let controller =
                    manager.get_exclusive_mode(self.window.wl_surface(), &self.queue_handle);
                self.exclusive_mode_controller = Some(controller);
            } else {
                tracing::trace!("Exclusive mode manager unavailable");
                return false;
            }
        }

        if let Some(controller) = self.exclusive_mode_controller.as_ref() {
            tracing::trace!(exclusive, "Setting exclusive mode");
            controller.set_exclusive(exclusive);
            true
        } else {
            false
        }
    }

    /// Check if exclusive mode is currently enabled for this window.
    pub fn is_exclusive_mode(&self) -> bool {
        self.exclusive_mode_controller.as_ref().map(|c| c.is_enabled()).unwrap_or(false)
    }

    /// Set corner radius for this window.
    ///
    /// Communicates the corner radius hint to the compositor so it can
    /// draw proper blur outlines and rounded corners.
    ///
    /// Returns `true` if the request was sent, `false` if the protocol
    /// is not available.
    #[inline]
    pub fn set_corner_radius(
        &mut self,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) -> bool {
        // Create controller if we don't have one yet
        if self.corner_radius_controller.is_none() {
            if let Some(manager) = self.corner_radius_manager.as_ref() {
                let controller =
                    manager.get_corner_radius(self.window.wl_surface(), &self.queue_handle);
                self.corner_radius_controller = Some(controller);
            } else {
                tracing::trace!("Corner radius manager unavailable");
                return false;
            }
        }

        if let Some(controller) = self.corner_radius_controller.as_ref() {
            tracing::trace!(
                top_left,
                top_right,
                bottom_right,
                bottom_left,
                "Setting corner radius"
            );
            controller.set_radius(top_left, top_right, bottom_right, bottom_left);
            true
        } else {
            false
        }
    }

    /// Set the compositor-rendered backdrop color for this window.
    ///
    /// Returns `true` if the request was sent, `false` if the protocol
    /// is not available.
    #[inline]
    pub fn set_backdrop_color(&mut self, r: u32, g: u32, b: u32, a: u32) -> bool {
        if self.backdrop_color_controller.is_none() {
            if let Some(manager) = self.backdrop_color_manager.as_ref() {
                let controller =
                    manager.get_backdrop_color(self.window.wl_surface(), &self.queue_handle);
                self.backdrop_color_controller = Some(controller);
            } else {
                tracing::trace!("Backdrop color manager unavailable");
                return false;
            }
        }

        if let Some(controller) = self.backdrop_color_controller.as_ref() {
            tracing::trace!(r, g, b, a, "Setting backdrop color");
            controller.set_color(r, g, b, a);
            true
        } else {
            false
        }
    }

    /// Register this window as a voice mode receiver.
    ///
    /// Returns `true` if registration was successful, `false` if the protocol
    /// is not available.
    ///
    /// # Arguments
    /// * `is_default` - If true, this window becomes the default receiver
    #[inline]
    pub fn register_voice_mode(&mut self, is_default: bool) -> bool {
        // Already registered
        if self.voice_mode_receiver.is_some() {
            tracing::trace!("Voice mode already registered for this window");
            return true;
        }

        if let Some(manager) = self.voice_mode_manager.as_ref() {
            let receiver =
                manager.get_voice_mode(self.window.wl_surface(), is_default, &self.queue_handle);
            self.voice_mode_receiver = Some(receiver);
            tracing::info!(is_default, "Registered window as voice mode receiver");
            true
        } else {
            tracing::trace!("Voice mode manager unavailable");
            false
        }
    }

    /// Unregister this window as a voice mode receiver.
    #[inline]
    pub fn unregister_voice_mode(&mut self) -> bool {
        if let Some(receiver) = self.voice_mode_receiver.take() {
            receiver.destroy();
            tracing::info!("Unregistered window as voice mode receiver");
            true
        } else {
            false
        }
    }

    /// Set the audio level for voice mode visualization.
    ///
    /// # Arguments
    /// * `level` - Audio level from 0-1000, where 0 is silence and 1000 is maximum.
    #[inline]
    pub fn set_voice_audio_level(&mut self, level: u32) -> bool {
        if let Some(receiver) = self.voice_mode_receiver.as_ref() {
            receiver.set_audio_level(level);
            true
        } else {
            false
        }
    }

    /// Acknowledge a will_stop event from the compositor.
    #[inline]
    pub fn voice_ack_stop(&mut self, serial: u32, freeze: bool) -> bool {
        if let Some(receiver) = self.voice_mode_receiver.as_ref() {
            receiver.ack_stop(serial, freeze);
            tracing::info!(serial, freeze, "Sent ack_stop to compositor");
            true
        } else {
            false
        }
    }

    /// Dismiss the frozen voice orb.
    ///
    /// This tells the compositor to hide the orb when transcription completes
    /// without spawning a new window (e.g., empty result or error).
    #[inline]
    pub fn voice_dismiss(&mut self) -> bool {
        if let Some(receiver) = self.voice_mode_receiver.as_ref() {
            receiver.dismiss();
            tracing::info!("Sent dismiss to compositor");
            true
        } else {
            false
        }
    }

    /// Take pending voice mode events from this window's receiver.
    ///
    /// Returns a list of events that should be sent as WindowEvent::VoiceMode.
    pub fn take_voice_mode_events(&self) -> Vec<crate::event::VoiceModeWindowEvent> {
        use crate::event::{VoiceModeOrbState, VoiceModeWindowEvent};
        use crate::platform_impl::wayland::types::cosmic_voice_mode::{OrbState, VoiceModeEvent};

        let Some(receiver) = self.voice_mode_receiver.as_ref() else {
            return Vec::new();
        };

        receiver
            .take_events()
            .into_iter()
            .map(|event| match event {
                VoiceModeEvent::Start { orb_state } => {
                    let orb_state = match orb_state {
                        OrbState::Hidden => VoiceModeOrbState::Hidden,
                        OrbState::Floating => VoiceModeOrbState::Floating,
                        OrbState::Attached => VoiceModeOrbState::Attached,
                        OrbState::Frozen => VoiceModeOrbState::Frozen,
                        OrbState::Transitioning => VoiceModeOrbState::Transitioning,
                    };
                    VoiceModeWindowEvent::Start { orb_state }
                },
                VoiceModeEvent::Stop => VoiceModeWindowEvent::Stop,
                VoiceModeEvent::Cancel => VoiceModeWindowEvent::Cancel,
                VoiceModeEvent::OrbAttached { x, y, width, height } => {
                    VoiceModeWindowEvent::OrbAttached { x, y, width, height }
                },
                VoiceModeEvent::OrbDetached => VoiceModeWindowEvent::OrbDetached,
                VoiceModeEvent::WillStop { serial } => VoiceModeWindowEvent::WillStop { serial },
                VoiceModeEvent::FocusInput => VoiceModeWindowEvent::FocusInput,
            })
            .collect()
    }

    /// Embed a toplevel by process ID into this window's surface.
    ///
    /// This requests the compositor to embed the window created by the specified
    /// process into this window's surface. The compositor will monitor for new
    /// toplevels from the PID and embed the first matching one.
    ///
    /// Returns an embed ID that can be used to set geometry or remove the embed,
    /// or `None` if the surface embed protocol is not available.
    ///
    /// # Arguments
    /// * `pid` - Process ID of the application to embed
    /// * `app_id` - Optional app_id hint for verification (can be empty)
    /// * `x` - X position within this window's surface
    /// * `y` - Y position within this window's surface
    /// * `width` - Width of the embed region
    /// * `height` - Height of the embed region
    /// * `interactive` - Whether input should be routed to the embedded surface
    pub fn embed_toplevel_by_pid(
        &mut self,
        pid: u32,
        app_id: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        interactive: bool,
    ) -> Option<u64> {
        let manager = self.surface_embed_manager.as_ref()?;

        let embedded = manager.embed_toplevel_by_pid(
            self.window.wl_surface(),
            pid,
            app_id,
            &self.queue_handle,
        );

        // Set initial geometry if width/height are provided
        // Skip set_geometry when width=0/height=0 (anchor-based positioning will be used)
        if width > 0 && height > 0 {
            embedded.set_geometry(x, y, width, height);
        }
        embedded.set_interactive(interactive);
        embedded.commit();

        let embed_id = self.next_embed_id;
        self.next_embed_id += 1;
        self.embedded_surfaces.insert(embed_id, embedded);

        tracing::info!(
            embed_id,
            pid,
            app_id,
            x,
            y,
            width,
            height,
            interactive,
            "Created embed for PID"
        );

        Some(embed_id)
    }

    /// Update the geometry of an embedded surface.
    ///
    /// # Arguments
    /// * `embed_id` - The ID returned from `embed_toplevel_by_pid`
    /// * `x` - X position within this window's surface
    /// * `y` - Y position within this window's surface
    /// * `width` - Width of the embed region
    /// * `height` - Height of the embed region
    pub fn set_embed_geometry(
        &mut self,
        embed_id: u64,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if let Some(embedded) = self.embedded_surfaces.get(&embed_id) {
            embedded.set_geometry(x, y, width, height);
            embedded.commit();
            tracing::trace!(embed_id, x, y, width, height, "Updated embed geometry");
            true
        } else {
            tracing::warn!(embed_id, "Embed not found for geometry update");
            false
        }
    }

    /// Set anchor-based positioning for an embedded surface.
    ///
    /// Instead of specifying absolute (x, y) coordinates, this allows positioning
    /// relative to the parent window edges. The geometry is automatically
    /// recalculated by the compositor when the parent window resizes.
    ///
    /// # Arguments
    /// * `embed_id` - The embedded surface ID
    /// * `anchor` - Bitflags indicating which edges to anchor to (0=none, 1=top, 2=bottom, 4=left, 8=right)
    /// * `margin_top` - Margin from top edge
    /// * `margin_right` - Margin from right edge
    /// * `margin_bottom` - Margin from bottom edge
    /// * `margin_left` - Margin from left edge
    /// * `width` - Width of embed region (0 to stretch between left/right anchors)
    /// * `height` - Height of embed region (0 to stretch between top/bottom anchors)
    pub fn set_embed_anchor(
        &mut self,
        embed_id: u64,
        anchor: u32,
        margin_top: i32,
        margin_right: i32,
        margin_bottom: i32,
        margin_left: i32,
        width: i32,
        height: i32,
    ) -> bool {
        if let Some(embedded) = self.embedded_surfaces.get(&embed_id) {
            embedded.set_anchor(
                anchor,
                margin_top,
                margin_right,
                margin_bottom,
                margin_left,
                width,
                height,
            );
            embedded.commit();
            tracing::trace!(
                embed_id,
                anchor,
                margin_top,
                margin_right,
                margin_bottom,
                margin_left,
                width,
                height,
                "Updated embed anchor"
            );
            true
        } else {
            tracing::warn!(embed_id, "Embed not found for anchor update");
            false
        }
    }

    /// Set corner radius for an embedded surface.
    ///
    /// This allows the parent to specify rounded corners that match its own UI.
    /// Each corner can have a different radius. Values are in logical pixels.
    pub fn set_embed_corner_radius(
        &mut self,
        embed_id: u64,
        top_left: u32,
        top_right: u32,
        bottom_right: u32,
        bottom_left: u32,
    ) -> bool {
        if let Some(embedded) = self.embedded_surfaces.get(&embed_id) {
            embedded.set_corner_radius(top_left, top_right, bottom_right, bottom_left);
            embedded.commit();
            tracing::trace!(
                embed_id,
                top_left,
                top_right,
                bottom_right,
                bottom_left,
                "Updated embed corner radius"
            );
            true
        } else {
            tracing::warn!(embed_id, "Embed not found for corner radius update");
            false
        }
    }

    /// Set interactivity for an embedded surface.
    ///
    /// When interactive, pointer/keyboard/touch events within the embed
    /// region will be routed to the embedded toplevel.
    pub fn set_embed_interactive(&mut self, embed_id: u64, interactive: bool) -> bool {
        if let Some(embedded) = self.embedded_surfaces.get(&embed_id) {
            embedded.set_interactive(interactive);
            embedded.commit();
            tracing::trace!(embed_id, interactive, "Updated embed interactivity");
            true
        } else {
            tracing::warn!(embed_id, "Embed not found for interactive update");
            false
        }
    }

    /// Remove an embedded surface.
    pub fn remove_embed(&mut self, embed_id: u64) -> bool {
        if self.embedded_surfaces.remove(&embed_id).is_some() {
            tracing::info!(embed_id, "Removed embed");
            true
        } else {
            tracing::warn!(embed_id, "Embed not found for removal");
            false
        }
    }

    /// Check if an embedded surface is still valid.
    pub fn is_embed_valid(&self, embed_id: u64) -> bool {
        self.embedded_surfaces.get(&embed_id).is_some_and(|e| e.is_valid())
    }

    /// Start a Wayland drag-and-drop operation from this window.
    ///
    /// Creates a `wl_data_source` with the given MIME types and actions,
    /// stores the pre-serialized data for `send` events, and calls
    /// `wl_data_device.start_drag(…)` with the latest pointer serial.
    ///
    /// Returns `true` if the drag was started, `false` if DnD is unavailable
    /// or no pointer serial is available.
    ///
    /// `icon_pixels` is `(width, height, pixels, buffer_scale)` for HiDPI support.
    pub fn start_drag(
        &mut self,
        mime_types: Vec<String>,
        actions: u32,
        data: Vec<Vec<u8>>,
        icon_pixels: Option<(u32, u32, Vec<u8>, i32)>,
    ) -> (bool, Vec<String>, Vec<Vec<u8>>) {
        use crate::platform_impl::wayland::types::wayland_dnd::DndSourceData;

        let dnd_manager = match self.dnd_manager.as_ref() {
            Some(m) => m,
            None => {
                tracing::error!("DnD manager unavailable, cannot start drag");
                return (false, mime_types, data);
            },
        };

        let data_device = match self.dnd_data_devices.first() {
            Some(d) => d.clone(),
            None => {
                tracing::error!("No data device available, cannot start drag");
                return (false, mime_types, data);
            },
        };

        // Get the latest pointer serial.
        let serial = self
            .pointers
            .iter()
            .filter_map(|p| p.upgrade())
            .map(|p| p.pointer().winit_data().latest_button_serial())
            .find(|&s| s != 0);

        let serial = match serial {
            Some(s) => s,
            None => {
                tracing::error!("No pointer serial available for DnD start_drag");
                return (false, mime_types, data);
            },
        };

        // Get the seat from the first pointer.
        let seat = self
            .pointers
            .iter()
            .filter_map(|p| p.upgrade())
            .map(|p| p.pointer().winit_data().seat().clone())
            .next();

        let _seat = match seat {
            Some(s) => s,
            None => {
                tracing::error!("No seat available for DnD start_drag");
                return (false, mime_types, data);
            },
        };

        // Create the data source with pre-serialized payload as user_data.
        let payload = DndSourceData { mime_types: mime_types.clone(), data: data.clone() };
        let source = dnd_manager.create_data_source(&self.queue_handle, payload);

        // Offer MIME types.
        for mime in &mime_types {
            source.offer(mime.clone());
        }

        // Set DnD actions on the source.
        use wayland_client::protocol::wl_data_device_manager::DndAction;
        let mut wl_actions = DndAction::empty();
        if actions & 1 != 0 {
            wl_actions |= DndAction::Copy;
        }
        if actions & 2 != 0 {
            wl_actions |= DndAction::Move;
        }
        if actions & 4 != 0 {
            wl_actions |= DndAction::Ask;
        }
        source.set_actions(wl_actions);

        // Create the icon surface for the drag visual.
        // If the caller provided custom ARGB pixels, use those; otherwise
        // fall back to a generic 48×48 semi-transparent rounded rectangle.
        let icon = {
            let mut pool = self.custom_cursor_pool.lock().unwrap();
            if let Some((w, h, pixels, scale)) = icon_pixels {
                crate::platform_impl::wayland::types::wayland_dnd::create_dnd_icon_from_pixels(
                    &self.compositor,
                    &mut pool,
                    &self.queue_handle,
                    w,
                    h,
                    &pixels,
                    scale,
                )
            } else {
                crate::platform_impl::wayland::types::wayland_dnd::create_dnd_icon_surface(
                    &self.compositor,
                    &mut pool,
                    &self.queue_handle,
                )
            }
        };
        let icon_surface_ref = icon.as_ref().map(|(s, _)| s);
        let had_icon = icon_surface_ref.is_some();

        let origin = self.window.wl_surface();
        data_device.start_drag(Some(&source), origin, icon_surface_ref, serial);

        // Keep icon surface + buffer alive for the drag duration.
        self.dnd_icon = icon;

        tracing::info!(
            serial,
            ?mime_types,
            has_icon = had_icon,
            "DnD: started drag from window (wl_data_device.start_drag)"
        );

        // Return the source info so the caller can store it in WinitState's dnd_session.
        (true, mime_types, data)
    }

    /// Accept a MIME type from the current DnD offer.
    ///
    /// Pass `None` to reject the current drag.
    pub fn dnd_accept_mime_type(&self, mime_type: Option<&str>) {
        let guard = self.dnd_shared_offer.lock().unwrap();
        if let Some(offer) = guard.current_offer.as_ref() {
            // Get the latest pointer serial for the accept call.
            let serial = self
                .pointers
                .iter()
                .filter_map(|p| p.upgrade())
                .map(|p| p.pointer().winit_data().latest_enter_serial())
                .find(|&s| s != 0)
                .unwrap_or(0);

            offer.accept(serial, mime_type.map(String::from));
            tracing::trace!(?mime_type, serial, "DnD: accept_mime_type");
        } else {
            tracing::warn!("DnD: accept_mime_type called with no current offer");
        }
    }

    /// Set the accepted DnD actions and preferred action.
    pub fn dnd_set_actions(&self, actions: u32, preferred: u32) {
        let guard = self.dnd_shared_offer.lock().unwrap();
        if let Some(offer) = guard.current_offer.as_ref() {
            use wayland_client::protocol::wl_data_device_manager::DndAction;
            let mut wl_actions = DndAction::empty();
            if actions & 1 != 0 {
                wl_actions |= DndAction::Copy;
            }
            if actions & 2 != 0 {
                wl_actions |= DndAction::Move;
            }
            if actions & 4 != 0 {
                wl_actions |= DndAction::Ask;
            }
            let mut wl_preferred = DndAction::empty();
            if preferred & 1 != 0 {
                wl_preferred |= DndAction::Copy;
            }
            if preferred & 2 != 0 {
                wl_preferred |= DndAction::Move;
            }
            if preferred & 4 != 0 {
                wl_preferred |= DndAction::Ask;
            }
            offer.set_actions(wl_actions, wl_preferred);
            tracing::trace!(?wl_actions, ?wl_preferred, "DnD: set_actions");
        } else {
            tracing::warn!("DnD: set_actions called with no current offer");
        }
    }

    /// Signal that the destination has finished processing the drop.
    pub fn dnd_finish(&self) {
        let mut guard = self.dnd_shared_offer.lock().unwrap();
        if let Some(offer) = guard.current_offer.take() {
            offer.finish();
            tracing::info!("DnD: finish - called offer.finish() and cleared offer");
        } else {
            tracing::warn!("DnD: finish called with no current offer");
        }
    }

    /// Request data from the current DnD offer for the specified MIME type.
    ///
    /// The data will be stored in `SharedDndOfferState::pending_data` for the
    /// event loop to pick up and dispatch as `DndWindowEvent::DataReceived`.
    pub fn dnd_request_data(&self, mime_type: &str) {
        use std::io::Read;
        use std::os::fd::AsFd;

        let mut guard = self.dnd_shared_offer.lock().unwrap();
        let Some(offer) = guard.current_offer.as_ref() else {
            tracing::warn!("DnD: request_data called with no current offer");
            return;
        };

        // Create a pipe for the data transfer
        let (read_fd, write_fd) = match rustix::pipe::pipe() {
            Ok(fds) => fds,
            Err(e) => {
                tracing::error!(?e, "DnD: failed to create pipe for receive");
                return;
            },
        };

        tracing::trace!(?mime_type, "DnD: requesting data via receive");

        // Tell the source to write data to our pipe
        offer.receive(mime_type.to_string(), write_fd.as_fd());

        // Drop the write end so we get EOF when the source is done writing
        drop(write_fd);

        // We need to flush the Wayland connection so the source receives the request
        let _ = self.connection.flush();

        // Read the data from the pipe
        let mut read_file = std::fs::File::from(read_fd);
        let mut data = Vec::new();

        // Read with a reasonable buffer - the source should have written by now
        // after we flushed and the compositor forwarded the request
        if let Err(e) = read_file.read_to_end(&mut data) {
            tracing::error!(?e, "DnD: failed to read data from pipe");
            return;
        }

        tracing::trace!(mime_type, bytes = data.len(), "DnD: received data");

        // Store the data for the event loop to dispatch
        guard.pending_data.push((mime_type.to_string(), data));
    }

    /// Set the window title to a new value.
    ///
    /// This will automatically truncate the title to something meaningful.
    pub fn set_title(&mut self, mut title: String) {
        // Truncate the title to at most 1024 bytes, so that it does not blow up the protocol
        // messages
        if title.len() > 1024 {
            let mut new_len = 1024;
            while !title.is_char_boundary(new_len) {
                new_len -= 1;
            }
            title.truncate(new_len);
        }

        // Update the CSD title.
        if let Some(frame) = self.frame.as_mut() {
            frame.set_title(&title);
        }

        self.window.set_title(&title);
        self.title = title;
    }

    /// Mark the window as transparent.
    #[inline]
    pub fn set_transparent(&mut self, transparent: bool) {
        self.transparent = transparent;
        self.reload_transparency_hint();
    }

    /// Register text input on the top-level.
    #[inline]
    pub fn text_input_entered(&mut self, text_input: &ZwpTextInputV3) {
        if !self.text_inputs.iter().any(|t| t == text_input) {
            self.text_inputs.push(text_input.clone());
        }
    }

    /// The text input left the top-level.
    #[inline]
    pub fn text_input_left(&mut self, text_input: &ZwpTextInputV3) {
        if let Some(position) = self.text_inputs.iter().position(|t| t == text_input) {
            self.text_inputs.remove(position);
        }
    }

    /// Get the cached title.
    #[inline]
    pub fn title(&self) -> &str {
        &self.title
    }
}

impl Drop for WindowState {
    fn drop(&mut self) {
        if let Some(blur) = self.blur.take() {
            blur.release();
        }

        if let Some(fs) = self.fractional_scale.take() {
            fs.destroy();
        }

        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }

        // NOTE: the wl_surface used by the window is being cleaned up when
        // dropping SCTK `Window`.
    }
}

/// The state of the cursor grabs.
#[derive(Clone, Copy)]
struct GrabState {
    /// The grab mode requested by the user.
    user_grab_mode: CursorGrabMode,

    /// The current grab mode.
    current_grab_mode: CursorGrabMode,
}

impl GrabState {
    fn new() -> Self {
        Self { user_grab_mode: CursorGrabMode::None, current_grab_mode: CursorGrabMode::None }
    }
}

/// The state of the frame callback.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameCallbackState {
    /// No frame callback was requested.
    #[default]
    None,
    /// The frame callback was requested, but not yet arrived, the redraw events are throttled.
    Requested,
    /// The callback was marked as done, and user could receive redraw requested
    Received,
}

impl From<ResizeDirection> for XdgResizeEdge {
    fn from(value: ResizeDirection) -> Self {
        match value {
            ResizeDirection::North => XdgResizeEdge::Top,
            ResizeDirection::West => XdgResizeEdge::Left,
            ResizeDirection::NorthWest => XdgResizeEdge::TopLeft,
            ResizeDirection::NorthEast => XdgResizeEdge::TopRight,
            ResizeDirection::East => XdgResizeEdge::Right,
            ResizeDirection::SouthWest => XdgResizeEdge::BottomLeft,
            ResizeDirection::SouthEast => XdgResizeEdge::BottomRight,
            ResizeDirection::South => XdgResizeEdge::Bottom,
        }
    }
}

// NOTE: Rust doesn't allow `From<Option<Theme>>`.
#[cfg(feature = "sctk-adwaita")]
fn into_sctk_adwaita_config(theme: Option<Theme>) -> sctk_adwaita::FrameConfig {
    match theme {
        Some(Theme::Light) => sctk_adwaita::FrameConfig::light(),
        Some(Theme::Dark) => sctk_adwaita::FrameConfig::dark(),
        None => sctk_adwaita::FrameConfig::auto(),
    }
}
