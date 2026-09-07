//! The seventeen categories of develop settings, and the bit each one owns
//! (ADR 0132 §1, `docs/presets.md` §3.1).
//!
//! Two dialogs — Copy Settings and Save Preset — ask the same question, and
//! both answer it as one `int`: a bitfield composed in Slint by
//! `SettingsGroupChips` and read here. **The order is the contract**, and it
//! is written down twice on purpose — once in the widget, once in [`ORDER`]
//! below — because the two live in different languages and neither can
//! import the other. What keeps them honest is that the order is not
//! arbitrary: it is the order of the table in `docs/presets.md` §3.1, which
//! is the order of the develop panel.

use leyline_sdk::SettingsGroup;

/// Bit `i` of the mask is `ORDER[i]`.
///
/// Adding a category means appending to this list *and* to the widget's last
/// row: appending, never inserting, since an inserted bit would silently
/// change the meaning of every stored mask.
pub(crate) const ORDER: &[SettingsGroup] = &[
    SettingsGroup::WhiteBalance,
    SettingsGroup::Tone,
    SettingsGroup::Presence,
    SettingsGroup::ToneCurve,
    SettingsGroup::ColorMixer,
    SettingsGroup::ColorGrading,
    SettingsGroup::CameraProfile,
    SettingsGroup::CreativeLut,
    SettingsGroup::Effects,
    SettingsGroup::LensCorrection,
    SettingsGroup::Detail,
    SettingsGroup::Rendering,
    SettingsGroup::Geometry,
    SettingsGroup::Reshape,
    SettingsGroup::SpotRemoval,
    SettingsGroup::RedEye,
    SettingsGroup::LocalAdjustments,
];

/// Everything a *look* is made of: the first eleven categories (ADR 0132 §6).
///
/// The six left out are positional — geometry, rendering, reshape, spot
/// removal, red eye, local adjustments — and the value is the same `2047`
/// the widget's `SettingsGroups.look` and `PreferencesState.copy-groups`
/// start from.
pub(crate) const LOOK: i32 = 0b111_1111_1111;

/// The categories a mask names, in [`ORDER`].
pub(crate) fn from_mask(mask: i32) -> Vec<SettingsGroup> {
    ORDER
        .iter()
        .enumerate()
        .filter(|(bit, _)| mask & (1 << bit) != 0)
        .map(|(_, group)| *group)
        .collect()
}

/// The mask naming `groups` — the inverse of [`from_mask`], used to seed a
/// dialog from what was stored.
pub(crate) fn to_mask(groups: &[SettingsGroup]) -> i32 {
    ORDER
        .iter()
        .enumerate()
        .filter(|(_, group)| groups.contains(group))
        .map(|(bit, _)| 1 << bit)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_look_is_the_first_eleven_and_nothing_positional() {
        let look = from_mask(LOOK);
        assert_eq!(look.len(), 11);
        assert_eq!(look[0], SettingsGroup::WhiteBalance);
        assert_eq!(look[10], SettingsGroup::Detail);
        // The six ADR 0132 §6 keeps out, by the rule "what a place in this
        // photograph is made of is not part of a look".
        for excluded in [
            SettingsGroup::Rendering,
            SettingsGroup::Geometry,
            SettingsGroup::Reshape,
            SettingsGroup::SpotRemoval,
            SettingsGroup::RedEye,
            SettingsGroup::LocalAdjustments,
        ] {
            assert!(!look.contains(&excluded), "{excluded:?} is not a look");
        }
    }

    #[test]
    fn a_mask_round_trips_through_the_categories_it_names() {
        for mask in [0, 1, LOOK, (1 << ORDER.len()) - 1, 0b1_0101_0101] {
            assert_eq!(to_mask(&from_mask(mask)), mask, "mask {mask:#b}");
        }
    }

    /// The widget's `SettingsGroups.every` is a literal, and this is the
    /// number it has to be: a wrong one would leave a category unreachable
    /// from the Everything button with nothing to show for it.
    #[test]
    fn every_category_fits_the_masks_the_widget_writes() {
        assert_eq!(ORDER.len(), 17);
        assert_eq!((1 << ORDER.len()) - 1, 131_071);
    }
}
