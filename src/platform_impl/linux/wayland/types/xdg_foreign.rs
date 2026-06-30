//! xdg-foreign (`zxdg_importer_v2`) client support.
//!
//! This lets a window become a child of a *foreign* toplevel exported by
//! another client. The primary user is an xdg-desktop-portal `FileChooser`
//! backend: the portal hands the backend a `parent_window` handle (the
//! requesting application exported its toplevel via `zxdg_exporter_v2`), and the
//! backend imports that handle here and calls `set_parent_of` on the picker
//! surface. The compositor then treats the picker as a dialog of the requesting
//! application and places it over that window, on the correct output, instead of
//! dropping it on some default output.

use std::sync::{Arc, Mutex};

use sctk::globals::GlobalData;
use sctk::reexports::client::globals::{BindError, GlobalList};
use sctk::reexports::client::protocol::wl_surface::WlSurface;
use sctk::reexports::client::{delegate_dispatch, Connection, Dispatch, Proxy, QueueHandle};
use sctk::reexports::protocols::xdg::foreign::zv2::client::{
    zxdg_imported_v2::{self, ZxdgImportedV2},
    zxdg_importer_v2::ZxdgImporterV2,
};
use tracing::warn;

use crate::platform_impl::wayland::state::WinitState;

/// State shared with an imported toplevel handle.
#[derive(Debug)]
pub struct ImportedData {
    /// Whether the import is still valid. The compositor sends `destroyed`
    /// when the exported toplevel goes away or the handle was never valid.
    pub valid: bool,
}

/// Wrapper around the `zxdg_importer_v2` global.
#[derive(Debug, Clone)]
pub struct XdgForeign {
    importer: ZxdgImporterV2,
}

impl XdgForeign {
    /// Try to bind the `zxdg_importer_v2` global.
    pub fn new(
        globals: &GlobalList,
        queue_handle: &QueueHandle<WinitState>,
    ) -> Result<Self, BindError> {
        let importer = globals.bind(queue_handle, 1..=1, GlobalData)?;
        Ok(Self { importer })
    }

    /// Import a foreign toplevel by its xdg-foreign `handle` and make it the
    /// parent of `surface` (which must already have the `xdg_toplevel` role).
    ///
    /// The returned [`ImportedToplevel`] must be kept alive for as long as the
    /// parent relationship should hold; dropping it removes the relationship.
    pub fn import_parent(
        &self,
        handle: &str,
        surface: &WlSurface,
        queue_handle: &QueueHandle<WinitState>,
    ) -> ImportedToplevel {
        let data = Arc::new(Mutex::new(ImportedData { valid: true }));
        let imported =
            self.importer.import_toplevel(handle.to_string(), queue_handle, data.clone());
        imported.set_parent_of(surface);
        ImportedToplevel { imported, data }
    }
}

/// An imported foreign toplevel used as a parent. Dropping it destroys the
/// import, which removes the parent relationship in the compositor.
#[derive(Debug)]
pub struct ImportedToplevel {
    imported: ZxdgImportedV2,
    data: Arc<Mutex<ImportedData>>,
}

impl ImportedToplevel {
    /// Re-assign the imported toplevel as the parent of `surface`.
    pub fn set_parent_of(&self, surface: &WlSurface) {
        if self.data.lock().unwrap().valid {
            self.imported.set_parent_of(surface);
        }
    }

    /// Whether the import is still valid (not yet `destroyed` by the compositor).
    pub fn is_valid(&self) -> bool {
        self.data.lock().unwrap().valid
    }
}

impl Drop for ImportedToplevel {
    fn drop(&mut self) {
        self.imported.destroy();
    }
}

impl Dispatch<ZxdgImporterV2, GlobalData, WinitState> for XdgForeign {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZxdgImporterV2,
        _event: <ZxdgImporterV2 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        // The importer has no events.
    }
}

impl Dispatch<ZxdgImportedV2, Arc<Mutex<ImportedData>>, WinitState> for XdgForeign {
    fn event(
        _state: &mut WinitState,
        _proxy: &ZxdgImportedV2,
        event: <ZxdgImportedV2 as Proxy>::Event,
        data: &Arc<Mutex<ImportedData>>,
        _conn: &Connection,
        _qhandle: &QueueHandle<WinitState>,
    ) {
        if let zxdg_imported_v2::Event::Destroyed = event {
            warn!("xdg-foreign: imported parent handle was invalidated by the compositor");
            data.lock().unwrap().valid = false;
        }
    }
}

delegate_dispatch!(WinitState: [ZxdgImporterV2: GlobalData] => XdgForeign);
delegate_dispatch!(WinitState: [ZxdgImportedV2: Arc<Mutex<ImportedData>>] => XdgForeign);
