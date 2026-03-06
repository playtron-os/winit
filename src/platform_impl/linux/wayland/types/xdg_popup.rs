//! XDG popup surface implementation.
//!
//! This module provides xdg_popup support for context menus and other
//! transient surfaces that can extend outside their parent window bounds.

use sctk::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use sctk::shell::xdg::popup::Popup;

use crate::dpi::LogicalSize;
use crate::platform_impl::wayland::types::cosmic_tooltip::TooltipHandle;
use crate::platform_impl::wayland::WindowId;
use crate::window::WindowId as PublicWindowId;

/// Popup anchor position on the parent surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum PopupAnchor {
    #[default]
    None = 0,
    Top = 1,
    Bottom = 2,
    Left = 3,
    Right = 4,
    TopLeft = 5,
    BottomLeft = 6,
    TopRight = 7,
    BottomRight = 8,
}

impl PopupAnchor {
    pub fn to_xdg(self) -> wayland_protocols::xdg::shell::client::xdg_positioner::Anchor {
        use wayland_protocols::xdg::shell::client::xdg_positioner::Anchor;
        match self {
            PopupAnchor::None => Anchor::None,
            PopupAnchor::Top => Anchor::Top,
            PopupAnchor::Bottom => Anchor::Bottom,
            PopupAnchor::Left => Anchor::Left,
            PopupAnchor::Right => Anchor::Right,
            PopupAnchor::TopLeft => Anchor::TopLeft,
            PopupAnchor::BottomLeft => Anchor::BottomLeft,
            PopupAnchor::TopRight => Anchor::TopRight,
            PopupAnchor::BottomRight => Anchor::BottomRight,
        }
    }
}

impl From<u32> for PopupAnchor {
    fn from(value: u32) -> Self {
        match value {
            1 => PopupAnchor::Top,
            2 => PopupAnchor::Bottom,
            3 => PopupAnchor::Left,
            4 => PopupAnchor::Right,
            5 => PopupAnchor::TopLeft,
            6 => PopupAnchor::BottomLeft,
            7 => PopupAnchor::TopRight,
            8 => PopupAnchor::BottomRight,
            _ => PopupAnchor::None,
        }
    }
}

/// Popup gravity direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum PopupGravity {
    #[default]
    None = 0,
    Top = 1,
    Bottom = 2,
    Left = 3,
    Right = 4,
    TopLeft = 5,
    BottomLeft = 6,
    TopRight = 7,
    BottomRight = 8,
}

impl PopupGravity {
    pub fn to_xdg(self) -> wayland_protocols::xdg::shell::client::xdg_positioner::Gravity {
        use wayland_protocols::xdg::shell::client::xdg_positioner::Gravity;
        match self {
            PopupGravity::None => Gravity::None,
            PopupGravity::Top => Gravity::Top,
            PopupGravity::Bottom => Gravity::Bottom,
            PopupGravity::Left => Gravity::Left,
            PopupGravity::Right => Gravity::Right,
            PopupGravity::TopLeft => Gravity::TopLeft,
            PopupGravity::BottomLeft => Gravity::BottomLeft,
            PopupGravity::TopRight => Gravity::TopRight,
            PopupGravity::BottomRight => Gravity::BottomRight,
        }
    }
}

impl From<u32> for PopupGravity {
    fn from(value: u32) -> Self {
        match value {
            1 => PopupGravity::Top,
            2 => PopupGravity::Bottom,
            3 => PopupGravity::Left,
            4 => PopupGravity::Right,
            5 => PopupGravity::TopLeft,
            6 => PopupGravity::BottomLeft,
            7 => PopupGravity::TopRight,
            8 => PopupGravity::BottomRight,
            _ => PopupGravity::None,
        }
    }
}

/// Settings for creating a popup surface.
#[derive(Debug, Clone)]
pub struct PopupSettings {
    /// ID of the parent window this popup belongs to.
    pub parent_id: PublicWindowId,
    /// Size of the popup (width, height).
    pub size: (u32, u32),
    /// Anchor rectangle on the parent surface (x, y, width, height).
    pub anchor_rect: (i32, i32, i32, i32),
    /// Anchor edge of the rectangle.
    pub anchor: PopupAnchor,
    /// Gravity direction.
    pub gravity: PopupGravity,
    /// Offset from calculated position (x, y).
    pub offset: (i32, i32),
    /// Constraint adjustment flags (xdg_positioner::ConstraintAdjustment bits).
    /// 0 = no adjustment, let popup extend outside parent.
    pub constraint_adjustment: u32,
    /// Whether to grab input focus.
    pub grab: bool,
    /// Window geometry (x, y, width, height) in logical pixels.
    ///
    /// Tells the compositor which part of the surface is visible content,
    /// excluding shadows/decorations. Used by the compositor for constraint
    /// adjustment — only the geometry rect is kept on-screen.
    pub window_geometry: Option<(i32, i32, i32, i32)>,
    /// When set, enables compositor-driven tooltip positioning at pointer + offset.
    /// The tuple is (x, y) offset in logical pixels.
    pub tooltip_offset: Option<(i32, i32)>,
    /// Tooltip anchor corner (0=TopLeft, 1=TopRight, 2=BottomLeft, 3=BottomRight).
    pub tooltip_anchor: Option<u32>,
    /// Show delay in milliseconds (0 = immediate cursor-following, >0 = delayed fixed position).
    pub tooltip_delay_ms: Option<u32>,
}

impl Default for PopupSettings {
    fn default() -> Self {
        Self {
            parent_id: PublicWindowId::from(0u64),
            size: (200, 200),
            anchor_rect: (0, 0, 1, 1),
            anchor: PopupAnchor::None,
            gravity: PopupGravity::None,
            offset: (0, 0),
            constraint_adjustment: 0,
            grab: false,
            window_geometry: None,
            tooltip_offset: None,
            tooltip_anchor: None,
            tooltip_delay_ms: None,
        }
    }
}

/// State for a popup surface.
pub struct PopupState {
    /// The sctk popup instance.
    pub popup: Popup,
    /// Parent window ID.
    pub parent_id: WindowId,
    /// Current size.
    pub size: LogicalSize<u32>,
    /// Whether first configure has been received.
    pub configured: bool,
    /// Viewporter for HiDPI scaling (optional, present if compositor supports it).
    pub viewport: Option<WpViewport>,
    /// Tooltip handle (if tooltip positioning is enabled for this popup).
    pub tooltip: Option<TooltipHandle>,
}

impl PopupState {
    /// Create a new popup state.
    pub fn new(
        popup: Popup,
        parent_id: WindowId,
        size: LogicalSize<u32>,
        viewport: Option<WpViewport>,
    ) -> Self {
        Self { popup, parent_id, size, configured: false, viewport, tooltip: None }
    }
}

/// Unique ID for a popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PopupId(pub u64);

impl PopupId {
    /// Create a new popup ID.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for PopupId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<u64> for PopupId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

/// Popup event sent to the application.
#[derive(Debug, Clone)]
pub enum PopupEvent {
    /// Popup has been configured and is ready for content.
    Configure { id: PopupId, width: u32, height: u32 },
    /// Popup was dismissed (closed by compositor or user clicking outside).
    Done { id: PopupId },
    /// Pointer entered the popup surface.
    PointerEnter { id: PopupId, x: f64, y: f64 },
    /// Pointer left the popup surface.
    PointerLeave { id: PopupId },
    /// Pointer moved on the popup surface.
    PointerMotion { id: PopupId, x: f64, y: f64 },
    /// Pointer button pressed/released on the popup surface.
    PointerButton { id: PopupId, button: u32, pressed: bool },
}
