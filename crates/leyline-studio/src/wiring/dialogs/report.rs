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

use crate::app::App;
use crate::ui::{DialogState, LibraryState, StudioWindow, Tr};
use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

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
    /// Every file in the folder besides the two texts, in the order the
    /// dialog listed them. Named here so someone reading `build.txt` alone
    /// knows what else was meant to be in the folder, and notices when
    /// something did not reach them.
    pub(crate) attachments: Vec<String>,
}

/// Renders `build.txt`.
pub(crate) fn build_report(facts: &ReportFacts) -> String {
    let schema = match facts.schema_version {
        Some(version) => format!("v{version}"),
        None => "unreadable".to_owned(),
    };
    let (width, height) = facts.window_size;
    let attachments = if facts.attachments.is_empty() {
        "none".to_owned()
    } else {
        facts.attachments.join(", ")
    };
    format!(
        "{}\nWindow {width}x{height}\nLibrary {}\nCatalog schema {schema}\nAttachments \
         {attachments}\n",
        facts.build_details, facts.library_path,
    )
}

/// The line shown for one attached file: its name, and its size when it has
/// one on disk.
///
/// The size is shown because a report is something the photographer will
/// attach to a message: a 60 MB file they forgot they added should be visible
/// before they send it, not after it bounces.
fn label(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    match std::fs::metadata(path).map(|m| m.len()) {
        Ok(bytes) => format!("{name} ({})", human_size(bytes)),
        Err(_) => name,
    }
}

/// A byte count as a person reads it.
fn human_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} kB", bytes / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Sends the current attachment list to the dialog.
fn show_attachments(app: &App, window: &StudioWindow) {
    let lines: Vec<SharedString> = app
        .report_files
        .iter()
        .map(|path| SharedString::from(label(path)))
        .collect();
    DialogState::get(window).set_report_attachments(ModelRc::from(Rc::new(VecModel::from(lines))));
}

/// The folder this report goes in: `Documents/Leyline Reports/<stamp>/`.
///
/// Not inside the library — a library is portable and self-contained
/// (`docs/catalog.md` §37) and a bug report is not library content. Documents
/// rather than the config directory because this is a document the user is
/// meant to find, open and attach (ADR 0123 §4).
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
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("leyline-report-{now}")
}

/// A name for `wanted` that is not already taken in the folder.
///
/// Two files picked from two different folders can carry the same name, and
/// the second one silently replacing the first would lose evidence — which is
/// the one thing this feature exists to gather.
pub(crate) fn free_name(taken: &[String], wanted: &str) -> String {
    if !taken.iter().any(|name| name == wanted) {
        return wanted.to_owned();
    }
    let (stem, extension) = match wanted.rsplit_once('.') {
        Some((stem, extension)) => (stem, format!(".{extension}")),
        None => (wanted, String::new()),
    };
    (2..)
        .map(|n| format!("{stem}-{n}{extension}"))
        .find(|candidate| !taken.iter().any(|name| name == candidate))
        .unwrap_or_else(|| wanted.to_owned())
}

/// Copies one chosen file into the folder, under a name nothing else has
/// taken, and returns that name.
///
/// Extracted from the callback so the one step where evidence can be lost is
/// covered by a test: a file that fails to copy returns `None` and is left out
/// of the list `build.txt` names, so the folder and its manifest agree even
/// when a source has gone away between the picking and the writing.
pub(crate) fn copy_attachment(dir: &Path, source: &Path, taken: &[String]) -> Option<String> {
    let wanted = source
        .file_name()
        .map_or_else(|| "attachment".to_owned(), |n| n.to_string_lossy().into());
    let name = free_name(taken, &wanted);
    std::fs::copy(source, dir.join(&name)).ok()?;
    Some(name)
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

/// Writes the two texts.
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
            let Some(window) = handle.upgrade() else {
                return;
            };
            app.borrow_mut().report_files.clear();
            let state = DialogState::get(&window);
            state.set_report_description(SharedString::default());
            state.set_dialog_result(SharedString::default());
            show_attachments(&app.borrow(), &window);
            state.set_dialog(SharedString::from("report"));
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_report_add_files(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // Their own screenshot, taken with whatever tool they use — of
            // whatever they judge worth showing, which is not always Leyline's
            // own window (ADR 0123 §3).
            let Some(picked) = rfd::FileDialog::new().pick_files() else {
                return;
            };
            {
                let mut app = app.borrow_mut();
                for path in picked {
                    if !app.report_files.contains(&path) {
                        app.report_files.push(path);
                    }
                }
            }
            show_attachments(&app.borrow(), &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_report_remove_attachment(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            {
                let mut app = app.borrow_mut();
                let Ok(index) = usize::try_from(index) else {
                    return;
                };
                if index >= app.report_files.len() {
                    return;
                }
                app.report_files.remove(index);
            }
            show_attachments(&app.borrow(), &window);
        });
    }
    {
        let app = Rc::clone(app);
        let handle = window.as_weak();
        DialogState::get(window).on_run_report(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let app = app.borrow();
            let dir = report_dir(&timestamp());
            if let Err(error) = std::fs::create_dir_all(&dir) {
                DialogState::get(&window).set_dialog_result(
                    Tr::get(&window).invoke_error_prefix(SharedString::from(error.to_string())),
                );
                return;
            }

            // Walked in the order the dialog listed them, so what `build.txt`
            // names and what the folder holds are the same list, in the same
            // order.
            let mut written: Vec<String> = Vec::new();
            for path in &app.report_files {
                if let Some(name) = copy_attachment(&dir, path, &written) {
                    written.push(name);
                }
            }

            let facts = ReportFacts {
                build_details: LibraryState::get(&window).get_build_details().to_string(),
                library_path: app.library.root().display().to_string(),
                schema_version: app.library.catalog().user_version().ok(),
                window_size: {
                    let size = window.window().size();
                    (size.width, size.height)
                },
                attachments: written,
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
                            0.22.2"
                .to_owned(),
            library_path: "/home/someone/Pictures/Library".to_owned(),
            schema_version: Some(11),
            window_size: (1400, 720),
            attachments: vec!["screenshot.png".to_owned()],
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

    /// `build.txt` names the other files, so someone who received the folder
    /// can tell that something was meant to be in it and is not.
    #[test]
    fn the_attachments_are_named_and_their_absence_too() {
        let named = build_report(&ReportFacts {
            attachments: vec!["screenshot.png".to_owned(), "frozen-window.png".to_owned()],
            ..facts()
        });
        assert!(named.contains("Attachments screenshot.png, frozen-window.png"));

        // Nothing attached is an ordinary report, not a broken one: the
        // description may be all there is to say.
        let bare = build_report(&ReportFacts {
            attachments: Vec::new(),
            ..facts()
        });
        assert!(bare.contains("Attachments none"));
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

    /// Two files picked from two folders may share a name. The second must not
    /// silently replace the first: losing evidence is the one failure this
    /// feature cannot afford.
    #[test]
    fn a_name_already_taken_is_given_a_suffix_never_overwritten() {
        assert_eq!(free_name(&[], "shot.png"), "shot.png");
        let taken = vec!["shot.png".to_owned()];
        assert_eq!(free_name(&taken, "shot.png"), "shot-2.png");
        let taken = vec!["shot.png".to_owned(), "shot-2.png".to_owned()];
        assert_eq!(free_name(&taken, "shot.png"), "shot-3.png");
        // A name with no extension keeps its shape too.
        assert_eq!(free_name(&["notes".to_owned()], "notes"), "notes-2");
    }

    /// Two files of the same name, chosen from two folders, both arrive — and
    /// `build.txt` names both. The alternative is a report that silently holds
    /// one of the two pictures its author attached.
    #[test]
    fn two_files_of_the_same_name_both_reach_the_folder() {
        let scratch = tempfile::tempdir().expect("tempdir");
        let (first, second) = (scratch.path().join("a"), scratch.path().join("b"));
        std::fs::create_dir_all(&first).expect("a");
        std::fs::create_dir_all(&second).expect("b");
        std::fs::write(first.join("shot.png"), b"first").expect("write");
        std::fs::write(second.join("shot.png"), b"second").expect("write");

        let dir = scratch.path().join("report");
        std::fs::create_dir_all(&dir).expect("report dir");
        let mut taken: Vec<String> = Vec::new();
        for source in [first.join("shot.png"), second.join("shot.png")] {
            let name = copy_attachment(&dir, &source, &taken).expect("copied");
            taken.push(name);
        }
        assert_eq!(taken, vec!["shot.png", "shot-2.png"]);
        assert_eq!(
            std::fs::read(dir.join("shot.png")).expect("first"),
            b"first"
        );
        assert_eq!(
            std::fs::read(dir.join("shot-2.png")).expect("second"),
            b"second"
        );
    }

    /// A source that has gone away between the picking and the writing is left
    /// out of the manifest rather than named in it: the folder and `build.txt`
    /// must describe the same set of files.
    #[test]
    fn a_source_that_vanished_is_not_named_in_the_manifest() {
        let scratch = tempfile::tempdir().expect("tempdir");
        let dir = scratch.path().join("report");
        std::fs::create_dir_all(&dir).expect("report dir");
        assert_eq!(
            copy_attachment(&dir, &scratch.path().join("never-existed.png"), &[]),
            None
        );
    }

    /// Sizes are shown so a 60 MB attachment is noticed before the message is
    /// sent, not after it bounces.
    #[test]
    fn sizes_read_the_way_a_person_reads_them() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2 kB");
        assert_eq!(human_size(3 * 1024 * 1024), "3.0 MB");
    }

    /// The two texts, and nothing else — the attachments are copied by the
    /// caller, which is what keeps `write_report` from ever adding a file the
    /// dialog did not announce (ADR 0123 §3).
    #[test]
    fn the_texts_are_the_only_thing_written_here() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("report");
        write_report(&path, "the crop tool stopped responding", &facts()).expect("write");

        let mut written: Vec<String> = std::fs::read_dir(&path)
            .expect("read back")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        written.sort();
        assert_eq!(written, vec!["build.txt", "description.txt"]);
        assert_eq!(
            std::fs::read_to_string(path.join("description.txt")).expect("description"),
            "the crop tool stopped responding"
        );
    }
}
