//! Wires `Help ▸ Report a Problem…` (ADR 0123, ADR 0045 §4).
//!
//! Mirrors `ui/dialogs/report.slint`.
//!
//! Leyline assembles a folder and opens it; it never sends anything and never
//! names a destination. The one rule the code has to keep is that what lands
//! in the folder is exactly what the dialog said would land there.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::app::App;
use crate::ui::{DialogState, LibraryState, StudioWindow, Tr};
use slint::{ComponentHandle, Global, SharedString, Timer};

/// Everything `build.txt` says, gathered before any file is written.
///
/// A plain struct of facts rather than a function reaching for them itself:
/// this is what makes the report's *content* testable without a window, which
/// is the half worth testing (ADR 0123, Consequences).
pub(crate) struct ReportFacts {
    /// The five lines `Help ▸ About ▸ Build` shows, verbatim, so the report
    /// and the About dialog cannot drift apart.
    pub(crate) build_details: String,
    /// The library that was open.
    pub(crate) library_path: String,
    /// Its catalog schema version, when it could be read.
    pub(crate) schema_version: Option<u32>,
    /// The window's size in physical pixels — the first thing to check
    /// against a layout complaint.
    pub(crate) window_size: (u32, u32),
    /// Whether `screenshot.png` was written. A capture that fails is not a
    /// failed report (ADR 0123 §4), but it must not be left unsaid either:
    /// a reader has to know the difference between "nothing was visible" and
    /// "nothing was captured".
    pub(crate) screenshot: bool,
}

/// Renders `build.txt`.
pub(crate) fn build_report(facts: &ReportFacts) -> String {
    let schema = match facts.schema_version {
        Some(version) => format!("v{version}"),
        None => "unreadable".to_owned(),
    };
    let (width, height) = facts.window_size;
    format!(
        "{}\nWindow {width}x{height}\nLibrary {}\nCatalog schema {schema}\nScreenshot {}\n",
        facts.build_details,
        facts.library_path,
        if facts.screenshot {
            "screenshot.png"
        } else {
            "could not be captured"
        },
    )
}

/// The folder this report goes in: `Documents/Leyline Reports/<stamp>/`.
///
/// Not inside the library — a library is portable and self-contained
/// (`docs/catalog.md` §37) and a bug report is not library content. Documents
/// rather than the config directory because this is a document the user is
/// meant to find, open and attach (ADR 0123 §5).
fn report_dir(stamp: &str) -> PathBuf {
    let base = directories::UserDirs::new()
        .and_then(|dirs| dirs.document_dir().map(Path::to_path_buf))
        .or_else(|| {
            directories::ProjectDirs::from("", "", "Leyline")
                .map(|dirs| dirs.config_dir().to_path_buf())
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("Leyline Reports").join(stamp)
}

/// A sortable, filename-safe stamp — the folder's whole identity.
///
/// Local time on purpose: it is read by the person who made it, next to the
/// moment they remember.
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Enough to sort and to recognise, without pulling in a date library for
    // one filename: seconds since the epoch read the same everywhere.
    format!("leyline-report-{now}")
}

/// Opens the folder in the system file manager.
///
/// Best-effort and deliberately ignored: the dialog shows the path either way,
/// so a file manager that will not start costs a copy-paste (ADR 0123 §2).
fn reveal(path: &Path) {
    #[cfg(target_os = "windows")]
    let command = "explorer";
    #[cfg(target_os = "macos")]
    let command = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let command = "xdg-open";
    let _ = std::process::Command::new(command).arg(path).spawn();
}

/// Writes the three files, and returns the folder.
fn write_report(dir: &Path, description: &str, facts: &ReportFacts) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("description.txt"), description).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("build.txt"), build_report(facts)).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn wire_report(app: &Rc<RefCell<App>>, window: &StudioWindow) {
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_open_report(move || {
            let app = Rc::clone(&app);
            let handle = handle.clone();
            // Deferred by one frame, and that delay is the feature.
            //
            // The screenshot is of the moment the report was asked for — what
            // the photographer was looking at — not of the moment they press
            // the button, which would show this dialog and nothing else
            // (ADR 0123 §4). But the menu row that just ran sets
            // `open-menu = ""` and returns: the frame on screen still has the
            // Help menu covering a quarter of the grid, and
            // `take_snapshot()` gave exactly that, menu included. It reads
            // what has been painted; closing a menu in the model does not
            // repaint it.
            //
            // So: let the frame without the menu be painted, then capture,
            // then open the dialog. Long enough for one repaint at any
            // refresh rate, short enough that nothing else can happen in it.
            Timer::single_shot(Duration::from_millis(50), move || {
                let Some(window) = handle.upgrade() else {
                    return;
                };
                app.borrow_mut().report_shot = window.window().take_snapshot().ok();
                let state = DialogState::get(&window);
                state.set_report_description(SharedString::default());
                state.set_dialog_result(SharedString::default());
                state.set_dialog(SharedString::from("report"));
            });
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_report(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let mut app = app.borrow_mut();
            let dir = report_dir(&timestamp());

            // The capture is consumed here whether or not it can be encoded:
            // a second report should photograph a second moment.
            let shot = app.report_shot.take();
            let screenshot = shot.and_then(|buffer| {
                let png = dir.join("screenshot.png");
                std::fs::create_dir_all(&dir).ok()?;
                image::RgbaImage::from_raw(
                    buffer.width(),
                    buffer.height(),
                    buffer.as_bytes().to_vec(),
                )?
                .save(&png)
                .ok()
            });

            let facts = ReportFacts {
                build_details: LibraryState::get(&window).get_build_details().to_string(),
                library_path: app.library.root().display().to_string(),
                schema_version: app.library.catalog().user_version().ok(),
                window_size: {
                    let size = window.window().size();
                    (size.width, size.height)
                },
                screenshot: screenshot.is_some(),
            };
            let description = DialogState::get(&window)
                .get_report_description()
                .to_string();

            match write_report(&dir, &description, &facts) {
                Ok(()) => {
                    reveal(&dir);
                    DialogState::get(&window).set_dialog_result(
                        Tr::get(&window)
                            .invoke_report_written(SharedString::from(dir.display().to_string())),
                    );
                }
                Err(error) => {
                    DialogState::get(&window).set_dialog_result(
                        Tr::get(&window).invoke_error_prefix(SharedString::from(error)),
                    );
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> ReportFacts {
        ReportFacts {
            build_details: "Version 0.1.0\nCommit abc1234\nTarget x86_64\nrustc 1.97.1\nLibRaw \
                            0.21.4"
                .to_owned(),
            library_path: "/home/someone/Pictures/Library".to_owned(),
            schema_version: Some(11),
            window_size: (1400, 720),
            screenshot: true,
        }
    }

    /// `build.txt` carries the whole About ▸ Build block, unedited.
    ///
    /// It is the one part of a report nobody can transcribe by hand, and the
    /// decoder line in it is a term of `pipeline.md` §5.1 (ADR 0086): a report
    /// that dropped it could not explain a rendering difference at all.
    #[test]
    fn the_build_block_reaches_the_report_whole() {
        let report = build_report(&facts());
        for line in facts().build_details.lines() {
            assert!(report.contains(line), "{line:?} missing from {report:?}");
        }
        assert!(report.contains("Catalog schema v11"));
        assert!(report.contains("1400x720"));
    }

    /// A capture that failed says so, rather than leaving a reader to wonder
    /// whether the window was empty (ADR 0123 §4).
    #[test]
    fn a_missing_screenshot_is_stated_not_omitted() {
        let written = build_report(&facts());
        assert!(written.contains("screenshot.png"));

        let none = build_report(&ReportFacts {
            screenshot: false,
            ..facts()
        });
        assert!(none.contains("could not be captured"));
    }

    /// An unreadable schema is a fact about the library, not a reason to fail:
    /// a catalog too damaged to answer is exactly when a report is wanted.
    #[test]
    fn an_unreadable_schema_still_produces_a_report() {
        let report = build_report(&ReportFacts {
            schema_version: None,
            ..facts()
        });
        assert!(report.contains("Catalog schema unreadable"));
    }

    /// The three files, and nothing else — the dialog names them before the
    /// folder exists, and this is what keeps that promise true (ADR 0123 §3).
    #[test]
    fn the_folder_holds_exactly_what_the_dialog_announced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report");
        write_report(&path, "the crop tool stopped responding", &facts()).expect("write");

        let mut written: Vec<String> = std::fs::read_dir(&path)
            .expect("read back")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        written.sort();
        // `screenshot.png` is written separately, by the capture path; what
        // this asserts is that nothing *else* appears — no catalog copy, no
        // preferences, no list of the user's other libraries.
        assert_eq!(written, vec!["build.txt", "description.txt"]);
        assert_eq!(
            std::fs::read_to_string(path.join("description.txt")).expect("description"),
            "the crop tool stopped responding"
        );
    }
}
