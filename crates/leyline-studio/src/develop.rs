//! Maps develop sliders to engine parameter updates (`docs/pipeline.md`).
//!
//! Pure functions kept free of any Slint type so every rule is
//! unit-testable: the UI forwards the slider name and released value, and
//! the session applies whatever comes back.

use leyline_sdk::{Crop, NoiseReduction, Param, Settings, Sharpening, Value};

/// Decodes a slider release into an engine parameter update.
///
/// `current` supplies the multi-field steps the slider merges into: the
/// white balance pair (starting from the engine default when the photo is
/// still as-shot), the noise reduction and sharpening pairs, and the crop
/// rectangle — in percent of the frame, clamped so it stays inside the
/// frame; a crop grown back to the full frame clears the override.
pub fn action(slider: &str, value: f64, current: &Settings) -> Option<(Param, Value)> {
    if let Some(rest) = slider.strip_prefix("crop-") {
        return crop_action(rest, value, current.crop.clone())
            .map(|next| (Param::Crop, Value::Crop(next)));
    }
    let int = Value::Int(value.round() as i32);
    Some(match slider {
        "exposure" => (Param::Exposure, Value::Float(value)),
        "rotation" => (Param::Rotation, Value::Float(value)),
        "contrast" => (Param::Contrast, int),
        "highlights" => (Param::Highlights, int),
        "shadows" => (Param::Shadows, int),
        "whites" => (Param::Whites, int),
        "blacks" => (Param::Blacks, int),
        "vibrance" => (Param::Vibrance, int),
        "saturation" => (Param::Saturation, int),
        "wb-temp" => {
            let mut wb = current.white_balance.clone().unwrap_or_default();
            wb.temperature = value.round() as u32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "wb-tint" => {
            let mut wb = current.white_balance.clone().unwrap_or_default();
            wb.tint = value.round() as i32;
            (Param::WhiteBalance, Value::WhiteBalance(Some(wb)))
        }
        "nr-luminance" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                luminance: value.round() as i32,
                ..current.noise_reduction.clone()
            }),
        ),
        "nr-color" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                color: value.round() as i32,
                ..current.noise_reduction.clone()
            }),
        ),
        "sharpen-amount" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: value.round() as i32,
                ..current.sharpening.clone()
            }),
        ),
        "sharpen-radius" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                radius: value.max(0.1),
                ..current.sharpening.clone()
            }),
        ),
        _ => return None,
    })
}

/// The crop edge sliders, in percent of the frame. The smallest accepted
/// edge is 1 % so the rectangle never collapses.
fn crop_action(edge: &str, percent: f64, current: Option<Crop>) -> Option<Option<Crop>> {
    if edge == "reset" {
        return Some(None);
    }
    let mut crop = current.unwrap_or(FULL_FRAME);
    let value = (percent / 100.0).clamp(0.0, 1.0);
    match edge {
        "left" => {
            crop.x = value.min(0.99);
            crop.width = crop.width.min(1.0 - crop.x);
        }
        "top" => {
            crop.y = value.min(0.99);
            crop.height = crop.height.min(1.0 - crop.y);
        }
        "width" => crop.width = value.clamp(0.01, 1.0 - crop.x),
        "height" => crop.height = value.clamp(0.01, 1.0 - crop.y),
        _ => return None,
    }
    Some(if crop == FULL_FRAME { None } else { Some(crop) })
}

/// The neutral crop: the whole frame.
const FULL_FRAME: Crop = Crop {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

#[cfg(test)]
mod tests {
    use leyline_sdk::WhiteBalance;

    use super::*;

    /// Neutral settings, with the given white balance / crop overrides.
    fn settings(wb: Option<WhiteBalance>, crop: Option<Crop>) -> Settings {
        Settings {
            white_balance: wb,
            crop,
            ..Settings::default()
        }
    }

    #[test]
    fn exposure_and_rotation_stay_floats() {
        let neutral = settings(None, None);
        assert_eq!(
            action("exposure", 1.25, &neutral),
            Some((Param::Exposure, Value::Float(1.25)))
        );
        assert_eq!(
            action("rotation", -2.5, &neutral),
            Some((Param::Rotation, Value::Float(-2.5)))
        );
    }

    #[test]
    fn sliders_round_to_integers() {
        let neutral = settings(None, None);
        assert_eq!(
            action("contrast", 24.6, &neutral),
            Some((Param::Contrast, Value::Int(25)))
        );
        assert_eq!(
            action("saturation", -99.7, &neutral),
            Some((Param::Saturation, Value::Int(-100)))
        );
    }

    #[test]
    fn white_balance_sliders_merge_into_the_override() {
        let current = settings(
            Some(WhiteBalance {
                temperature: 5200,
                tint: 12,
            }),
            None,
        );
        assert_eq!(
            action("wb-temp", 7000.0, &current),
            Some((
                Param::WhiteBalance,
                Value::WhiteBalance(Some(WhiteBalance {
                    temperature: 7000,
                    tint: 12,
                }))
            ))
        );
        assert_eq!(
            action("wb-tint", -20.0, &current),
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
            action("wb-tint", 15.0, &settings(None, None)),
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
    fn detail_sliders_keep_their_sibling_field() {
        let mut current = settings(None, None);
        current.noise_reduction.color = 40;
        current.sharpening.radius = 2.0;
        assert_eq!(
            action("nr-luminance", 30.0, &current),
            Some((
                Param::NoiseReduction,
                Value::NoiseReduction(NoiseReduction {
                    luminance: 30,
                    color: 40,
                })
            ))
        );
        assert_eq!(
            action("sharpen-amount", 55.0, &current),
            Some((
                Param::Sharpening,
                Value::Sharpening(Sharpening {
                    amount: 55,
                    radius: 2.0,
                })
            ))
        );
        assert_eq!(
            action("sharpen-radius", 0.0, &current),
            Some((
                Param::Sharpening,
                Value::Sharpening(Sharpening {
                    amount: 0,
                    radius: 0.1,
                })
            ))
        );
    }

    #[test]
    fn unknown_sliders_do_nothing() {
        let neutral = settings(None, None);
        assert_eq!(action("gamma", 1.0, &neutral), None);
        assert_eq!(action("crop-diagonal", 10.0, &neutral), None);
    }

    #[test]
    fn crop_sliders_merge_and_stay_inside_the_frame() {
        let Some((Param::Crop, Value::Crop(Some(crop)))) =
            action("crop-left", 25.0, &settings(None, None))
        else {
            panic!("expected a crop update");
        };
        assert_eq!((crop.x, crop.width), (0.25, 0.75));

        let Some((_, Value::Crop(Some(crop)))) =
            action("crop-width", 90.0, &settings(None, Some(crop)))
        else {
            panic!("expected a crop update");
        };
        assert_eq!((crop.x, crop.width), (0.25, 0.75));

        let Some((_, Value::Crop(Some(crop)))) =
            action("crop-height", 50.0, &settings(None, Some(crop)))
        else {
            panic!("expected a crop update");
        };
        assert_eq!((crop.y, crop.height), (0.0, 0.5));
    }

    #[test]
    fn full_frame_and_reset_clear_the_crop() {
        let current = Crop {
            x: 0.2,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        assert_eq!(
            action("crop-reset", 0.0, &settings(None, Some(current.clone()))),
            Some((Param::Crop, Value::Crop(None)))
        );
        let Some((_, Value::Crop(Some(widened)))) =
            action("crop-left", 0.0, &settings(None, Some(current)))
        else {
            panic!("expected a crop update");
        };
        assert_eq!(
            action("crop-width", 100.0, &settings(None, Some(widened))),
            Some((Param::Crop, Value::Crop(None)))
        );
    }
}
