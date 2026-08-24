//! Application preferences (ADR 0078): the two settings that belong to the
//! installation rather than to a photo, a library or a view.
//!
//! The admission rule the ADR writes is not enforceable by code — it is a
//! rule for whoever adds the *next* setting. What this module does enforce
//! is the direction every failure degrades in: a missing, unreadable or
//! corrupt file yields [`Preferences::default`], and every default is
//! offline. No read path can ever turn a network check on.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The native name of every bundled translation, keyed by the language tag
/// its `translations/<tag>/` directory is named after.
///
/// This table is the one Rust line ADR 0019 promised would never be needed:
/// it held while nothing displayed a language *name*. Slint's bundled list
/// is `["", "fr"]` — a source-language tag that is the empty string, and no
/// human-readable name anywhere — so a menu built from it alone would offer
/// a blank entry. English is absent on purpose: it is the source language
/// `@tr(...)` is written in, and it has no `.po` directory.
///
/// Adding a language therefore costs a `.po` **and** a line here; the test
/// at the bottom of this file fails the build if only one of the two is
/// done.
pub(crate) const TRANSLATED_LANGUAGES: &[(&str, &str)] = &[("fr", "Français")];

/// The language tag stored for the source language, which has no `.po` of
/// its own. Slint's `select_bundled_translation` accepts it as a synonym
/// for "the strings as written".
pub(crate) const SOURCE_LANGUAGE: &str = "en";

/// Stops counting launches here (ADR 0078 §4): the file never learns
/// anything beyond "this is not the first time", which is all the consent
/// question needs to know.
const LAUNCH_CAP: u32 = 2;

/// How long a successful update check stays fresh (ADR 0077 §2), so five
/// launches in an afternoon make one request rather than five.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// `preferences.json`, as it sits on disk.
///
/// Every field is optional, and the two that are not settings —
/// `last_update_check` and `launches` — live here because this file is
/// where state of the same scope and lifetime belongs (ADR 0078 §5); a
/// second file for the beauty of the classification would buy nothing.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Preferences {
    /// Language tag; **absent** means "follow the system". The distinction
    /// is deliberate: an explicit value keeps winning the day the system
    /// language changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<String>,
    /// Consent to the update check (ADR 0077 §2); **absent** means the
    /// question has never been answered, and the answer before an answer
    /// is "no".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) update_check: Option<bool>,
    /// Unix seconds of the last *successful* check — a failed one leaves
    /// this alone, so being offline for a week does not consume the day's
    /// allowance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_update_check: Option<u64>,
    /// Launch counter, saturating at [`LAUNCH_CAP`].
    #[serde(default)]
    pub(crate) launches: u32,
}

/// The preferences as the running application holds them: the values, and
/// the file they were read from and are written back to.
///
/// Shared as `Arc<Mutex<_>>` rather than the `Rc<RefCell<_>>` the rest of
/// Studio uses for UI-thread state, for one reason: the update check runs
/// on a worker thread and reports its outcome back through a closure that
/// therefore has to be `Send` ([`crate::updates::in_background`]). Every
/// actual access still happens on the UI thread.
pub(crate) type SharedPreferences = Arc<Mutex<PreferencesFile>>;

/// See [`SharedPreferences`].
#[derive(Debug)]
pub(crate) struct PreferencesFile {
    path: PathBuf,
    values: Preferences,
}

impl PreferencesFile {
    /// Reads the file at `path`, whatever state it is in.
    pub(crate) fn open(path: PathBuf) -> Self {
        let values = load_preferences(&path);
        Self { path, values }
    }

    /// The values as last read or written.
    pub(crate) fn values(&self) -> &Preferences {
        &self.values
    }

    /// Changes the values and writes them out at once.
    ///
    /// There is no "apply" step anywhere in this panel (ADR 0078 §2), so
    /// there is no moment at which a change could sit in memory unwritten:
    /// every setter goes through here. A write failure is returned rather
    /// than swallowed — the caller puts it in the status line — but the
    /// in-memory value still changes, so the session behaves as asked even
    /// when the config directory is read-only.
    pub(crate) fn update(&mut self, change: impl FnOnce(&mut Preferences)) -> Result<(), String> {
        change(&mut self.values);
        save_preferences(&self.path, &self.values)
    }
}

/// Where the preferences file lives: next to `recent_libraries.json` and
/// the `detectors/` directory, in the per-user config directory
/// (ADR 0078 §5). Third occupant, no new convention.
pub(crate) fn preferences_path() -> Result<PathBuf, String> {
    let dirs = directories::ProjectDirs::from("", "", "Leyline")
        .ok_or_else(|| "cannot determine the user's config directory".to_owned())?;
    Ok(dirs.config_dir().join("preferences.json"))
}

/// Reads the preferences, tolerating everything: a first launch (no file),
/// a truncated write, a hand-edited file with the wrong shape. All of them
/// land on the defaults, and the defaults are offline.
pub(crate) fn load_preferences(path: &Path) -> Preferences {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Preferences::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

/// Writes the preferences atomically: a temporary file in the same
/// directory, then a rename over the target.
///
/// A rename within one filesystem is atomic, so a power cut during a write
/// leaves either the old file or the new one — never a truncated file that
/// would read back as "no consent recorded" and put the question back in
/// front of someone who already answered it.
pub(crate) fn save_preferences(path: &Path, preferences: &Preferences) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "the preferences path has no parent directory".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(preferences).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, json).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

/// Counts this launch, saturating at [`LAUNCH_CAP`]. Pure, so the
/// second-launch rule is testable without a config directory.
pub(crate) fn record_launch(preferences: &mut Preferences) {
    preferences.launches = preferences.launches.saturating_add(1).min(LAUNCH_CAP);
}

/// Whether the consent question is due (ADR 0078 §4): at the second launch,
/// and only while it has never been answered. A non-answer is an answer —
/// the dialog stores "no" on any way out — so this can only ever be true
/// once in the life of an installation.
pub(crate) fn consent_question_due(preferences: &Preferences) -> bool {
    preferences.launches >= LAUNCH_CAP && preferences.update_check.is_none()
}

/// Whether an automatic update check may run now (ADR 0077 §2): consent
/// given, and no successful check in the last 24 hours.
///
/// `now` is passed in rather than read here so the cadence can be tested at
/// chosen instants; [`now_secs`] is what the caller hands it in production.
pub(crate) fn update_check_due(preferences: &Preferences, now: u64) -> bool {
    if preferences.update_check != Some(true) {
        return false;
    }
    match preferences.last_update_check {
        // A clock that went backwards (a corrected timezone, a restored
        // machine) must not lock the check out until the future catches
        // up: a timestamp ahead of `now` is treated as due.
        Some(last) => now < last || now - last >= CHECK_INTERVAL_SECS,
        None => true,
    }
}

/// Unix seconds now, or `0` if the system clock predates the epoch — a
/// value that simply makes the next check due.
pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The language menu, in display order: "System language" first, then
/// English, then every bundled translation (ADR 0078 §3).
///
/// The tag is `None` for the first entry, which is what makes "follow the
/// system" different from "English" in the stored file rather than only on
/// screen.
pub(crate) fn language_choices() -> Vec<(Option<&'static str>, String)> {
    let mut choices = vec![(None, "System language".to_owned())];
    choices.push((Some(SOURCE_LANGUAGE), "English".to_owned()));
    choices.extend(
        TRANSLATED_LANGUAGES
            .iter()
            .map(|(tag, name)| (Some(*tag), (*name).to_owned())),
    );
    choices
}

/// Index into [`language_choices`] for a stored preference, falling back to
/// "follow the system" for a tag no build of this binary knows — a
/// preferences file written by a later version that shipped another `.po`
/// must not select an entry that isn't there.
pub(crate) fn language_index(stored: Option<&str>) -> usize {
    let Some(tag) = stored else {
        return 0;
    };
    language_choices()
        .iter()
        .position(|(choice, _)| *choice == Some(tag))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_translation_has_a_name_and_every_name_a_translation() {
        // The guard rail ADR 0019's correction asks for: a `.po` added
        // without its native name would otherwise produce a dead menu
        // entry (or a missing one), and nothing else would notice.
        let mut on_disk: Vec<String> =
            std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("translations"))
                .expect("the translations directory must exist")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect();
        on_disk.sort();

        let mut named: Vec<String> = TRANSLATED_LANGUAGES
            .iter()
            .map(|(tag, _)| (*tag).to_owned())
            .collect();
        named.sort();

        assert_eq!(on_disk, named);
    }

    #[test]
    fn a_missing_file_reads_as_the_offline_defaults() {
        let preferences = load_preferences(Path::new("/nonexistent/preferences.json"));
        assert_eq!(preferences, Preferences::default());
        assert!(!update_check_due(&preferences, now_secs()));
    }

    #[test]
    fn a_corrupt_file_reads_as_the_offline_defaults() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("preferences.json");
        std::fs::write(&path, "{ this is not json").expect("write");
        let preferences = load_preferences(&path);
        assert_eq!(preferences, Preferences::default());
        // The point of the whole degradation direction: a damaged file
        // cannot switch a network check on.
        assert!(!update_check_due(&preferences, now_secs()));
    }

    #[test]
    fn a_saved_file_reads_back_identical() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("preferences.json");
        let written = Preferences {
            language: Some("fr".to_owned()),
            update_check: Some(true),
            last_update_check: Some(1_724_500_000),
            launches: 2,
        };
        save_preferences(&path, &written).expect("save");
        assert_eq!(load_preferences(&path), written);
        // The temporary file is gone: a rename, not a copy.
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn saving_creates_the_config_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested").join("preferences.json");
        save_preferences(&path, &Preferences::default()).expect("save");
        assert!(path.is_file());
    }

    #[test]
    fn the_launch_counter_saturates_at_two() {
        let mut preferences = Preferences::default();
        record_launch(&mut preferences);
        assert_eq!(preferences.launches, 1);
        assert!(!consent_question_due(&preferences));
        record_launch(&mut preferences);
        assert_eq!(preferences.launches, 2);
        assert!(consent_question_due(&preferences));
        for _ in 0..10 {
            record_launch(&mut preferences);
        }
        assert_eq!(preferences.launches, LAUNCH_CAP);
    }

    #[test]
    fn an_answered_question_is_never_due_again() {
        let mut preferences = Preferences {
            launches: 2,
            update_check: Some(false),
            ..Preferences::default()
        };
        assert!(!consent_question_due(&preferences));
        preferences.update_check = Some(true);
        assert!(!consent_question_due(&preferences));
    }

    #[test]
    fn the_check_waits_a_day_between_successes() {
        let preferences = Preferences {
            update_check: Some(true),
            last_update_check: Some(1_000_000),
            ..Preferences::default()
        };
        assert!(!update_check_due(
            &preferences,
            1_000_000 + CHECK_INTERVAL_SECS - 1
        ));
        assert!(update_check_due(
            &preferences,
            1_000_000 + CHECK_INTERVAL_SECS
        ));
    }

    #[test]
    fn a_first_check_after_consent_is_due_at_once() {
        let preferences = Preferences {
            update_check: Some(true),
            ..Preferences::default()
        };
        assert!(update_check_due(&preferences, 0));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_lock_the_check_out() {
        let preferences = Preferences {
            update_check: Some(true),
            last_update_check: Some(2_000_000),
            ..Preferences::default()
        };
        assert!(update_check_due(&preferences, 1_000));
    }

    #[test]
    fn refusal_and_silence_both_stop_the_check() {
        for answer in [None, Some(false)] {
            let preferences = Preferences {
                update_check: answer,
                ..Preferences::default()
            };
            assert!(!update_check_due(&preferences, now_secs()));
        }
    }

    #[test]
    fn the_first_language_choice_is_the_system_one() {
        let choices = language_choices();
        assert_eq!(choices[0].0, None);
        assert_eq!(choices[1].0, Some(SOURCE_LANGUAGE));
        assert_eq!(choices.len(), 2 + TRANSLATED_LANGUAGES.len());
    }

    #[test]
    fn an_unknown_stored_language_falls_back_to_the_system() {
        assert_eq!(language_index(None), 0);
        assert_eq!(language_index(Some("en")), 1);
        assert_eq!(language_index(Some("fr")), 2);
        assert_eq!(language_index(Some("kl")), 0);
    }
}
