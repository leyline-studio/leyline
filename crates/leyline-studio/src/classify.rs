//! Maps grid interactions to classement changes (`docs/catalog.md` §11, §18).
//!
//! Pure functions kept free of any Slint type so every rule is
//! unit-testable: the UI forwards raw key text or the clicked filter value
//! together with the current state, and applies whatever comes back.

use leyline_sdk::{ColorLabel, PickState};

/// One classement change requested from the grid.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Set (or clear) the star rating.
    Rate(Option<u8>),
    /// Set (or clear) the color label.
    Label(Option<ColorLabel>),
    /// Set the pick / reject flag.
    Flag(PickState),
}

/// Decodes a key press on the grid, Lightroom-style: `0`–`5` rate,
/// `6`–`9` toggle the red / yellow / green / blue label, `p` / `x` toggle
/// pick / reject, `u` unflags. Anything else is not a classement key.
pub fn from_key(key: &str, label: Option<ColorLabel>, pick: PickState) -> Option<Action> {
    match key {
        "0" => Some(Action::Rate(None)),
        "1" => Some(Action::Rate(Some(1))),
        "2" => Some(Action::Rate(Some(2))),
        "3" => Some(Action::Rate(Some(3))),
        "4" => Some(Action::Rate(Some(4))),
        "5" => Some(Action::Rate(Some(5))),
        "6" => Some(Action::Label(toggle(label, ColorLabel::Red))),
        "7" => Some(Action::Label(toggle(label, ColorLabel::Yellow))),
        "8" => Some(Action::Label(toggle(label, ColorLabel::Green))),
        "9" => Some(Action::Label(toggle(label, ColorLabel::Blue))),
        "p" | "P" => Some(Action::Flag(flag(pick, PickState::Pick))),
        "x" | "X" => Some(Action::Flag(flag(pick, PickState::Reject))),
        "u" | "U" => Some(Action::Flag(PickState::None)),
        _ => None,
    }
}

/// A click on filter star `clicked` (1–5): same threshold again clears it.
pub fn toggle_rating_filter(current: Option<u8>, clicked: u8) -> Option<u8> {
    if current == Some(clicked) {
        None
    } else {
        Some(clicked)
    }
}

/// A click on a filter label dot: same label again clears the filter.
pub fn toggle_label_filter(current: Option<ColorLabel>, clicked: ColorLabel) -> Option<ColorLabel> {
    toggle(current, clicked)
}

/// A click on a pick filter chip: same state again clears the filter.
pub fn toggle_pick_filter(current: Option<PickState>, clicked: PickState) -> Option<PickState> {
    if current == Some(clicked) {
        None
    } else {
        Some(clicked)
    }
}

/// Toggles a label value: selecting the current one clears it.
fn toggle(current: Option<ColorLabel>, clicked: ColorLabel) -> Option<ColorLabel> {
    if current == Some(clicked) {
        None
    } else {
        Some(clicked)
    }
}

/// Toggles a flag value: setting the current one goes back to unflagged.
fn flag(current: PickState, clicked: PickState) -> PickState {
    if current == clicked {
        PickState::None
    } else {
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_rate_and_zero_clears() {
        let key = |k| from_key(k, None, PickState::None);
        assert_eq!(key("3"), Some(Action::Rate(Some(3))));
        assert_eq!(key("5"), Some(Action::Rate(Some(5))));
        assert_eq!(key("0"), Some(Action::Rate(None)));
    }

    #[test]
    fn label_keys_toggle() {
        assert_eq!(
            from_key("6", None, PickState::None),
            Some(Action::Label(Some(ColorLabel::Red)))
        );
        assert_eq!(
            from_key("6", Some(ColorLabel::Red), PickState::None),
            Some(Action::Label(None))
        );
        assert_eq!(
            from_key("9", Some(ColorLabel::Red), PickState::None),
            Some(Action::Label(Some(ColorLabel::Blue)))
        );
    }

    #[test]
    fn pick_keys_toggle_and_unflag() {
        assert_eq!(
            from_key("p", None, PickState::None),
            Some(Action::Flag(PickState::Pick))
        );
        assert_eq!(
            from_key("P", None, PickState::Pick),
            Some(Action::Flag(PickState::None))
        );
        assert_eq!(
            from_key("x", None, PickState::Pick),
            Some(Action::Flag(PickState::Reject))
        );
        assert_eq!(
            from_key("u", None, PickState::Reject),
            Some(Action::Flag(PickState::None))
        );
    }

    #[test]
    fn other_keys_do_nothing() {
        assert_eq!(from_key("a", None, PickState::None), None);
        assert_eq!(from_key("", None, PickState::None), None);
        assert_eq!(from_key("42", None, PickState::None), None);
    }

    #[test]
    fn filters_toggle_off_on_repeat() {
        assert_eq!(toggle_rating_filter(None, 3), Some(3));
        assert_eq!(toggle_rating_filter(Some(3), 3), None);
        assert_eq!(toggle_rating_filter(Some(3), 4), Some(4));
        assert_eq!(
            toggle_label_filter(Some(ColorLabel::Green), ColorLabel::Green),
            None
        );
        assert_eq!(
            toggle_pick_filter(None, PickState::Reject),
            Some(PickState::Reject)
        );
        assert_eq!(
            toggle_pick_filter(Some(PickState::Reject), PickState::Reject),
            None
        );
    }
}
