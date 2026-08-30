//! The camera-profile browser (ADR 0089) (ADR 0045 §4).
//!
//! Mirrors the Camera profile group of `ui/panels/develop.slint`. Its whole
//! job is showing what a profile *does* before it is chosen: every row is
//! the open photo rendered through that profile, through `preview_live` —
//! the "a view, never a revision" path the soft proof and the preset trial
//! already use. Nothing is committed until a row is clicked.
//!
//! Lived in `wiring/map.rs` until the browser was written, which was the
//! wrong module for it by ADR 0045 §4's own rule: these are `DevelopState`
//! callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::ui::{DevelopState, ProfileRow, StudioWindow, Tr};
use leyline_sdk::{CameraProfile, Param, PreviewKind, Value, VersionId};
use slint::{ComponentHandle, Global, Model, ModelRc, SharedString, VecModel};

pub(super) fn wire_profiles(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_load_profiles(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if let Err(error) = load_profiles(&mut app, &window) {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_choose_profile(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let Some(path) = usize::try_from(index)
                .ok()
                .and_then(|index| app.profile_paths.get(index))
                .cloned()
            else {
                return;
            };
            let referenced = (|| {
                // The checksum comes from the library's own listing, never
                // from a path anyone typed — the same rule the import below
                // follows, for the same reason.
                let profile = if path.is_empty() {
                    None
                } else {
                    let listed = app.library.camera_profiles()?;
                    let Some(found) = listed.into_iter().find(|p| p.relative_path == path) else {
                        return Ok(());
                    };
                    Some(CameraProfile {
                        enabled: true,
                        path: found.relative_path,
                        checksum: found.checksum,
                    })
                };
                let mut session = app.library.edit(version)?;
                session.set(Param::CameraProfile, Value::CameraProfile(profile))?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = referenced
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
                .and_then(|()| load_profiles(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DevelopState::get(window).on_browse_camera_profile(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let Some(path) = rfd::FileDialog::new()
                .add_filter("DCP camera profile", &["dcp"])
                .pick_file()
            else {
                return;
            };
            let referenced = (|| {
                // Importing copies the file into `Profiles/Camera/` and
                // hands back the checksum of the bytes it copied — the
                // reference is never built from a path the user typed.
                let imported = app.library.import_camera_profile(&path)?;
                let mut session = app.library.edit(version)?;
                session.set(
                    Param::CameraProfile,
                    Value::CameraProfile(Some(CameraProfile {
                        enabled: true,
                        path: imported.relative_path,
                        checksum: imported.checksum,
                    })),
                )?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = referenced
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
}

/// Builds the browser's rows for the photo open in develop (ADR 0089 §1):
/// the library's profiles, each rendering that photo.
///
/// Idempotent and cheap on a second call for the same photo — the renders
/// are what cost, and they are kept while the photo stays open. Clearing
/// `profile_thumbs_for` is how a caller says "build them again".
pub(crate) fn load_profiles(app: &mut App, window: &StudioWindow) -> Result<(), String> {
    let Some((asset, version)) = app.develop else {
        return Ok(());
    };
    if app.profile_thumbs_for == Some(asset) {
        // Only the selection can have moved; the pictures have not.
        mark_selected(app, window);
        return Ok(());
    }
    let listed = app.library.camera_profiles().map_err(|e| e.to_string())?;
    let base = settings_of(app, version)?;

    // "No profile" first, and it is a rendering like the others — the
    // decoder's own colours are a choice, not the absence of one
    // (ADR 0089 §2).
    let mut rows: Vec<ProfileRow> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    let mut candidates: Vec<(String, Option<CameraProfile>)> =
        vec![(Tr::get(window).invoke_no_camera_profile().to_string(), None)];
    candidates.extend(listed.iter().map(|profile| {
        (
            display_name(&profile.relative_path),
            Some(CameraProfile {
                enabled: true,
                path: profile.relative_path.clone(),
                checksum: profile.checksum.clone(),
            }),
        )
    }));

    for (name, profile) in candidates {
        let mut trial = base.clone();
        let path = profile
            .as_ref()
            .map_or_else(String::new, |profile| profile.path.clone());
        trial.camera_profile = profile;
        // A profile that fails to render still gets its row, with no
        // picture: a broken `.dcp` should be visible and refusable, not
        // absent from a list the user knows they imported into.
        let thumbnail = app
            .library
            .preview_live(asset, PreviewKind::Thumbnail, &trial)
            .map(|image| crate::models::rgb8_to_slint_image(&image))
            .unwrap_or_default();
        rows.push(ProfileRow {
            name: SharedString::from(name.as_str()),
            path: SharedString::from(path.as_str()),
            thumbnail,
            selected: false,
        });
        paths.push(path);
    }
    app.profile_paths = paths;
    app.profile_thumbs_for = Some(asset);
    DevelopState::get(window).set_profile_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
    mark_selected(app, window);
    Ok(())
}

/// The head settings of `version`, to render each candidate against the
/// photo as it currently is rather than against a neutral one.
fn settings_of(app: &App, version: VersionId) -> Result<leyline_sdk::Settings, String> {
    let session = app.library.edit(version).map_err(|e| e.to_string())?;
    Ok(session.settings().clone())
}

/// What a `.dcp`'s row is called: its file name, without the folder the
/// panel has no room for and without the extension every entry shares.
fn display_name(relative_path: &str) -> String {
    relative_path
        .rsplit('/')
        .next()
        .unwrap_or(relative_path)
        .trim_end_matches(".dcp")
        .to_owned()
}

/// Moves the browser's highlight to whichever row the revision references.
fn mark_selected(app: &App, window: &StudioWindow) {
    let current = current_path(app);
    let rows = DevelopState::get(window).get_profile_rows();
    for (index, path) in app.profile_paths.iter().enumerate() {
        if let Some(mut row) = rows.row_data(index) {
            row.selected = *path == current;
            rows.set_row_data(index, row);
        }
    }
}

/// The profile the open revision references, empty for none.
fn current_path(app: &App) -> String {
    let Some((_, version)) = app.develop else {
        return String::new();
    };
    settings_of(app, version)
        .ok()
        .and_then(|settings| settings.camera_profile.map(|profile| profile.path))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::display_name;

    /// The panel has room for a name, not for `Profiles/Camera/…`, and the
    /// prefix is the same for every imported profile anyway.
    #[test]
    fn a_row_is_named_after_the_file_and_not_the_folder() {
        assert_eq!(
            display_name("Profiles/Camera/Canon EOS 5D Mark III.dcp"),
            "Canon EOS 5D Mark III"
        );
        assert_eq!(display_name("bare.dcp"), "bare");
        assert_eq!(display_name(""), "");
    }
}
