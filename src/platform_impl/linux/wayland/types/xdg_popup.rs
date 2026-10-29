//! XDG popup surface implementation.
//!
//! This module provides xdg_popup support for context menus and other
//! transient surfaces that can extend outside their parent window bounds.

use sctk::compositor::{CompositorState, Region};
use sctk::reexports::client::Proxy;
use sctk::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use sctk::shell::xdg::popup::Popup;
use sctk::shell::xdg::{XdgPositioner, XdgShell};
use wayland_protocols::xdg::shell::client::xdg_positioner;
use wayland_protocols_plasma::blur::client::org_kde_kwin_blur::OrgKdeKwinBlur;

use crate::dpi::LogicalSize;
use crate::event::WindowEvent;
use crate::platform_impl::wayland::types::background_effect::BackgroundEffect;
use crate::platform_impl::wayland::types::cosmic_corner_radius::CornerRadiusController;
use crate::platform_impl::wayland::types::cosmic_tooltip::TooltipHandle;
use crate::platform_impl::wayland::types::layer_shadow::ShadowController;
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
    /// Ask the compositor to blur what is behind the popup.
    ///
    /// Needs a compositor implementing one of the blur protocols, and content
    /// that is translucent — an opaque card has nothing to show the blur
    /// through. The blurred area is the card: the window geometry, or the
    /// whole surface without one. It follows the surface when it resizes.
    pub blur: bool,
    /// Ask the compositor to draw a drop shadow behind the popup.
    ///
    /// The alternative to padding the surface out and drawing one, whose
    /// padding is then only an invisible ring around the card.
    pub shadow: bool,
    /// Corner radius hinted to the compositor, in logical pixels.
    ///
    /// Rounds the blur and the shadow to match the card drawn on the surface.
    /// Without it both come out square while the content is rounded.
    pub corner_radius: Option<u32>,
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
            blur: false,
            shadow: false,
            corner_radius: None,
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
    /// The toplevel this popup's tree belongs to, even when its parent is a popup.
    pub parent_id: WindowId,
    /// The popup this one is parented to, if not the toplevel.
    pub parent_popup: Option<PopupId>,
    /// Whether an xdg_popup grab was sent; a child may grab only under the newest such popup.
    pub grabbed: bool,
    /// Current size.
    pub size: LogicalSize<u32>,
    /// Whether first configure has been received.
    pub configured: bool,
    /// Viewporter for HiDPI scaling (optional, present if compositor supports it).
    pub viewport: Option<WpViewport>,
    /// Tooltip handle (if tooltip positioning is enabled for this popup).
    pub tooltip: Option<TooltipHandle>,
    /// Background-effect handle, when the popup asked to be blurred.
    ///
    /// Held for the popup's lifetime: dropping it destroys the object and the
    /// compositor stops blurring.
    pub background_effect: Option<BackgroundEffect>,
    /// The KDE blur handle, for compositors that implement that protocol
    /// instead. Held for the same reason.
    pub blur: Option<OrgKdeKwinBlur>,
    /// Compositor-drawn shadow handle, held for the popup's lifetime.
    pub shadow: Option<ShadowController>,
    /// Corner radius handle, held for the popup's lifetime.
    pub corner_radius: Option<CornerRadiusController>,
    /// Space a `window_geometry` left around the card, kept as the surface resizes.
    pub padding: Option<PopupPadding>,
    /// What placed the popup, to place it again when it resizes.
    pub placement: PopupPlacement,
    /// The last `xdg_popup.reposition` token sent.
    pub repositions: u32,
}

/// Space around a popup's card on its surface, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PopupPadding {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PopupPadding {
    /// The padding `window_geometry` leaves on a `size` surface; `None` without one.
    pub fn around(size: (u32, u32), window_geometry: Option<(i32, i32, i32, i32)>) -> Option<Self> {
        let (x, y, width, height) = window_geometry?;
        Some(Self {
            left: x,
            top: y,
            right: size.0 as i32 - x - width,
            bottom: size.1 as i32 - y - height,
        })
    }
}

/// The card on a `size` surface with `padding` around it, as surface-local `(x, y, width,
/// height)`: the window geometry, the region blurred behind it, and on a padded surface the input
/// region.
pub fn card_rect(size: (u32, u32), padding: Option<PopupPadding>) -> (i32, i32, i32, i32) {
    let padding = padding.unwrap_or_default();
    (
        padding.left,
        padding.top,
        (size.0 as i32 - padding.left - padding.right).max(0),
        (size.1 as i32 - padding.top - padding.bottom).max(0),
    )
}

/// The positioner rules a popup was placed by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PopupPlacement {
    pub anchor_rect: (i32, i32, i32, i32),
    pub anchor: PopupAnchor,
    pub gravity: PopupGravity,
    pub offset: (i32, i32),
    pub constraint_adjustment: u32,
}

impl From<&PopupSettings> for PopupPlacement {
    fn from(settings: &PopupSettings) -> Self {
        Self {
            anchor_rect: settings.anchor_rect,
            anchor: settings.anchor,
            gravity: settings.gravity,
            offset: settings.offset,
            constraint_adjustment: settings.constraint_adjustment,
        }
    }
}

impl PopupPlacement {
    /// A positioner placing a card of `size` by these rules.
    ///
    /// The size is the card's, not the surface's, so the compositor keeps what is visible on
    /// screen when it slides or flips the popup.
    pub fn positioner(&self, xdg_shell: &XdgShell, size: (i32, i32)) -> Option<XdgPositioner> {
        let positioner = XdgPositioner::new(xdg_shell).ok()?;
        positioner.set_size(size.0, size.1);
        let (x, y, width, height) = self.anchor_rect;
        positioner.set_anchor_rect(x, y, width, height);
        positioner.set_anchor(self.anchor.to_xdg());
        positioner.set_gravity(self.gravity.to_xdg());
        positioner.set_offset(self.offset.0, self.offset.1);
        // Placed again when the parent moves.
        positioner.set_reactive();
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::from_bits_truncate(self.constraint_adjustment),
        );
        Some(positioner)
    }
}

impl PopupState {
    /// Create a new popup state.
    pub fn new(
        popup: Popup,
        parent_id: WindowId,
        size: LogicalSize<u32>,
        viewport: Option<WpViewport>,
    ) -> Self {
        Self {
            popup,
            parent_id,
            parent_popup: None,
            grabbed: false,
            size,
            configured: false,
            viewport,
            tooltip: None,
            background_effect: None,
            blur: None,
            shadow: None,
            corner_radius: None,
            padding: None,
            placement: PopupPlacement::default(),
            repositions: 0,
        }
    }

    /// Tells the compositor where the card is on the surface at its current size: the window
    /// geometry, the region blurred behind it and, on a padded surface, the input region.
    ///
    /// Double-buffered, so it takes effect with the surface's next commit.
    pub fn send_card(&self, compositor: &CompositorState) {
        let (x, y, width, height) = card_rect((self.size.width, self.size.height), self.padding);
        self.popup.xdg_surface().set_window_geometry(x, y, width, height);
        let card = || {
            let region = Region::new(compositor).ok();
            match &region {
                Some(region) => region.add(x, y, width, height),
                None => tracing::warn!("Could not build a region for the popup's card"),
            }
            region
        };
        // A click on the padding lands outside the popup, and so dismisses it.
        if self.padding.is_some() {
            if let Some(region) = card() {
                self.popup.wl_surface().set_input_region(Some(region.wl_region()));
            }
        }
        if let Some(effect) = &self.background_effect {
            if let Some(region) = card() {
                effect.set_blur_region(Some(region.wl_region()));
            }
        }
        if let Some(blur) = &self.blur {
            if let Some(region) = card() {
                blur.set_region(Some(region.wl_region()));
            }
            blur.commit();
        }
    }

    /// Asks the compositor to place the popup again by its placement, for its card's current
    /// size: one flipped above its anchor stays against it, and one that grew stays on screen.
    ///
    /// The compositor answers with a configure. Needs xdg-shell 3; does nothing before.
    pub fn reposition(&mut self, xdg_shell: &XdgShell) {
        if self.popup.xdg_popup().version() < 3 {
            return;
        }
        let (_, _, width, height) = card_rect((self.size.width, self.size.height), self.padding);
        let Some(positioner) = self.placement.positioner(xdg_shell, (width, height)) else {
            return;
        };
        self.repositions = self.repositions.wrapping_add(1);
        self.popup.reposition(&positioner, self.repositions);
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

/// A popup the compositor dismissed, kept alive until the app has seen its `Done`.
///
/// Generic over the state it holds, so the grace-window bookkeeping can be exercised without a
/// Wayland connection to build a [`PopupState`] with.
pub struct DismissedPopup<S = PopupState> {
    pub id: PopupId,
    pub state: S,
    /// Its `Done` has been taken, so the next take may drop it.
    pub reported: bool,
}

/// Drops the dismissed popups whose `Done` an earlier take handed out, and marks the rest as
/// handed out by this one.
///
/// The released popups come back in dismissal order, children ahead of parents, so dropping the
/// vec front to back destroys them in the order xdg-shell requires.
pub(crate) fn release_dismissed<S>(
    dismissed: &mut Vec<DismissedPopup<S>>,
) -> Vec<DismissedPopup<S>> {
    let (released, pending): (Vec<_>, Vec<_>) =
        std::mem::take(dismissed).into_iter().partition(|popup| popup.reported);
    *dismissed = pending;
    for popup in dismissed.iter_mut() {
        popup.reported = true;
    }
    released
}

/// What destroying a popup does to each member of its subtree.
pub(crate) struct DestroyPlan {
    /// Live popups to destroy now, children ahead of parents.
    pub destroy: Vec<PopupId>,
    /// Dismissed popups the walk leaves where they are.
    pub held: Vec<PopupId>,
}

/// Splits `id`'s subtree into what destroying `id` takes with it and what it must leave alone.
///
/// A popup in `dismissed` is inside the grace window its `Done` opened: the app can still be
/// presenting to a surface it has not yet learned is gone, so an ancestor being destroyed must
/// not cut that short. It is left to [`release_dismissed`], which drops it a batch later, by
/// which point the app has had its iteration to stop drawing.
pub(crate) fn plan_destroy_tree(
    links: impl IntoIterator<Item = (PopupId, Option<PopupId>)>,
    dismissed: &[PopupId],
    id: PopupId,
) -> DestroyPlan {
    let mut plan = DestroyPlan { destroy: Vec::new(), held: Vec::new() };
    for popup in subtree_leaf_first(links, id) {
        if dismissed.contains(&popup) {
            plan.held.push(popup);
        } else {
            plan.destroy.push(popup);
        }
    }
    plan
}

/// `id` and every popup below it, each child ahead of its parent: the order xdg-shell requires
/// popups be destroyed in. Siblings are sorted by id only to keep the order deterministic.
///
/// `links` pairs each popup with the popup it is parented to.
pub(crate) fn subtree_leaf_first(
    links: impl IntoIterator<Item = (PopupId, Option<PopupId>)>,
    id: PopupId,
) -> Vec<PopupId> {
    let links: Vec<_> = links.into_iter().collect();
    // Breadth-first from `id`, so the reversal puts every child ahead of its parent.
    let mut order = vec![id];
    let mut next = 0;
    while let Some(&parent) = order.get(next) {
        // `order.contains` also stops a malformed cycle.
        let mut children: Vec<_> = links
            .iter()
            .filter(|(child, link)| *link == Some(parent) && !order.contains(child))
            .map(|(child, _)| *child)
            .collect();
        children.sort_unstable_by_key(|child| child.0);
        order.extend(children);
        next += 1;
    }
    order.reverse();
    order
}

/// Whether `parent` holds the topmost grab, the only popup xdg-shell lets a grabbing popup be
/// parented to. Grabs stack per seat, so that is the newest popup that grabbed.
///
/// `popups` pairs each live popup with whether it grabbed. Ids grow with creation, which is when
/// winit grabs.
pub(crate) fn holds_topmost_grab(
    popups: impl IntoIterator<Item = (PopupId, bool)>,
    parent: PopupId,
) -> bool {
    popups.into_iter().filter(|&(_, grabbed)| grabbed).map(|(id, _)| id.0).max() == Some(parent.0)
}

/// Popup event sent to the application.
#[derive(Debug, Clone)]
pub enum PopupEvent {
    /// Popup has been configured and is ready for content.
    Configure { id: PopupId, width: u32, height: u32 },
    /// Popup was dismissed (closed by compositor or user clicking outside), or
    /// destroyed because a popup it was parented to was.
    Done { id: PopupId },
    /// Pointer entered the popup surface.
    PointerEnter { id: PopupId, x: f64, y: f64 },
    /// Pointer left the popup surface.
    PointerLeave { id: PopupId },
    /// Pointer moved on the popup surface.
    PointerMotion { id: PopupId, x: f64, y: f64 },
    /// Pointer button pressed/released on the popup surface.
    PointerButton { id: PopupId, button: u32, pressed: bool },
    /// Keyboard input on the popup surface while it has keyboard focus: `Focused`,
    /// `ModifiersChanged` and `KeyboardInput`, as a window receives them.
    Window { id: PopupId, event: WindowEvent },
}

#[cfg(test)]
mod tests {
    use super::{
        card_rect, holds_topmost_grab, plan_destroy_tree, release_dismissed, subtree_leaf_first,
        DismissedPopup, PopupId, PopupPadding,
    };

    // A context menu measured with an Open and an Open With row, which then
    // lost one when no app turned out to open the file: 36px shorter.
    const OPENED: (u32, u32) = (280, 356);
    const SHRUNK: (u32, u32) = (280, 320);

    #[test]
    fn unpadded_card_is_the_whole_surface_at_every_size() {
        assert_eq!(card_rect(OPENED, None), (0, 0, 280, 356));
        assert_eq!(
            card_rect(SHRUNK, None),
            (0, 0, 280, 320),
            "the blur and window geometry follow the surface down"
        );
        assert_eq!(card_rect((280, 400), None), (0, 0, 280, 400), "and up");
    }

    #[test]
    fn padding_stays_put_as_the_surface_resizes() {
        // 20px of shadow padding all round, the card inside it.
        let padding = PopupPadding::around((320, 396), Some((20, 20, 280, 356)));
        assert_eq!(padding, Some(PopupPadding { left: 20, top: 20, right: 20, bottom: 20 }));
        assert_eq!(card_rect((320, 396), padding), (20, 20, 280, 356));
        assert_eq!(card_rect((320, 360), padding), (20, 20, 280, 320));
    }

    #[test]
    fn uneven_padding_keeps_each_side() {
        // A shadow offset down: more room below than above.
        let padding = PopupPadding::around((320, 406), Some((20, 10, 280, 356)));
        assert_eq!(card_rect((320, 370), padding), (20, 10, 280, 320));
    }

    #[test]
    fn no_window_geometry_means_no_padding() {
        assert_eq!(PopupPadding::around(OPENED, None), None);
    }

    #[test]
    fn a_surface_smaller_than_its_padding_has_an_empty_card() {
        let padding = PopupPadding::around((320, 396), Some((20, 20, 280, 356)));
        assert_eq!(card_rect((30, 30), padding), (20, 20, 0, 0));
    }

    fn ids(raw: &[u64]) -> Vec<PopupId> {
        raw.iter().copied().map(PopupId).collect()
    }

    fn links(raw: &[(u64, Option<u64>)]) -> Vec<(PopupId, Option<PopupId>)> {
        raw.iter().map(|&(id, parent)| (PopupId(id), parent.map(PopupId))).collect()
    }

    #[test]
    fn toplevel_popup_is_only_itself() {
        let links = links(&[(1, None), (2, None), (3, None)]);
        assert_eq!(subtree_leaf_first(links, PopupId(2)), ids(&[2]));
    }

    #[test]
    fn chain_destroys_deepest_first() {
        let links = links(&[(1, None), (2, Some(1)), (3, Some(2))]);
        assert_eq!(subtree_leaf_first(links, PopupId(1)), ids(&[3, 2, 1]));
    }

    #[test]
    fn branches_put_children_before_parents() {
        // 1 -> {2 -> 3, 4}; input order must not matter.
        let links = links(&[(4, Some(1)), (3, Some(2)), (1, None), (2, Some(1))]);
        assert_eq!(subtree_leaf_first(links, PopupId(1)), ids(&[3, 4, 2, 1]));
    }

    fn grabs(raw: &[(u64, bool)]) -> Vec<(PopupId, bool)> {
        raw.iter().map(|&(id, grabbed)| (PopupId(id), grabbed)).collect()
    }

    #[test]
    fn non_grabbing_or_unknown_parent_holds_no_grab() {
        let popups = grabs(&[(1, false)]);
        assert!(!holds_topmost_grab(popups.clone(), PopupId(1)));
        assert!(!holds_topmost_grab(popups, PopupId(2)));
    }

    #[test]
    fn grabbed_child_blocks_a_sibling_grab() {
        // Menu 1 grabbed, then submenu 2 under it: a second grabbing child of 1 is fatal.
        let popups = grabs(&[(1, true), (2, true)]);
        assert!(!holds_topmost_grab(popups.clone(), PopupId(1)));
        assert!(holds_topmost_grab(popups, PopupId(2)));
    }

    #[test]
    fn deepest_grab_of_a_chain_is_topmost() {
        let popups = grabs(&[(3, true), (1, true), (2, true)]);
        assert!(holds_topmost_grab(popups.clone(), PopupId(3)));
        assert!(!holds_topmost_grab(popups, PopupId(2)));
    }

    #[test]
    fn non_grabbing_child_leaves_the_grab_with_its_parent() {
        // The fly-out case: menu 1 grabbed, fly-out 2 created with grab: false.
        let popups = grabs(&[(1, true), (2, false)]);
        assert!(holds_topmost_grab(popups, PopupId(1)));
    }

    #[test]
    fn subtree_excludes_parent_and_siblings() {
        let links = links(&[(1, None), (2, Some(1)), (3, Some(2)), (4, Some(1)), (5, None)]);
        assert_eq!(subtree_leaf_first(links, PopupId(2)), ids(&[3, 2]));
    }

    #[test]
    fn cycle_terminates() {
        let links = links(&[(1, Some(2)), (2, Some(1))]);
        assert_eq!(subtree_leaf_first(links, PopupId(1)), ids(&[2, 1]));
    }

    /// Handling submenu 2's `Done` closes parent menu 1. Parent destruction must preserve
    /// the submenu's grace window because the app may still be presenting to its surface.
    #[test]
    fn destroying_an_ancestor_keeps_a_dismissed_child_its_grace_window() {
        let mut dismissed =
            vec![DismissedPopup { id: PopupId(2), state: "submenu", reported: false }];

        // The take that hands out 2's `Done` drops nothing.
        assert!(release_dismissed(&mut dismissed).is_empty());

        // Closing the menu destroys 1 now and leaves 2 to finish its window.
        let plan = plan_destroy_tree(links(&[(1, None), (2, Some(1))]), &ids(&[2]), PopupId(1));
        assert_eq!(plan.destroy, ids(&[1]));
        assert_eq!(plan.held, ids(&[2]));

        // Which it does on the next take, and not before.
        let released = release_dismissed(&mut dismissed);
        assert_eq!(released.iter().map(|popup| popup.id).collect::<Vec<_>>(), ids(&[2]));
        assert!(dismissed.is_empty());
    }

    /// Live descendants still go with their ancestor, and still go first.
    #[test]
    fn destroying_an_ancestor_takes_its_live_children_with_it() {
        let plan = plan_destroy_tree(
            links(&[(1, None), (2, Some(1)), (3, Some(2))]),
            &ids(&[3]),
            PopupId(1),
        );
        assert_eq!(plan.destroy, ids(&[2, 1]));
        assert_eq!(plan.held, ids(&[3]));
    }
}
