//! Handling of the `xdg-toplevel-icon-v1` protocol: lets a client set (and
//! change at runtime) the icon the compositor shows for a toplevel — e.g. in a
//! server-side decoration header — independently of the application icon.

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_shm;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use sctk::reexports::protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use sctk::reexports::protocols::xdg::toplevel_icon::v1::client::{
    xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1, xdg_toplevel_icon_v1::XdgToplevelIconV1,
};
use sctk::shm::slot::{Buffer, SlotPool};
use sctk::shm::Shm;
use tracing::warn;

use crate::platform_impl::wayland::state::WinitState;

/// Bound `xdg_toplevel_icon_manager_v1` global.
#[derive(Debug)]
pub struct ToplevelIconManager {
    manager: XdgToplevelIconManagerV1,
}

impl ToplevelIconManager {
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let manager = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { manager })
    }

    /// Build a per-window icon setter, backed by its own (small) shm pool.
    pub fn icon_for_window(&self, shm: &Shm) -> Option<WindowToplevelIcon> {
        let pool = SlotPool::new(1, shm).ok()?;
        Some(WindowToplevelIcon { manager: self.manager.clone(), pool, current: None })
    }
}

/// Per-window holder for the toplevel icon. Keeps the committed icon object and
/// its backing buffer alive until replaced — the compositor references the
/// buffer for as long as it is the current icon.
#[derive(Debug)]
pub struct WindowToplevelIcon {
    manager: XdgToplevelIconManagerV1,
    pool: SlotPool,
    current: Option<(XdgToplevelIconV1, Buffer)>,
}

impl WindowToplevelIcon {
    /// Set or clear the toplevel icon from square RGBA pixel data. The change is
    /// double-buffered with the surface, so it applies on the next commit (the
    /// window's next frame).
    pub fn set(
        &mut self,
        toplevel: &XdgToplevel,
        icon: Option<(&[u8], u32, u32)>,
        queue_handle: &QueueHandle<WinitState>,
    ) {
        let Some((rgba, width, height)) = icon else {
            self.manager.set_icon(toplevel, None);
            self.current = None;
            return;
        };

        if width != height || width == 0 {
            warn!("xdg-toplevel-icon requires a non-empty square icon; got {width}x{height}");
            return;
        }

        let (w, h) = (width as i32, height as i32);
        let stride = w * 4;

        // Scope the canvas borrow so the pool is free again before we touch
        // other fields; `Buffer` itself owns its slot, so it outlives the scope.
        let buffer = {
            let (buffer, canvas) =
                match self.pool.create_buffer(w, h, stride, wl_shm::Format::Argb8888) {
                    Ok(pair) => pair,
                    Err(err) => {
                        warn!("failed to allocate xdg-toplevel-icon buffer: {err:?}");
                        return;
                    },
                };
            // winit icons are RGBA8888; wl_shm ARGB8888 is little-endian, i.e.
            // bytes laid out B, G, R, A.
            for (dst, src) in canvas.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
                dst[0] = src[2];
                dst[1] = src[1];
                dst[2] = src[0];
                dst[3] = src[3];
            }
            buffer
        };

        let xdg_icon = self.manager.create_icon(queue_handle, GlobalData);
        xdg_icon.add_buffer(buffer.wl_buffer(), 1);
        self.manager.set_icon(toplevel, Some(&xdg_icon));
        // Replacing `current` drops the previous icon + buffer, which is safe
        // now that the new one has been committed as the toplevel's icon.
        self.current = Some((xdg_icon, buffer));
    }
}

impl Dispatch<XdgToplevelIconManagerV1, GlobalData, WinitState> for ToplevelIconManager {
    fn event(
        _: &mut WinitState,
        _: &XdgToplevelIconManagerV1,
        _: <XdgToplevelIconManagerV1 as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        // `icon_size` / `done` are advisory preferred sizes; ignore them.
    }
}

impl Dispatch<XdgToplevelIconV1, GlobalData, WinitState> for ToplevelIconManager {
    fn event(
        _: &mut WinitState,
        _: &XdgToplevelIconV1,
        _: <XdgToplevelIconV1 as Proxy>::Event,
        _: &GlobalData,
        _: &Connection,
        _: &QueueHandle<WinitState>,
    ) {
        // The icon object emits no events.
    }
}

delegate_dispatch!(WinitState: [XdgToplevelIconManagerV1: GlobalData] => ToplevelIconManager);
delegate_dispatch!(WinitState: [XdgToplevelIconV1: GlobalData] => ToplevelIconManager);
