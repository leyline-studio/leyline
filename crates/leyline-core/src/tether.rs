//! What a tethered camera reports (`docs/adr/0087-tethered-capture-bar.md`).
//!
//! These types live here, and not in `leyline-tether`, for one reason: the
//! `tether` feature removes the libgphoto2 *backend* and never the API
//! (ADR 0038). A client compiled without that backend still calls
//! `Library::tether_settings()` and still needs the type it returns.

use serde::{Deserialize, Serialize};

/// One of the four exposure settings the capture bar drives (ADR 0087 §3).
///
/// Named by intent rather than by libgphoto2 key: the same setting is
/// `shutterspeed` on one body and `eos-shutterspeed` on another, and no
/// client has any business knowing which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TetherSetting {
    /// Exposure time.
    Shutter,
    /// Lens aperture (f-number).
    Aperture,
    /// Sensor sensitivity.
    Iso,
    /// White balance mode.
    WhiteBalance,
}

impl TetherSetting {
    /// The four settings, in the order the capture bar shows them.
    pub const ALL: [TetherSetting; 4] = [
        TetherSetting::Shutter,
        TetherSetting::Aperture,
        TetherSetting::Iso,
        TetherSetting::WhiteBalance,
    ];

    /// Stable lowercase identifier, for a command line and for logs.
    pub fn as_str(self) -> &'static str {
        match self {
            TetherSetting::Shutter => "shutter",
            TetherSetting::Aperture => "aperture",
            TetherSetting::Iso => "iso",
            TetherSetting::WhiteBalance => "wb",
        }
    }

    /// Parses what [`TetherSetting::as_str`] writes, case-insensitively.
    pub fn parse(name: &str) -> Option<TetherSetting> {
        let name = name.trim().to_ascii_lowercase();
        TetherSetting::ALL
            .into_iter()
            .find(|setting| setting.as_str() == name)
    }
}

/// One setting as the camera currently reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraSetting {
    /// The value in force, as the camera spells it ("1/160", "5.6", "2000").
    pub value: String,
    /// Everything the body will accept here, empty for a free-text widget.
    pub choices: Vec<String>,
    /// The body exposes the value but refuses to have it set (a mode dial
    /// on M, an aperture on a manual lens): show it, do not offer a picker.
    pub readonly: bool,
}

/// Everything the capture bar displays about the connected body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraSettings {
    /// The body's model name, as libgphoto2's driver reports it.
    pub model: String,
    /// The body can be told to fire the shutter from here.
    pub can_capture: bool,
    /// The body can produce live-view frames.
    pub can_live_view: bool,
    /// The settings the body actually exposes, in [`TetherSetting::ALL`]
    /// order. A setting absent from this list is one the body does not have.
    pub settings: Vec<(TetherSetting, CameraSetting)>,
}

impl CameraSettings {
    /// The camera's report for one setting, or `None` if the body does not
    /// expose it.
    pub fn get(&self, setting: TetherSetting) -> Option<&CameraSetting> {
        self.settings
            .iter()
            .find(|(name, _)| *name == setting)
            .map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command line spells settings with [`TetherSetting::as_str`] and
    /// reads them back with `parse`: the two must agree, including case.
    #[test]
    fn setting_names_round_trip() {
        for setting in TetherSetting::ALL {
            assert_eq!(TetherSetting::parse(setting.as_str()), Some(setting));
            assert_eq!(
                TetherSetting::parse(&setting.as_str().to_uppercase()),
                Some(setting)
            );
        }
        assert_eq!(TetherSetting::parse("focus"), None);
    }

    /// A setting the body does not expose reads as `None`, not as an empty
    /// value: the bar hides that control rather than offering a picker with
    /// nothing in it.
    #[test]
    fn absent_settings_read_as_none() {
        let settings = CameraSettings {
            model: "Canon EOS R6m2".to_owned(),
            can_capture: true,
            can_live_view: true,
            settings: vec![(
                TetherSetting::Iso,
                CameraSetting {
                    value: "2000".to_owned(),
                    choices: vec!["100".to_owned(), "2000".to_owned()],
                    readonly: false,
                },
            )],
        };
        assert_eq!(
            settings.get(TetherSetting::Iso).map(|s| s.value.as_str()),
            Some("2000")
        );
        assert!(settings.get(TetherSetting::Aperture).is_none());
    }
}
