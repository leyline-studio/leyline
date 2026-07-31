//! Everything that binds a Slint global to the engine (ADR 0045 §4).
//!
//! One module per global, mirroring `ui/state/` and `ui/panels/` one for one:
//! a module reaches its own global through `Global::<T>::get(&window)` and
//! leaves the others alone.

pub(crate) mod collections;
pub(crate) mod develop;
pub(crate) mod dialogs;
pub(crate) mod filters;
pub(crate) mod folders;
pub(crate) mod grid;
pub(crate) mod keywords;
pub(crate) mod library;
pub(crate) mod map;
pub(crate) mod presets;
