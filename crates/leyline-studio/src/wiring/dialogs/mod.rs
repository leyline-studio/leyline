//! Wires `DialogState`: import, export, print, contact sheets, tethering,
//! watched folders, roots and renaming (ADR 0045 §4).
//!
//! One submodule per dialog, mirroring `ui/dialogs/` one for one. This file
//! only registers them all.

pub(crate) mod contact_sheet;
pub(crate) mod export;
pub(crate) mod import;
pub(crate) mod preferences;
pub(crate) mod print;
pub(crate) mod rename;
pub(crate) mod roots;
pub(crate) mod tether;
pub(crate) mod watch;

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::App;
use crate::ui::StudioWindow;

/// Wires every dialog `DialogState` can open.
pub(crate) fn wire_dialogs(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    import::wire_import(app, window);
    export::wire_export(app, window);
    print::wire_print(app, window);
    contact_sheet::wire_contact_sheet(app, window);
    tether::wire_tether(app, window);
    watch::wire_watch(app, window);
    roots::wire_roots(app, window);
    rename::wire_rename(app, window);
}
