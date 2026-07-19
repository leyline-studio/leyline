//! Maps develop sliders to engine parameter updates (`docs/pipeline.md`).
//!
//! Pure functions kept free of any Slint type so every rule is
//! unit-testable: the UI forwards the slider name and released value, and
//! the session applies whatever comes back.

use leyline_sdk::{Param, Value, WhiteBalance};

/// Decodes a slider release into an engine parameter update.
///
/// `wb` is the current white balance override: the two WB sliders merge
/// into it, so moving one keeps the other where it was (starting from the
/// engine default when the photo is still as-shot).
pub fn action(slider: &str, value: f64, wb: Option<WhiteBalance>) -> Option<(Param, Value)> {
    let int = Value::Int(value.round() as i32);
    Some(match slider {
        "exposure" => (Param::Exposure, Value::Float(value)),
        "contrast" => (Param::Contrast, int),
        "highlights" => (Param::Highlights, int),
        "shadows" => (Param::Shadows, int),
        "whites" => (Param::Whites, int),
        "blacks" => (Param::Blacks, int),
        "vibrance" => (Param::Vibrance, int),
        "saturation" => (Param::Saturation, int),
        "wb-temp" => {
            let mut wb = wb.unwrap_or_default();
            wb.temperature = value.round() as u32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "wb-tint" => {
            let mut wb = wb.unwrap_or_default();
            wb.tint = value.round() as i32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_stays_a_float() {
        assert_eq!(
            action("exposure", 1.25, None),
            Some((Param::Exposure, Value::Float(1.25)))
        );
    }

    #[test]
    fn sliders_round_to_integers() {
        assert_eq!(
            action("contrast", 24.6, None),
            Some((Param::Contrast, Value::Int(25)))
        );
        assert_eq!(
            action("saturation", -99.7, None),
            Some((Param::Saturation, Value::Int(-100)))
        );
    }

    #[test]
    fn white_balance_sliders_merge_into_the_override() {
        let current = WhiteBalance {
            temperature: 5200,
            tint: 12,
        };
        assert_eq!(
            action("wb-temp", 7000.0, Some(current.clone())),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: 7000,
                    tint: 12,
                }))
            ))
        );
        assert_eq!(
            action("wb-tint", -20.0, Some(current)),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: 5200,
                    tint: -20,
                }))
            ))
        );
    }

    #[test]
    fn as_shot_starts_from_the_default_override() {
        let default = WhiteBalance::default();
        assert_eq!(
            action("wb-tint", 15.0, None),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: default.temperature,
                    tint: 15,
                }))
            ))
        );
    }

    #[test]
    fn unknown_sliders_do_nothing() {
        assert_eq!(action("gamma", 1.0, None), None);
    }
}
