//! Entry point of the develop renderer: process version dispatch.
//!
//! A revision is always rendered with the process version it declares
//! (`docs/pipeline.md` §3.3). The engine keeps every past process module
//! forever; a revision written by a newer engine is refused, never guessed
//! at (§3.4) — the caller falls back to the best cached preview.

use leyline_catalog::Metadata;
use leyline_core::{CURRENT_PROCESS, CURRENT_SCHEMA, Settings};
use leyline_core::{LeylineError, Result};
use leyline_raw::RawImage;

use crate::process1;
use crate::process2;
use crate::process3;
use crate::process4;
use crate::process5;
use crate::process6;
use crate::process7;
use crate::process8;
use crate::process9;

/// A rendered develop result: tightly packed, interleaved 8-bit RGB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` samples.
    pub data: Vec<u8>,
}

/// EXIF identification of one shot, as needed to look up its Lensfun
/// profile (`leyline_lens::find_profile`) — the lens correction step of
/// process 3 (distortion) and process 4 (vignetting). Built from the
/// asset's catalog [`Metadata`] by [`lens_shot`].
#[derive(Debug, Clone, PartialEq)]
pub struct LensShot {
    /// Camera manufacturer, as written by EXIF.
    pub camera_make: String,
    /// Camera model, as written by EXIF.
    pub camera_model: String,
    /// Lens manufacturer, when recorded.
    pub lens_make: Option<String>,
    /// Lens model, when recorded.
    pub lens_model: Option<String>,
    /// Focal length in millimeters at capture.
    pub focal_mm: f32,
    /// Aperture f-number at capture, when recorded — needed for
    /// vignetting correction (process 4); distortion doesn't use it.
    pub aperture_f: Option<f32>,
}

/// Builds a [`LensShot`] from an asset's catalog metadata, when it carries
/// enough to attempt a profile match: a camera body and a focal length.
/// Missing lens make/model or aperture is not disqualifying here —
/// [`LensShot`] simply carries `None`, and the profile lookup / vignetting
/// step skip correction on their own.
pub fn lens_shot(meta: &Metadata) -> Option<LensShot> {
    let camera = meta.camera.as_ref()?;
    let focal_mm = meta.focal_length?.as_f64() as f32;
    Some(LensShot {
        camera_make: camera.manufacturer.clone(),
        camera_model: camera.model.clone(),
        lens_make: meta.lens.as_ref().map(|l| l.manufacturer.clone()),
        lens_model: meta.lens.as_ref().map(|l| l.model.clone()),
        focal_mm,
        aperture_f: meta.aperture.map(|r| r.as_f64() as f32),
    })
}

/// Renders a decoded image according to a revision's settings.
///
/// Identical image, settings and shot produce identical pixels
/// (`docs/pipeline.md` §5). Settings declaring a schema or process newer
/// than this engine are refused with [`LeylineError::NewerSettings`]: a
/// schema this engine cannot fully read could hide renamed parameters whose
/// neutral fallback would silently change the rendering. `shot` feeds only
/// process 3's lens correction; older process versions ignore it.
pub fn render(image: &RawImage, settings: &Settings, shot: Option<&LensShot>) -> Result<Rendered> {
    if settings.schema > CURRENT_SCHEMA || settings.process > CURRENT_PROCESS {
        return Err(LeylineError::NewerSettings {
            schema: settings.schema,
            process: settings.process,
        });
    }
    settings.validate()?;
    match settings.process {
        1 => process1::develop(image, settings),
        2 => process2::develop(image, settings),
        3 => process3::develop(image, settings, shot),
        4 => process4::develop(image, settings, shot),
        5 => process5::develop(image, settings, shot),
        6 => process6::develop(image, settings, shot),
        7 => process7::develop(image, settings, shot),
        8 => process8::develop(image, settings, shot),
        9 => process9::develop(image, settings, shot),
        other => Err(LeylineError::InvalidSettings(format!(
            "process version {other} does not exist"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leyline_catalog::{CameraInfo, LensInfo, Rational};
    use leyline_core::{Crop, NoiseReduction, Sharpening, WhiteBalance};

    #[test]
    fn lens_shot_needs_a_camera_and_a_focal_length() {
        assert_eq!(lens_shot(&Metadata::default()), None);
        assert_eq!(
            lens_shot(&Metadata {
                camera: Some(CameraInfo {
                    manufacturer: "Canon".to_owned(),
                    model: "EOS 5D Mark III".to_owned(),
                }),
                ..Metadata::default()
            }),
            None,
            "no focal length: no shot"
        );
    }

    #[test]
    fn lens_shot_carries_a_missing_lens_as_none() {
        let meta = Metadata {
            camera: Some(CameraInfo {
                manufacturer: "Canon".to_owned(),
                model: "EOS 5D Mark III".to_owned(),
            }),
            focal_length: Some(Rational {
                numerator: 20,
                denominator: 1,
            }),
            ..Metadata::default()
        };
        let shot = lens_shot(&meta).unwrap();
        assert_eq!(shot.camera_make, "Canon");
        assert_eq!(shot.focal_mm, 20.0);
        assert_eq!(shot.lens_make, None);
        assert_eq!(shot.lens_model, None);
    }

    #[test]
    fn lens_shot_carries_the_lens_when_present() {
        let meta = Metadata {
            camera: Some(CameraInfo {
                manufacturer: "Canon".to_owned(),
                model: "EOS 5D Mark III".to_owned(),
            }),
            lens: Some(LensInfo {
                manufacturer: "Canon".to_owned(),
                model: "EF 16-35mm f/2.8L II USM".to_owned(),
                mount: None,
            }),
            focal_length: Some(Rational {
                numerator: 200,
                denominator: 10,
            }),
            ..Metadata::default()
        };
        let shot = lens_shot(&meta).unwrap();
        assert_eq!(shot.lens_make.as_deref(), Some("Canon"));
        assert_eq!(shot.lens_model.as_deref(), Some("EF 16-35mm f/2.8L II USM"));
        assert_eq!(shot.focal_mm, 20.0);
    }

    /// A deterministic 12×8 test card: a gradient with a colored block.
    fn test_image() -> RawImage {
        let (width, height) = (12u32, 8u32);
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                if (3..6).contains(&x) && (2..5).contains(&y) {
                    data.extend_from_slice(&[200, 60, 40]);
                } else {
                    let g = (x * 20 + y * 5) as u8;
                    data.extend_from_slice(&[g, g, g]);
                }
            }
        }
        RawImage {
            width,
            height,
            bits: 8,
            data,
        }
    }

    #[test]
    fn neutral_settings_render_the_decoded_image_bit_for_bit() {
        let image = test_image();
        let out = render(&image, &Settings::default(), None).unwrap();
        assert_eq!((out.width, out.height), (image.width, image.height));
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn rendering_is_deterministic() {
        let image = test_image();
        let settings = Settings {
            white_balance: Some(WhiteBalance {
                temperature: 5000,
                tint: 10,
            }),
            exposure: 0.4,
            contrast: 25,
            highlights: -30,
            shadows: 20,
            whites: 10,
            blacks: -10,
            vibrance: 15,
            saturation: 5,
            noise_reduction: NoiseReduction {
                luminance: 20,
                color: 30,
            },
            sharpening: Sharpening {
                amount: 40,
                radius: 1.0,
            },
            rotation: 12.5,
            crop: Some(Crop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            }),
            ..Settings::default()
        };
        let first = render(&image, &settings, None).unwrap();
        let second = render(&image, &settings, None).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn exposure_brightens_and_darkens() {
        let image = test_image();
        let brighter = render(
            &image,
            &Settings {
                exposure: 1.0,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let darker = render(
            &image,
            &Settings {
                exposure: -1.0,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let sum = |data: &[u8]| data.iter().map(|&v| u64::from(v)).sum::<u64>();
        assert!(sum(&brighter.data) > sum(&image.data));
        assert!(sum(&darker.data) < sum(&image.data));
    }

    #[test]
    fn temperature_shifts_the_red_blue_balance() {
        let image = test_image();
        let warm = render(
            &image,
            &Settings {
                white_balance: Some(WhiteBalance {
                    temperature: 9000,
                    tint: 0,
                }),
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let sum = |data: &[u8], channel: usize| {
            data.chunks_exact(3)
                .map(|px| u64::from(px[channel]))
                .sum::<u64>()
        };
        assert!(sum(&warm.data, 0) > sum(&image.data, 0), "warmer: more red");
        assert!(
            sum(&warm.data, 2) < sum(&image.data, 2),
            "warmer: less blue"
        );
    }

    #[test]
    fn pivot_white_balance_is_neutral() {
        let image = test_image();
        let out = render(
            &image,
            &Settings {
                white_balance: Some(WhiteBalance {
                    temperature: 6500,
                    tint: 0,
                }),
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        assert_eq!(out.data, image.data);
    }

    #[test]
    fn full_desaturation_produces_gray() {
        let out = render(
            &test_image(),
            &Settings {
                saturation: -100,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        for px in out.data.chunks_exact(3) {
            assert!(
                px.iter().max().unwrap() - px.iter().min().unwrap() <= 1,
                "{px:?}"
            );
        }
    }

    #[test]
    fn vibrance_boosts_muted_colors_more_than_saturated_ones() {
        let image = RawImage {
            width: 2,
            height: 1,
            bits: 8,
            // A muted red and an almost fully saturated red.
            data: vec![140, 110, 110, 250, 10, 10],
        };
        let out = render(
            &image,
            &Settings {
                vibrance: 80,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let chroma = |px: &[u8]| i32::from(*px.iter().max().unwrap() - *px.iter().min().unwrap());
        let muted_gain = chroma(&out.data[0..3]) - chroma(&image.data[0..3]);
        let saturated_gain = chroma(&out.data[3..6]) - chroma(&image.data[3..6]);
        assert!(
            muted_gain > saturated_gain,
            "{muted_gain} vs {saturated_gain}"
        );
    }

    #[test]
    fn luminance_noise_reduction_smooths_the_image() {
        // A checkerboard is pure luma noise at pixel scale.
        let (width, height) = (16u32, 16u32);
        let data: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    let v = if (x + y) % 2 == 0 { 40 } else { 210 };
                    [v, v, v]
                })
            })
            .collect();
        let image = RawImage {
            width,
            height,
            bits: 8,
            data,
        };
        let out = render(
            &image,
            &Settings {
                noise_reduction: NoiseReduction {
                    luminance: 100,
                    color: 0,
                },
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        let spread = |data: &[u8]| {
            i32::from(*data.iter().max().unwrap()) - i32::from(*data.iter().min().unwrap())
        };
        assert!(spread(&out.data) < spread(&image.data) / 2);
    }

    #[test]
    fn sharpening_increases_edge_contrast() {
        // A vertical two-tone edge.
        let (width, height) = (8u32, 4u32);
        let data: Vec<u8> = (0..height)
            .flat_map(|_| {
                (0..width).flat_map(|x| {
                    let v = if x < 4 { 80 } else { 170 };
                    [v, v, v]
                })
            })
            .collect();
        let image = RawImage {
            width,
            height,
            bits: 8,
            data,
        };
        let out = render(
            &image,
            &Settings {
                sharpening: Sharpening {
                    amount: 100,
                    radius: 1.0,
                },
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        // Overshoot on both sides of the edge, on the row y=1.
        let row = &out.data[(width as usize * 3)..(width as usize * 3 * 2)];
        assert!(row[3 * 3] < 80, "dark side dips below the flat value");
        assert!(row[4 * 3] > 170, "bright side overshoots the flat value");
    }

    #[test]
    fn rotation_by_quarter_turn_swaps_dimensions() {
        let out = render(
            &test_image(),
            &Settings {
                rotation: 90.0,
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        assert_eq!((out.width, out.height), (8, 12));
    }

    #[test]
    fn crop_extracts_the_requested_window() {
        let image = test_image();
        let out = render(
            &image,
            &Settings {
                crop: Some(Crop {
                    x: 0.25,
                    y: 0.5,
                    width: 0.5,
                    height: 0.25,
                }),
                ..Settings::default()
            },
            None,
        )
        .unwrap();
        assert_eq!((out.width, out.height), (6, 2));
        // Top-left crop pixel is source pixel (3, 4).
        let src = &image.data[((4 * 12 + 3) * 3)..((4 * 12 + 3) * 3 + 3)];
        assert_eq!(&out.data[0..3], src);
    }

    #[test]
    fn process_2_matches_process_1_within_one_8bit_step() {
        // The LUT approximation (ADR 0013) may move a sample across a
        // rounding boundary, never further.
        let image = test_image();
        let settings = |process| Settings {
            process,
            white_balance: Some(WhiteBalance {
                temperature: 5000,
                tint: 10,
            }),
            exposure: 0.4,
            ..Settings::default()
        };
        let p1 = render(&image, &settings(1), None).unwrap();
        let p2 = render(&image, &settings(2), None).unwrap();
        assert_eq!(p1.data.len(), p2.data.len());
        for (a, b) in p1.data.iter().zip(&p2.data) {
            assert!(a.abs_diff(*b) <= 1, "{a} vs {b}");
        }
    }

    #[test]
    fn newer_schema_or_process_is_refused() {
        let image = test_image();
        for settings in [
            Settings {
                schema: CURRENT_SCHEMA + 1,
                ..Settings::default()
            },
            Settings {
                process: CURRENT_PROCESS + 1,
                ..Settings::default()
            },
        ] {
            assert!(matches!(
                render(&image, &settings, None),
                Err(LeylineError::NewerSettings { .. })
            ));
        }
    }

    #[test]
    fn invalid_settings_are_refused() {
        let err = render(
            &test_image(),
            &Settings {
                contrast: 999,
                ..Settings::default()
            },
            None,
        )
        .unwrap_err();
        assert!(matches!(err, LeylineError::InvalidSettings(_)));

        let err = render(
            &test_image(),
            &Settings {
                process: 0,
                ..Settings::default()
            },
            None,
        )
        .unwrap_err();
        assert!(matches!(err, LeylineError::InvalidSettings(_)));
    }
}
