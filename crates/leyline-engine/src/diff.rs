//! What changed between two develop states (ADR 0142).
//!
//! The history panel had nothing to show but timestamps, while every
//! revision carries its complete settings (`docs/pipeline.md` §3.2). This
//! module answers the question those two facts make obvious: *which settings
//! does this revision hold differently from the one before it?*
//!
//! The answer is a list of **keys of the stored document**, not words: the
//! engine names things in the vocabulary of the format it writes, and the
//! clients — which have translations — turn a key into a label.

use leyline_core::Settings;

/// The `settings_json` keys that differ between two develop states, **sorted
/// by key**.
///
/// Sorted because that is what the serialized form gives (`serde_json`'s
/// object is a `BTreeMap` here) and because the answer has to be
/// deterministic; a client that wants the order of its own panel sorts the
/// labels it turns these into, in the language it shows them.
///
/// Compared through the serialized form rather than field by field, for the
/// reason `docs/pipeline.md` §3.2 makes the format a contract: a field added
/// to `Settings` appears here the day it is added, without a second list to
/// keep in step. A neutral field serializes to nothing on both sides and is
/// therefore not a change.
///
/// `schema` is never reported: it is the document's own version, not a
/// setting anybody moved.
#[must_use]
pub fn changed_settings(before: &Settings, after: &Settings) -> Vec<String> {
    let (Ok(before), Ok(after)) = (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Vec::new();
    };
    let (Some(before), Some(after)) = (before.as_object(), after.as_object()) else {
        return Vec::new();
    };
    let mut changed = Vec::new();
    for (key, value) in after {
        if key == "schema" {
            continue;
        }
        if before.get(key) != Some(value) {
            changed.push(key.clone());
        }
    }
    // A setting that went back to neutral is gone from `after` and is just
    // as much a change as one that appeared.
    for key in before.keys() {
        if key != "schema" && !after.contains_key(key) {
            changed.push(key.clone());
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_compared_with_itself_changed_nothing() {
        let settings = Settings::default();
        assert!(changed_settings(&settings, &settings).is_empty());
    }

    #[test]
    fn a_moved_slider_is_named_by_its_key() {
        let before = Settings::default();
        let mut after = before.clone();
        after.exposure = 0.35;
        assert_eq!(changed_settings(&before, &after), vec!["exposure"]);
    }

    #[test]
    fn a_setting_back_at_neutral_is_a_change_too() {
        let before = Settings {
            contrast: 20,
            ..Settings::default()
        };
        let after = Settings::default();
        assert_eq!(changed_settings(&before, &after), vec!["contrast"]);
    }

    #[test]
    fn the_document_version_is_not_a_setting() {
        let before = Settings::default();
        let mut after = before.clone();
        after.schema += 1;
        assert!(changed_settings(&before, &after).is_empty());
    }

    /// A reprocess moves no slider: what it changes is the rendering axis,
    /// and the history has to be able to say so (ADR 0142 §2).
    #[test]
    fn a_reprocess_shows_up_as_the_stage_map() {
        let before = Settings::default();
        let mut after = before.clone();
        after.stages.insert("gains".to_owned(), 2);
        assert_eq!(changed_settings(&before, &after), vec!["stages"]);
    }

    #[test]
    fn several_settings_are_reported_sorted_by_key() {
        let before = Settings::default();
        let mut after = before.clone();
        after.exposure = 1.0;
        after.contrast = 10;
        after.rotation = 90.0;
        assert_eq!(
            changed_settings(&before, &after),
            vec!["contrast", "exposure", "rotation"]
        );
    }
}
