//! Entry point of the develop renderer.
//!
//! A revision is always rendered through the stage versions it declares
//! (`docs/pipeline.md` §3.3), and the engine keeps every published stage
//! version forever ([`crate::stages`]). A revision written by a newer engine
//! — newer format, or a stage version this one does not implement — is
//! refused, never guessed at (§3.4): the caller falls back to the best
//! cached preview.

use leyline_catalog::Metadata;
use leyline_core::{CURRENT_SCHEMA, Settings};
use leyline_core::{LeylineError, Result};
use leyline_raw::RawImage;

use crate::stages::{self, SourceColor};

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
/// profile (`leyline_lens::find_profile`) — the input of the lens
/// correction stage. Built from the asset's catalog [`Metadata`] by
/// [`lens_shot`].
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
    /// vignetting correction; distortion doesn't use it.
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
/// Identical image, settings, shot and camera profile produce identical
/// pixels (`docs/pipeline.md` §5). Settings declaring a schema newer than
/// this engine are refused with [`LeylineError::NewerSettings`]: a schema
/// this engine cannot fully read could hide renamed parameters whose
/// neutral fallback would silently change the rendering. A stage version it
/// does not implement is refused the same way
/// ([`LeylineError::UnknownStage`]).
///
/// `source` says what the decoder produced — which colorimetry the pixels
/// are in before the `input` stage converts them into the working space
/// (ADR 0044 §3). It is a property of the file, not of the revision.
///
/// `shot` feeds only the lens stage. `camera_profile` feeds only the camera
/// profile stage (ADR 0035) and `lut` only the LUT stage (ADR 0053) — both
/// already resolved, checksummed and parsed by the caller
/// (`crate::camera_profile::resolve_from_settings`,
/// `crate::lut::resolve_from_settings`), since reading a file from disk has no
/// place in this otherwise pure function. Any of them being `None` leaves its
/// stage with nothing to do.
pub fn render(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&leyline_color::DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    source: SourceColor,
) -> Result<Rendered> {
    render_scaled(image, settings, shot, camera_profile, lut, source, 1.0)
}

/// [`render`] of an image already reduced by `scale` (ADR 0041).
///
/// The preview path shrinks the decoded image to the requested size class
/// *before* developing it, rather than developing millions of pixels it is
/// about to throw away. Several stages express a radius in pixels — noise
/// reduction, sharpening, clarity, texture, dehaze — so the same factor has
/// to reach them, or a blur would cover several times more of the subject
/// on the proxy than at full size.
///
/// `scale == 1.0` is exactly [`render`]: that is the path export and print
/// take, and it is bit-identical to what this engine produced before the
/// parameter existed. The reproducibility contract (`docs/pipeline.md` §5)
/// therefore does not move, and no new stage version is needed.
pub fn render_scaled(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&leyline_color::DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    source: SourceColor,
    scale: f32,
) -> Result<Rendered> {
    if settings.schema > CURRENT_SCHEMA {
        return Err(LeylineError::NewerSettings {
            schema: settings.schema,
        });
    }
    settings.validate()?;
    stages::develop_scaled(image, settings, shot, camera_profile, lut, source, scale)
}

/// [`render_scaled`] with the preview pipeline's stage cache (ADR 0041 §3).
///
/// Same guards, same pixels — `cache` only decides how much of the plan has
/// to be replayed. Deliberately `pub(crate)` and preview-only: export and
/// print go through [`render_scaled`], unchanged and uncached.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_scaled_cached(
    image: &RawImage,
    settings: &Settings,
    shot: Option<&LensShot>,
    camera_profile: Option<&leyline_color::DcpProfile>,
    lut: Option<&leyline_color::CubeLut>,
    source: SourceColor,
    scale: f32,
    asset: leyline_core::AssetId,
    cache: &mut stages::StageCache,
) -> Result<Rendered> {
    if settings.schema > CURRENT_SCHEMA {
        return Err(LeylineError::NewerSettings {
            schema: settings.schema,
        });
    }
    settings.validate()?;
    stages::develop_scaled_cached(
        image,
        settings,
        shot,
        camera_profile,
        lut,
        source,
        scale,
        asset,
        cache,
    )
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

    /// The neutral rendering: what the file looks like when no operator
    /// runs.
    ///
    /// Before ADR 0044 it was the decoded bytes, unchanged — the decoder
    /// handed over display pixels and the pipeline passed them through.
    /// It cannot be that any more, and the reason is the point of the ADR:
    /// the decoder is now asked for the sensor's own linear numbers, and
    /// turning those into something a screen can show *is* a rendering.
    /// What stays true is that nothing between the two ends touches the
    /// image, which is what this asserts — geometry untouched, grey still
    /// grey, and monotonic in the input.
    fn neutral(image: &RawImage) -> Rendered {
        render(image, &Settings::default(), None, None, None, SOURCE).unwrap()
    }

    /// The colorimetry every test here renders through: no camera matrix,
    /// so the sensor's numbers are taken as working-space values and the
    /// tests stay about the operators rather than about a body's profile.
    const SOURCE: SourceColor = SourceColor::Camera {
        to_xyz: None,
        multipliers: None,
    };

    #[test]
    fn a_neutral_render_keeps_geometry_grey_and_order() {
        let image = test_image();
        let out = neutral(&image);
        assert_eq!((out.width, out.height), (image.width, image.height));

        for (source, rendered) in image.data.chunks_exact(3).zip(out.data.chunks_exact(3)) {
            if source[0] == source[1] && source[1] == source[2] {
                assert!(
                    rendered[0].abs_diff(rendered[1]) <= 1
                        && rendered[1].abs_diff(rendered[2]) <= 1,
                    "a grey sample must stay grey: {source:?} -> {rendered:?}"
                );
            }
        }

        // Monotonic on the greys: a brighter one in, a brighter one out.
        // Only the greys — a colored pixel's channels move against each
        // other through the output matrix, which is the whole point of
        // rendering out of a wider space.
        let mut pairs: Vec<(u8, u8)> = image
            .data
            .chunks_exact(3)
            .zip(out.data.chunks_exact(3))
            .filter(|(source, _)| source[0] == source[1] && source[1] == source[2])
            .map(|(source, rendered)| (source[0], rendered[0]))
            .collect();
        pairs.sort_by_key(|(source, _)| *source);
        for window in pairs.windows(2) {
            assert!(
                window[1].1 >= window[0].1,
                "rendering must be monotonic: {window:?}"
            );
        }
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
        let first = render(&image, &settings, None, None, None, SOURCE).unwrap();
        let second = render(&image, &settings, None, None, None, SOURCE).unwrap();
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
            None,
            None,
            SOURCE,
        )
        .unwrap();
        let darker = render(
            &image,
            &Settings {
                exposure: -1.0,
                ..Settings::default()
            },
            None,
            None,
            None,
            SOURCE,
        )
        .unwrap();
        let sum = |data: &[u8]| data.iter().map(|&v| u64::from(v)).sum::<u64>();
        let base = sum(&neutral(&image).data);
        assert!(sum(&brighter.data) > base);
        assert!(sum(&darker.data) < base);
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
            None,
            None,
            SOURCE,
        )
        .unwrap();
        let sum = |data: &[u8], channel: usize| {
            data.chunks_exact(3)
                .map(|px| u64::from(px[channel]))
                .sum::<u64>()
        };
        let base = neutral(&image);
        assert!(sum(&warm.data, 0) > sum(&base.data, 0), "warmer: more red");
        assert!(sum(&warm.data, 2) < sum(&base.data, 2), "warmer: less blue");
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
            None,
            None,
            SOURCE,
        )
        .unwrap();
        assert_eq!(
            out.data,
            neutral(&image).data,
            "the pivot temperature is a gain of exactly 1"
        );
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
            None,
            None,
            SOURCE,
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
            None,
            None,
            SOURCE,
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

    /// A grey checkerboard of amplitude `±(step/2)` around 128 — pixel-scale
    /// detail whose *amplitude* is the whole question for `noise_*::v2`.
    fn checkerboard(step: u8) -> RawImage {
        let (width, height) = (16u32, 16u32);
        let data: Vec<u8> = (0..height)
            .flat_map(|y| {
                (0..width).flat_map(move |x| {
                    let v = if (x + y) % 2 == 0 {
                        128 - step / 2
                    } else {
                        128 + step / 2
                    };
                    [v, v, v]
                })
            })
            .collect();
        RawImage {
            width,
            height,
            bits: 8,
            data,
        }
    }

    fn denoised(image: &RawImage, luminance: i32) -> Rendered {
        render(
            image,
            &Settings {
                noise_reduction: NoiseReduction {
                    luminance,
                    color: 0,
                },
                ..Settings::default()
            },
            None,
            None,
            None,
            SOURCE,
        )
        .unwrap()
    }

    /// Peak-to-peak amplitude over the middle of a 16×16 render. The interior,
    /// not the whole buffer: every kernel in the pipeline replicates its
    /// edges, so border pixels see a different neighborhood and say nothing
    /// about what the operator does to the image.
    fn interior_spread(data: &[u8], width: usize) -> i32 {
        let mut lo = u8::MAX;
        let mut hi = u8::MIN;
        for y in 4..12 {
            for x in 4..12 {
                let v = data[(y * width + x) * 3];
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
        i32::from(hi) - i32::from(lo)
    }

    /// Both halves compare the same render with the slider at 0 and at 100,
    /// so what is measured is the operator alone — not the pipeline's own
    /// tone rendering, which moves these values too.
    #[test]
    fn luminance_noise_reduction_smooths_low_amplitude_grain() {
        let image = checkerboard(8);
        let before = interior_spread(&denoised(&image, 0).data, 16);
        let after = interior_spread(&denoised(&image, 100).data, 16);
        assert!(after < before / 2, "grain survived: {after} of {before}");
    }

    /// The other half of ADR 0046, and what `v1` could not do: the same
    /// pattern at an amplitude no sensor produces as noise is *detail*, and
    /// the slider at 100 must leave it standing. A Gaussian blur strong
    /// enough to pass the test above would have flattened this one too.
    #[test]
    fn luminance_noise_reduction_leaves_high_amplitude_detail_alone() {
        let image = checkerboard(170);
        let before = interior_spread(&denoised(&image, 0).data, 16);
        let after = interior_spread(&denoised(&image, 100).data, 16);
        assert!(after > before * 3 / 4, "detail lost: {after} of {before}");
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
            None,
            None,
            SOURCE,
        )
        .unwrap();
        // Overshoot on both sides of the edge, on the row y=1, against the
        // same edge rendered without sharpening.
        let flat = neutral(&image);
        let row = &out.data[(width as usize * 3)..(width as usize * 3 * 2)];
        let flat_row = &flat.data[(width as usize * 3)..(width as usize * 3 * 2)];
        assert!(
            row[3 * 3] < flat_row[3 * 3],
            "dark side dips below the flat value"
        );
        assert!(
            row[4 * 3] > flat_row[4 * 3],
            "bright side overshoots the flat value"
        );
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
            None,
            None,
            SOURCE,
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
            None,
            None,
            SOURCE,
        )
        .unwrap();
        assert_eq!((out.width, out.height), (6, 2));
        // Top-left crop pixel is source pixel (3, 4) — compared through the
        // neutral rendering, since the source's own bytes are camera
        // numbers and no longer what a render outputs (ADR 0044).
        let base = neutral(&image);
        let src = &base.data[((4 * 12 + 3) * 3)..((4 * 12 + 3) * 3 + 3)];
        assert_eq!(&out.data[0..3], src);
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let settings = Settings {
            schema: CURRENT_SCHEMA + 1,
            ..Settings::default()
        };
        assert!(matches!(
            render(&test_image(), &settings, None, None, None, SOURCE),
            Err(LeylineError::NewerSettings { .. })
        ));
    }

    #[test]
    fn a_stage_version_this_engine_does_not_implement_is_refused() {
        // The rendering half of the §3.4 guard: a revision written by a
        // newer engine is refused, never rendered without the stage its
        // author saw.
        let settings = Settings {
            stages: leyline_core::StageVersions::from([("sharpen".to_owned(), 99)]),
            sharpening: Sharpening {
                amount: 40,
                radius: 1.0,
            },
            ..Settings::default()
        };
        assert!(matches!(
            render(&test_image(), &settings, None, None, None, SOURCE),
            Err(LeylineError::UnknownStage { version: 99, .. })
        ));
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
            None,
            None,
            SOURCE,
        )
        .unwrap_err();
        assert!(matches!(err, LeylineError::InvalidSettings(_)));
    }
}
