//! Local adjustment edits: the tools that trace a mask and the sliders that
//! re-parameterize it (ADR 0029, ADR 0048, ADR 0049, ADR 0045 §4).
//!
//! The rules deciding what a gesture writes live in [`crate::masks`], free of
//! any Slint type; this module only moves values across the boundary and
//! commits, exactly as [`super::spot`] does for spot removal.

use std::cell::RefCell;
use std::rc::Rc;

use super::refresh_develop;
use crate::app::{App, report_error};
use crate::masks;
use crate::ui::{MaskState, StudioWindow};
use leyline_sdk::{Param, Value};
use slint::{ComponentHandle, Global, ModelRc, VecModel};

/// Asks for an image file, converts it to a coverage and stores it in the
/// library, returning the mask that references it (ADR 0070 §7).
///
/// `Ok(None)` is a dismissed picker — the one outcome that is neither a mask
/// nor a failure.
fn pick_coverage(app: &App) -> Result<Option<leyline_sdk::Mask>, String> {
    let Some(file) = rfd::FileDialog::new()
        .add_filter("Image", &["png", "tif", "tiff", "jpg", "jpeg", "webp"])
        .pick_file()
    else {
        return Ok(None);
    };
    import_coverage(&app.library, &file).map(Some)
}

/// [`pick_coverage`] with the file already chosen — everything the picker
/// does *after* the picker, so it can be tested without a desktop portal.
fn import_coverage(
    library: &leyline_sdk::Library,
    file: &std::path::Path,
) -> Result<leyline_sdk::Mask, String> {
    let image = image::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    // The protocol's own reader (ADR 0105 §1): a second client must not
    // reimplement what a detector's output means.
    let (width, height, samples) = leyline_sdk::coverage_from_image(&image);
    library
        .store_mask_coverage(width, height, &samples)
        .map_err(|e| e.to_string())
}

/// Runs one detection and stores what comes back (ADR 0073).
///
/// The detector is handed the *cached preview file* rather than a fresh
/// temporary render: it is already a PNG RGB 8-bit on disk, which is exactly
/// the contract's input, and the medium class (≤ 2048 px) gives a model more
/// to work with than the 1024 px the panel displays.
///
/// Blocking, like the file picker next to it: a detection takes seconds, and
/// `leyline-detect` caps it so a wedged executable cannot hold the interface
/// forever.
fn run_detection(app: &App, key: &str) -> Result<leyline_sdk::Mask, String> {
    let (source_id, detection) =
        crate::models::split_detection_key(key).ok_or_else(|| format!("malformed key {key}"))?;
    let source = leyline_sdk::discover()
        .into_iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| format!("no detector named {source_id} is installed"))?;
    let (asset, _) = app
        .develop
        .ok_or_else(|| "no photo in develop".to_owned())?;
    let preview = app
        .library
        .preview(asset, leyline_sdk::PreviewKind::Medium)
        .map_err(|e| e.to_string())?;
    // The protocol's own call (ADR 0105 §1): it makes the temporary file,
    // runs the detector and reads the answer, so this client handles none
    // of that plumbing and cannot disagree with the other one about it.
    let (width, height, samples) = leyline_sdk::detect_coverage(&source, detection, &preview.path)
        .map_err(|e| e.to_string())?;
    app.library
        .store_mask_coverage(width, height, &samples)
        .map_err(|e| e.to_string())
}

pub(super) fn wire_masks(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    // Discovered once, at wiring time: installing a detector is not
    // something that happens while the window is open, and a scan of a
    // config directory has no business running on every refresh. Empty is
    // the normal state, and then the panel shows no chip at all.
    MaskState::get(window).set_detections(slint::ModelRc::new(slint::VecModel::from(
        crate::models::detection_rows(&leyline_sdk::discover()),
    )));
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_add_mask(move |kind| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            // A stored coverage comes from a file, not from a gesture
            // (ADR 0070 §7): pick it, convert it, store it, and only then is
            // there a mask to add.
            let stored = if kind == "coverage" {
                match pick_coverage(&app) {
                    Ok(Some(mask)) => Some(mask),
                    // The picker was dismissed: not an error, just nothing.
                    Ok(None) => return,
                    Err(message) => {
                        report_error(&window, &message);
                        return;
                    }
                }
            } else {
                None
            };
            commit(&mut app, &window, |current| match &stored {
                Some(mask) => Some((
                    Param::LocalAdjustment(current.len()),
                    Value::LocalAdjustment(Some(masks::fresh_entry(mask.clone()))),
                )),
                None => masks::add_mask(kind.as_str(), current),
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_select_mask(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            MaskState::get(&window).set_selected_mask(index);
            let mut app = app.borrow_mut();
            if let Err(message) = refresh_develop(&mut app, &window) {
                report_error(&window, &message);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_detect_mask(move |key| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let mask = match run_detection(&app, key.as_str()) {
                Ok(mask) => mask,
                Err(message) => {
                    report_error(&window, &message);
                    return;
                }
            };
            // Neutral values: the detection chose where, the user chooses
            // what (ADR 0073 §4). Appended rather than retracing the
            // selected row — a detected coverage is not a re-drag of a
            // geometry.
            commit(&mut app, &window, |current| {
                Some((
                    Param::LocalAdjustment(current.len()),
                    Value::LocalAdjustment(Some(masks::fresh_entry(mask.clone()))),
                ))
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_toggle_overlay(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.show_mask_overlay = !app.show_mask_overlay;
            MaskState::get(&window).set_show_overlay(app.show_mask_overlay);
            // Repaint through the ordinary refresh: the overlay is composited
            // into the develop image, so there is nothing else to update.
            if let Err(message) = refresh_develop(&mut app, &window) {
                report_error(&window, &message);
            }
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_remove_mask(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            commit(&mut app, &window, |current| {
                masks::remove_mask(index, current)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_place_mask(
            move |selected, kind, px, py, mx, my, vw, vh, iw, ih| {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                let mut app = app.borrow_mut();
                let Some(mask) = masks::drag_geometry(
                    kind.as_str(),
                    (f64::from(px), f64::from(py)),
                    (f64::from(mx), f64::from(my)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                ) else {
                    return;
                };
                commit(&mut app, &window, |current| {
                    Some(masks::place_geometry(selected, mask, current))
                });
            },
        );
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_paint_mask_dab(move |selected, mx, my, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let defaults = match brush_defaults(
                MaskState::get(&window).get_brush_size_text().as_str(),
                MaskState::get(&window).get_brush_flow_text().as_str(),
                MaskState::get(&window).get_brush_hardness_text().as_str(),
            ) {
                Ok(defaults) => defaults,
                Err(error) => {
                    report_error(&window, &error);
                    return;
                }
            };
            let Some(stroke) = masks::dab(
                (f64::from(mx), f64::from(my)),
                (f64::from(vw), f64::from(vh)),
                (f64::from(iw), f64::from(ih)),
                defaults,
            ) else {
                return;
            };
            commit(&mut app, &window, |current| {
                Some(masks::paint_dab(selected, stroke, current))
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // A stroke begins: the pending list is emptied and takes its first
        // dab. The row it will land on is decided at the commit, from the
        // selection as it stands then — the same rule the single click had.
        MaskState::get(window).on_brush_begin(move |mx, my, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            app.brush_stroke.clear();
            extend_stroke(&mut app, &window, (mx, my), (vw, vh), (iw, ih));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_brush_extend(move |mx, my, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            if app.brush_stroke.is_empty() {
                // A move with no press behind it: nothing to extend.
                return;
            }
            extend_stroke(&mut app, &window, (mx, my), (vw, vh), (iw, ih));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // Release: one revision for the whole stroke. Committing per dab
        // would fill the history with a hundred entries nobody can navigate,
        // and undo would step back one dab at a time.
        MaskState::get(window).on_brush_end(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let stroke = std::mem::take(&mut app.brush_stroke);
            show_pending(&app, &window);
            if stroke.is_empty() {
                return;
            }
            let selected = MaskState::get(&window).get_selected_mask();
            commit(&mut app, &window, |current| {
                Some(masks::paint_stroke(selected, &stroke, current))
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_preview_mask(move |index, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            super::live_preview(&mut app, &window, |settings| {
                masks::edit_field(index, field.as_str(), f64::from(value), settings)
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // A handle of the selected geometry moved (ADR 0097): only the
        // geometry is rewritten, so the entry keeps its feather, its
        // inversion, its range band and its values.
        MaskState::get(window).on_drag_mask_handle(move |which, vx, vy, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let selected = MaskState::get(&window).get_selected_mask();
            commit(&mut app, &window, |entries| {
                let index = usize::try_from(selected).ok()?;
                let mut entry = entries.get(index)?.clone();
                entry.mask = masks::drag_handle(
                    which.as_str(),
                    (f64::from(vx), f64::from(vy)),
                    (f64::from(vw), f64::from(vh)),
                    (f64::from(iw), f64::from(ih)),
                    &entry.mask,
                )?;
                Some((
                    Param::LocalAdjustment(index),
                    Value::LocalAdjustment(Some(entry)),
                ))
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        // The range eyedropper (ADR 0093): sample under the click, then one
        // ordinary commit through the shared gesture path.
        MaskState::get(window).on_range_sample_click(move |kind, vx, vy, vw, vh, iw, ih| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((asset, _)) = app.develop else {
                return;
            };
            let selected = MaskState::get(&window).get_selected_mask();
            let Some((x, y)) = crate::develop::letterbox_unit(
                (f64::from(vx), f64::from(vy)),
                (f64::from(vw), f64::from(vh)),
                (f64::from(iw), f64::from(ih)),
            ) else {
                return;
            };
            let sample = match app.library.sample_range(asset, x, y) {
                Ok(sample) => sample,
                Err(error) => {
                    report_error(&window, &error.to_string());
                    return;
                }
            };
            commit(&mut app, &window, |entries| {
                masks::sample_field(
                    selected,
                    kind.as_str(),
                    (sample.luminance, sample.hue),
                    entries,
                )
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        MaskState::get(window).on_edit_mask(move |index, field, value| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let Some((_, version)) = app.develop else {
                return;
            };
            let committed = (|| {
                let mut session = app.library.edit(version)?;
                let Some((param, value)) =
                    masks::edit_field(index, field.as_str(), f64::from(value), session.settings())
                else {
                    return Ok(());
                };
                session.set(param, value)?;
                session.commit().map(|_| ())
            })();
            if let Err(error) = committed
                .map_err(|e| e.to_string())
                .and_then(|()| refresh_develop(&mut app, &window))
            {
                report_error(&window, &error);
            }
        });
    }
}

/// Adds a dab to the stroke in progress, if it is far enough from the last
/// one, and mirrors the stroke back to the panel so it draws as it is traced.
///
/// Nothing is rendered here: the coverage of a brush only exists once the
/// revision is committed, and asking the engine for one per mouse event would
/// be far slower than the pointer. What the photographer sees while tracing is
/// the outline, which is exactly what they are placing.
fn extend_stroke(
    app: &mut App,
    window: &StudioWindow,
    click: (f32, f32),
    view: (f32, f32),
    image: (f32, f32),
) {
    let defaults = match brush_defaults(
        MaskState::get(window).get_brush_size_text().as_str(),
        MaskState::get(window).get_brush_flow_text().as_str(),
        MaskState::get(window).get_brush_hardness_text().as_str(),
    ) {
        Ok(defaults) => defaults,
        Err(error) => {
            report_error(window, &error);
            return;
        }
    };
    let Some(stroke) = masks::dab(
        (f64::from(click.0), f64::from(click.1)),
        (f64::from(view.0), f64::from(view.1)),
        (f64::from(image.0), f64::from(image.1)),
        defaults,
    ) else {
        return;
    };
    if !masks::dab_is_far_enough(app.brush_stroke.last(), &stroke) {
        return;
    }
    app.brush_stroke.push(stroke);
    show_pending(app, window);
}

/// Sends the stroke in progress to the panel.
fn show_pending(app: &App, window: &StudioWindow) {
    let dabs: Vec<crate::ui::MaskDab> = app
        .brush_stroke
        .iter()
        .map(|stroke| crate::ui::MaskDab {
            x: stroke.x as f32,
            y: stroke.y as f32,
            radius: stroke.radius as f32,
        })
        .collect();
    MaskState::get(window).set_brush_pending(ModelRc::from(Rc::new(VecModel::from(dabs))));
}

/// Runs one local-adjustment decision against the current entries and commits
/// what it returns, then refreshes the develop view.
///
/// The four gestures share this shape — read the stored list, decide, commit,
/// refresh — and differ only in the decision, which is why it is a closure
/// here and a pure function in [`crate::masks`]. A decision returning `None`
/// (a degenerate drag, a row that no longer exists) commits nothing.
fn commit<F>(app: &mut App, window: &StudioWindow, decide: F)
where
    F: FnOnce(&[leyline_sdk::LocalAdjustment]) -> Option<(Param, Value)>,
{
    let Some((_, version)) = app.develop else {
        return;
    };
    let mut written = None;
    let committed = (|| {
        let mut session = app.library.edit(version)?;
        let Some((param, value)) = decide(&session.settings().local_adjustments) else {
            return Ok(());
        };
        // A gesture can create as well as modify, and what it created has to
        // become the selected row — otherwise the values of a mask that was
        // just traced would have nowhere to appear (ADR 0049 §2).
        written =
            masks::written_row(&param).filter(|_| !matches!(value, Value::LocalAdjustment(None)));
        session.set(param, value)?;
        session.commit().map(|_| ())
    })();
    if committed.is_ok() {
        if let Some(row) = written {
            MaskState::get(window).set_selected_mask(i32::try_from(row).unwrap_or(-1));
        }
    }
    if let Err(error) = committed
        .map_err(|e| e.to_string())
        .and_then(|()| refresh_develop(app, window))
    {
        report_error(window, &error);
    }
}

/// Parses the brush panel's size/flow/hardness percent fields into the
/// `[0, 1]` units [`leyline_sdk::BrushStroke`] stores — the defaults applied
/// to the *next* dab painted, part of no stored revision themselves, exactly
/// like [`super::spot::spot_defaults`].
pub(crate) fn brush_defaults(
    size: &str,
    flow: &str,
    hardness: &str,
) -> Result<(f64, f64, f64), String> {
    let size: f64 = size.parse().map_err(|_| format!("bad size {size:?}"))?;
    let flow: f64 = flow.parse().map_err(|_| format!("bad flow {flow:?}"))?;
    let hardness: f64 = hardness
        .parse()
        .map_err(|_| format!("bad hardness {hardness:?}"))?;
    Ok((size / 100.0, flow / 100.0, hardness / 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_defaults_converts_percent_to_unit_range() {
        assert_eq!(brush_defaults("8", "50", "40").unwrap(), (0.08, 0.5, 0.4));
    }

    #[test]
    fn brush_defaults_rejects_bad_input() {
        assert!(brush_defaults("wide", "50", "40").is_err());
        assert!(brush_defaults("8", "half", "40").is_err());
        assert!(brush_defaults("8", "50", "hard").is_err());
    }

    /// The glue between the file picker and the engine (ADR 0070 §7): a real
    /// image file becomes a stored mask that a revision can carry. The picker
    /// itself needs a desktop portal, so everything after it is tested here.
    #[test]
    fn an_image_file_becomes_a_stored_mask() {
        let dir = tempfile::tempdir().unwrap();
        let library = leyline_sdk::Library::create(&dir.path().join("Lib"), "Import").unwrap();

        // Half covered, half not — and opaque, so the luminance path.
        let mut source = image::RgbImage::new(4, 2);
        for y in 0..2 {
            for x in 0..4 {
                let shade = if x < 2 { 255 } else { 0 };
                source.put_pixel(x, y, image::Rgb([shade; 3]));
            }
        }
        let file = dir.path().join("mask.png");
        source.save(&file).unwrap();

        let mask = import_coverage(&library, &file).expect("a readable image imports");
        let leyline_sdk::Mask::Coverage { path, checksum } = &mask else {
            panic!("an imported mask is a stored coverage, got {mask:?}");
        };
        assert!(path.starts_with("Masks/"), "{path}");
        assert!(checksum.starts_with("blake3:"), "{checksum}");
        assert!(
            dir.path()
                .join("Lib")
                .join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
                .is_file(),
            "the file is written where the mask says"
        );

        // And it is a usable setting, not just a value: validation accepts it
        // on the stage version that can express it.
        let settings = leyline_sdk::Settings {
            stages: leyline_sdk::StageVersions::from([("local_adjustments".to_owned(), 3)]),
            local_adjustments: vec![masks::fresh_entry(mask)],
            ..leyline_sdk::Settings::default()
        };
        settings.validate().expect("an imported mask validates");
    }

    /// The whole detector chain, on real files (ADR 0073): an executable is
    /// handed an image, writes a 16-bit grey coverage, and what it wrote
    /// becomes a stored mask a revision can carry. Everything
    /// [`run_detection`] does except reading which photo is open — the part
    /// that needs a window.
    #[cfg(unix)]
    #[test]
    fn a_detector_run_ends_in_a_stored_mask() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let library = leyline_sdk::Library::create(&dir.path().join("Lib"), "Detect").unwrap();

        // A detector standing in for a model: the top half of whatever it is
        // given, in the format the contract asks for.
        let script = dir.path().join("sky.py");
        std::fs::write(
            &script,
            r#"#!/usr/bin/env python3
import sys
args = dict(zip(sys.argv[1::2], sys.argv[2::2]))
w, h = 8, 4
rows = b"".join(b"\x00" + (b"\xff\xff" if y < h // 2 else b"\x00\x00") * w
                for y in range(h))
import zlib, struct
def chunk(tag, data):
    return (struct.pack(">I", len(data)) + tag + data
            + struct.pack(">I", zlib.crc32(tag + data)))
png = (b"\x89PNG\r\n\x1a\n"
       + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 16, 0, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(rows))
       + chunk(b"IEND", b""))
open(args["--out"], "wb").write(png)
"#,
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let source = leyline_sdk::DetectorSource {
            id: "stub".to_owned(),
            label: "Stub".to_owned(),
            command: script,
            args: Vec::new(),
            detections: vec![leyline_sdk::Detection {
                id: "sky".to_owned(),
                label: "Ciel".to_owned(),
            }],
        };

        let image = dir.path().join("preview.png");
        image::RgbImage::new(8, 4).save(&image).unwrap();
        let out = dir.path().join("coverage.png");
        if leyline_sdk::detect(&source, "sky", &image, &out).is_err() {
            // No Python on this machine: the contract is exercised by
            // `leyline-detect`'s own tests, which need no interpreter.
            return;
        }

        let mask = import_coverage(&library, &out).expect("the coverage imports");
        let leyline_sdk::Mask::Coverage { path, .. } = &mask else {
            panic!("a detected mask is a stored coverage, got {mask:?}");
        };
        assert!(path.starts_with("Masks/"), "{path}");
        // The half the detector covered is the half that comes back covered:
        // a chain that inverted or flattened it would still store *a* mask.
        let stored = image::open(
            dir.path()
                .join("Lib")
                .join(path.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
        .unwrap()
        .to_luma16();
        assert_eq!(stored.get_pixel(0, 0).0[0], u16::MAX);
        assert_eq!(stored.get_pixel(0, stored.height() - 1).0[0], 0);
    }

    /// A file that is not an image fails by name rather than importing an
    /// empty mask.
    #[test]
    fn a_file_that_is_not_an_image_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let library = leyline_sdk::Library::create(&dir.path().join("Lib"), "Import").unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, b"not an image").unwrap();
        assert!(import_coverage(&library, &file).is_err());
    }
}
