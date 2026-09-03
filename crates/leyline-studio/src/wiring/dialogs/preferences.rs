//! Wires `PreferencesState`: the preferences panel (ADR 0078) and the
//! update check it gates (ADR 0077).
//!
//! The two are one module because they are one decision seen from two
//! sides: every path that reaches the network here starts at a stored
//! consent, and every path that writes that consent is in this file.

use std::sync::PoisonError;
use std::time::Duration;

use slint::{ComponentHandle, Global, ModelRc, SharedString, VecModel};

use crate::app::report_error;
use crate::preferences::{
    PreferencesFile, SharedPreferences, consent_question_due, language_choices, language_index,
    now_secs, update_check_due,
};
use crate::ui::{DialogState, PreferencesState, StudioWindow, Tr};
use crate::updates::{self, CheckOutcome};

/// How long after the window opens the automatic check runs (ADR 0077 §2):
/// after the library is on screen, never while it is being opened, and
/// never again during the session.
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(5);

/// Locks the shared preferences, tolerating a poisoned mutex.
///
/// The lock is only ever taken on the UI thread and never held across a
/// call that can panic, so poisoning would mean a panic somewhere else
/// entirely — in which case losing the language setting is not the problem
/// worth failing over.
fn with_preferences<T>(
    preferences: &SharedPreferences,
    change: impl FnOnce(&mut PreferencesFile) -> T,
) -> T {
    let mut guard = preferences.lock().unwrap_or_else(PoisonError::into_inner);
    change(&mut guard)
}

/// Selects a bundled translation, which Slint re-evaluates every visible
/// `@tr(...)` against on the spot (ADR 0078 §3).
///
/// A tag no build knows keeps the strings as they are — a preferences file
/// written by a version that shipped one more `.po` is not an error, it is
/// simply a language this binary does not have.
pub(crate) fn apply_language(tag: &str) {
    let _ = slint::select_bundled_translation(tag);
}

/// Applies the language this installation should start in: the stored
/// preference if there is one, the system locale otherwise (ADR 0078 §3).
///
/// Must run after the first component exists — `select_bundled_translation`
/// reads the bundle list the generated constructor registers — which is why
/// this is called from `run` and not from any earlier startup step.
pub(crate) fn apply_startup_language(
    preferences: &SharedPreferences,
    system_language: Option<&str>,
) {
    let stored = with_preferences(preferences, |file| file.values().language.clone());
    if let Some(tag) = stored.as_deref().or(system_language) {
        apply_language(tag);
    }
}

/// Fills the language menu and the current values of both settings.
fn show_current_values(window: &StudioWindow, preferences: &SharedPreferences) {
    let state = PreferencesState::get(window);
    let choices = language_choices();
    let names: Vec<SharedString> = choices
        .iter()
        .map(|(_, name)| SharedString::from(name.as_str()))
        .collect();
    state.set_languages(ModelRc::from(std::rc::Rc::new(VecModel::from(names))));
    let (language, update_check) = with_preferences(preferences, |file| {
        let values = file.values();
        (
            language_index(values.language.as_deref()),
            values.update_check.unwrap_or(false),
        )
    });
    state.set_language(i32::try_from(language).unwrap_or(0));
    state.set_update_check(update_check);
    show_backgrounds(window, preferences);
}

/// Fills the two ground pickers and the colours the viewers paint
/// (ADR 0117). Called on every change, so a chip and the photograph behind
/// it never disagree.
pub(crate) fn show_backgrounds(window: &StudioWindow, preferences: &SharedPreferences) {
    let state = PreferencesState::get(window);
    let names: Vec<SharedString> = crate::preferences::BACKGROUNDS
        .iter()
        .map(|(key, _)| match *key {
            "dark" => Tr::get(window).invoke_ground_dark(),
            "grey" => Tr::get(window).invoke_ground_grey(),
            _ => Tr::get(window).invoke_ground_white(),
        })
        .collect();
    state.set_background_names(ModelRc::from(std::rc::Rc::new(VecModel::from(names))));

    let (viewer, proof) = with_preferences(preferences, |file| {
        let values = file.values();
        (
            values.viewer_background.clone(),
            values.proof_background.clone(),
        )
    });
    let index_of = |name: Option<&str>, default_name: &str| -> i32 {
        let wanted = name.unwrap_or(default_name);
        i32::try_from(
            crate::preferences::BACKGROUNDS
                .iter()
                .position(|(key, _)| *key == wanted)
                .unwrap_or(0),
        )
        .unwrap_or(0)
    };
    state.set_viewer_background(index_of(viewer.as_deref(), "dark"));
    state.set_proof_background(index_of(proof.as_deref(), "white"));
    state.set_viewer_ground(colour_of(crate::preferences::background_colour(
        viewer.as_deref(),
        "dark",
    )));
    state.set_proof_ground(colour_of(crate::preferences::background_colour(
        proof.as_deref(),
        "white",
    )));
}

/// `#rrggbb` to a Slint colour. The strings are this crate's own constants,
/// so a malformed one is a bug here rather than a value to tolerate.
fn colour_of(hex: &str) -> slint::Color {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16)
        .expect("the background table holds well-formed hex");
    slint::Color::from_argb_u8(
        255,
        ((value >> 16) & 0xff) as u8,
        ((value >> 8) & 0xff) as u8,
        (value & 0xff) as u8,
    )
}

/// Wires every callback of `PreferencesState`.
pub(crate) fn wire_preferences(window: &StudioWindow, preferences: &SharedPreferences) {
    show_current_values(window, preferences);
    let state = PreferencesState::get(window);

    {
        let handle = window.as_weak();
        state.on_open_preferences(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            DialogState::get(&window).set_dialog_result(SharedString::new());
            DialogState::get(&window).set_dialog(SharedString::from("preferences"));
        });
    }

    {
        let handle = window.as_weak();
        let preferences = preferences.clone();
        state.on_choose_language(move |index| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let choices = language_choices();
            let Some((tag, _)) = choices.get(usize::try_from(index).unwrap_or(0)) else {
                return;
            };
            // The whole interface switches under the cursor; a message
            // already printed in the status line keeps the words it was
            // produced with, which is correct — it is the report of a past
            // event, not a label.
            apply_language(tag.unwrap_or(""));
            let stored = tag.map(str::to_owned);
            if let Err(error) = with_preferences(&preferences, |file| {
                file.update(|values| values.language = stored)
            }) {
                report_error(&window, &error);
            }
            PreferencesState::get(&window).set_language(index);
        });
    }

    for (which, is_proof) in [("viewer", false), ("proof", true)] {
        let handle = window.as_weak();
        let preferences = preferences.clone();
        let choose = move |index: i32| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let Some((key, _)) = usize::try_from(index)
                .ok()
                .and_then(|i| crate::preferences::BACKGROUNDS.get(i))
            else {
                return;
            };
            let key = (*key).to_owned();
            if let Err(error) = with_preferences(&preferences, |file| {
                file.update(|values| {
                    if is_proof {
                        values.proof_background = Some(key.clone());
                    } else {
                        values.viewer_background = Some(key.clone());
                    }
                })
            }) {
                report_error(&window, &error);
            }
            show_backgrounds(&window, &preferences);
        };
        if which == "viewer" {
            state.on_choose_viewer_background(choose);
        } else {
            state.on_choose_proof_background(choose);
        }
    }

    {
        let handle = window.as_weak();
        let preferences = preferences.clone();
        state.on_set_update_check(move |allowed| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            if let Err(error) = with_preferences(&preferences, |file| {
                file.update(|values| values.update_check = Some(allowed))
            }) {
                report_error(&window, &error);
            }
            PreferencesState::get(&window).set_update_check(allowed);
            if allowed {
                // Turning it on gets this launch's check, under the same
                // cadence as any other — there is no special first case.
                check_if_due(&window, &preferences);
            }
        });
    }

    {
        let handle = window.as_weak();
        let preferences = preferences.clone();
        state.on_answer_update_consent(move |allowed| {
            let Some(window) = handle.upgrade() else {
                return;
            };
            // The first answer wins and the question is never asked again.
            // That is what lets the dialog report a refusal from three
            // different gestures — the button, Escape, a click on the
            // backdrop — without the refusal that follows the accept
            // button overwriting it.
            let written = with_preferences(&preferences, |file| {
                if file.values().update_check.is_some() {
                    None
                } else {
                    Some(file.update(|values| values.update_check = Some(allowed)))
                }
            });
            let Some(written) = written else {
                return;
            };
            if let Err(error) = written {
                report_error(&window, &error);
            }
            PreferencesState::get(&window).set_update_check(allowed);
            if allowed {
                check_if_due(&window, &preferences);
            }
        });
    }

    {
        let handle = window.as_weak();
        let preferences = preferences.clone();
        state.on_check_updates(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let state = PreferencesState::get(&window);
            DialogState::get(&window).set_dialog_result(SharedString::new());
            DialogState::get(&window).set_dialog(SharedString::from("update"));
            if state.get_update_available() || state.get_update_busy() {
                // Already found, or already asking: the entry opens the
                // dialog on what is known rather than starting a second
                // request.
                return;
            }
            state.set_update_state(SharedString::from("checking"));
            state.set_update_busy(true);
            start_check(&window, &preferences, true);
        });
    }

    {
        let handle = window.as_weak();
        state.on_install_update(move || {
            let Some(window) = handle.upgrade() else {
                return;
            };
            let state = PreferencesState::get(&window);
            if state.get_update_busy() {
                return;
            }
            state.set_update_state(SharedString::from("installing"));
            state.set_update_busy(true);
            let weak = window.as_weak();
            updates::in_background(
                || updates::install_latest(env!("CARGO_PKG_VERSION")),
                move |outcome| {
                    let Some(window) = weak.upgrade() else {
                        return;
                    };
                    let state = PreferencesState::get(&window);
                    state.set_update_busy(false);
                    // On Windows the installer takes the process over and
                    // this never runs; everywhere else the package has
                    // been replaced and the next launch is the new one.
                    match outcome {
                        Ok(()) => state.set_update_state(SharedString::from("installed")),
                        Err(error) => {
                            state.set_update_state(SharedString::from("failed"));
                            report_error(&window, &error);
                        }
                    }
                },
            );
        });
    }
}

/// Asks the second-launch question, if it is due (ADR 0078 §4).
///
/// Called once, at startup. The first launch has its own purpose
/// (ADR 0054); this is the first moment the application has nothing else to
/// say.
pub(crate) fn ask_consent_if_due(window: &StudioWindow, preferences: &SharedPreferences) {
    let due = with_preferences(preferences, |file| consent_question_due(file.values()));
    if due {
        DialogState::get(window).set_dialog_result(SharedString::new());
        DialogState::get(window).set_dialog(SharedString::from("update-consent"));
    }
}

/// Schedules the one automatic check of this launch (ADR 0077 §2).
///
/// A single-shot timer, a few seconds after the window opens — not during
/// the library's opening, and never repeated: no timer runs during a
/// session, however long it lasts.
pub(crate) fn schedule_startup_check(window: &StudioWindow, preferences: &SharedPreferences) {
    let handle = window.as_weak();
    let preferences = preferences.clone();
    slint::Timer::single_shot(STARTUP_CHECK_DELAY, move || {
        let Some(window) = handle.upgrade() else {
            return;
        };
        check_if_due(&window, &preferences);
    });
}

/// Runs an automatic check when consent is given and the last successful
/// one is older than a day.
fn check_if_due(window: &StudioWindow, preferences: &SharedPreferences) {
    let due = with_preferences(preferences, |file| {
        update_check_due(file.values(), now_secs())
    });
    if due {
        start_check(window, preferences, false);
    }
}

/// Fires one check on a worker thread and applies its outcome.
///
/// `announced` separates the two callers: a check someone asked for reports
/// every outcome, including "up to date" and a server it could not reach;
/// the automatic one says nothing unless it found something (ADR 0077 §2).
fn start_check(window: &StudioWindow, preferences: &SharedPreferences, announced: bool) {
    let weak = window.as_weak();
    let preferences = preferences.clone();
    updates::in_background(
        || updates::check(env!("CARGO_PKG_VERSION")),
        move |outcome| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let state = PreferencesState::get(&window);
            state.set_update_busy(false);
            // Reaching the manifest at all is what the 24-hour cap counts;
            // a failure leaves the timestamp alone, so a week offline does
            // not eat a week of allowances.
            if !matches!(outcome, CheckOutcome::Unreachable(_)) {
                let _ = with_preferences(&preferences, |file| {
                    file.update(|values| values.last_update_check = Some(now_secs()))
                });
            }
            match outcome {
                CheckOutcome::Available(update) => {
                    state.set_update_version(SharedString::from(update.version.as_str()));
                    state.set_update_notes(SharedString::from(update.notes.as_str()));
                    state.set_update_available(true);
                    state.set_update_state(SharedString::new());
                }
                CheckOutcome::UpToDate => {
                    state.set_update_available(false);
                    state.set_update_state(SharedString::from(if announced {
                        "current"
                    } else {
                        ""
                    }));
                }
                CheckOutcome::Unreachable(error) => {
                    state.set_update_state(SharedString::from(if announced {
                        "unreachable"
                    } else {
                        ""
                    }));
                    if announced {
                        // Kept out of the dialog, which speaks in
                        // translated sentences: the underlying transport
                        // error is English-only detail, and the status
                        // line is where untranslated engine text already
                        // goes.
                        report_error(&window, &error);
                    }
                }
            }
        },
    );
}
