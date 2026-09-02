//! Text watermark: the recipe field, and the compositing that draws it
//! (ADR 0034, ADR 0051).
//!
//! A watermark is a *decoration of the output*, not an edit: it lives in
//! [`crate::ExportSettings`] beside the format and the quality, never in a
//! develop revision, and it is drawn at the very end — after everything that
//! shapes the image, immediately before the encoder sees the pixels.
//!
//! Text only, by ADR 0034: a logo would first need the project to decide where
//! an image resource lives and how a portable library references it.

use ab_glyph::{Font, FontRef, Glyph, ScaleFont};
use serde::{Deserialize, Serialize};

use crate::ExportError;

/// The embedded typeface (ADR 0051 §2). Embedded rather than looked up on the
/// system so the same recipe draws the same watermark on every machine.
pub(crate) const DEJAVU_SANS: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

/// A text watermark composited onto an export (ADR 0034).
///
/// Every field has a default, so a recipe naming only `text` is complete —
/// which is what the common case (a copyright line, bottom right) needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watermark {
    /// The line to draw. Empty is refused: an empty watermark is the absence
    /// of one, and the absence is expressed by the field itself being absent.
    pub text: String,
    /// Which embedded typeface to draw with.
    pub font: WatermarkFont,
    /// Cap height as a percentage of the image *height*, in `(0, 50]` — so a
    /// watermark keeps its proportions whatever the export is scaled to
    /// (ADR 0051 §3).
    pub size: f64,
    /// `#RRGGBB`.
    pub color: String,
    /// Blend strength in `[0, 1]`.
    pub opacity: f64,
    /// Which corner (or the center) the text sits in.
    pub anchor: WatermarkAnchor,
}

impl Default for Watermark {
    fn default() -> Self {
        Watermark {
            text: String::new(),
            font: WatermarkFont::Sans,
            size: 3.0,
            color: "#FFFFFF".to_owned(),
            opacity: 0.7,
            anchor: WatermarkAnchor::BottomRight,
        }
    }
}

/// The typefaces a watermark can be drawn with — one, for now (ADR 0051 §2).
/// An enumeration rather than a font name so a recipe can never ask for
/// something the binary does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WatermarkFont {
    /// DejaVu Sans, embedded.
    #[default]
    Sans,
}

/// Where the text sits in the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WatermarkAnchor {
    /// The conventional place for a copyright line.
    #[default]
    BottomRight,
    /// Bottom left corner.
    BottomLeft,
    /// Top right corner.
    TopRight,
    /// Top left corner.
    TopLeft,
    /// Centered, for a visible "proof" marking.
    Center,
}

impl Watermark {
    /// Validates the recipe's own ranges, like every other export setting.
    pub fn validate(&self) -> Result<(), ExportError> {
        if self.text.trim().is_empty() {
            return Err(ExportError::InvalidSettings(
                "watermark.text must not be empty; omit `watermark` for no watermark".to_owned(),
            ));
        }
        if !(0.0..=50.0).contains(&self.size) || self.size <= 0.0 {
            return Err(ExportError::InvalidSettings(format!(
                "watermark.size must be in (0, 50] percent of the image height, got {}",
                self.size
            )));
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(ExportError::InvalidSettings(format!(
                "watermark.opacity must be in [0, 1], got {}",
                self.opacity
            )));
        }
        parse_color(&self.color)?;
        Ok(())
    }

    /// The typeface's bytes.
    fn font_bytes(&self) -> &'static [u8] {
        match self.font {
            WatermarkFont::Sans => DEJAVU_SANS,
        }
    }
}

/// Draws `watermark` onto tightly packed 8-bit RGB pixels, in place.
///
/// The caller owns the copy: the buffer handed to [`crate::encode`] is the
/// rendering of a revision, and a decoration must not be burned into it
/// (ADR 0051 §3).
///
/// A degenerate request — an image too small for a single glyph, a text that
/// rasterizes to nothing — leaves the pixels untouched rather than failing:
/// the export itself is still exactly what was asked for.
pub(crate) fn draw(
    width: u32,
    height: u32,
    rgb8: &mut [u8],
    watermark: &Watermark,
) -> Result<(), ExportError> {
    watermark.validate()?;
    let color = parse_color(&watermark.color)?;
    let font = FontRef::try_from_slice(watermark.font_bytes())
        .map_err(|e| ExportError::Encode(format!("embedded font is unusable: {e}")))?;

    // `size` is a cap height in percent of the image height; a font's `px`
    // scale is its em size, which is taller than its caps — hence the
    // conversion through the face's own metrics, so "3 %" means the same
    // thing whatever typeface is embedded.
    let cap_ratio = {
        let unscaled = font.as_scaled(font.units_per_em().unwrap_or(1000.0));
        (unscaled.ascent() / unscaled.height()).clamp(0.1, 1.0)
    };
    let px = (height as f64 * watermark.size / 100.0 / f64::from(cap_ratio)) as f32;
    if px < 1.0 {
        return Ok(());
    }
    let scaled = font.as_scaled(px);

    // Lay the line out on a horizontal baseline, one glyph after the other,
    // with kerning. No shaping beyond that: a watermark is one line of text
    // (ADR 0051 §1).
    let mut glyphs: Vec<Glyph> = Vec::new();
    let mut cursor = 0.0f32;
    let mut previous = None;
    for character in watermark.text.chars() {
        let id = font.glyph_id(character);
        if let Some(previous) = previous {
            cursor += scaled.kern(previous, id);
        }
        previous = Some(id);
        glyphs.push(id.with_scale_and_position(px, ab_glyph::point(cursor, scaled.ascent())));
        cursor += scaled.h_advance(id);
    }
    let text_width = cursor;
    let text_height = scaled.height();

    // Half the type size as the margin, never configurable: it is placement,
    // not a decision (ADR 0051 §3).
    let margin = px / 2.0;
    let (origin_x, origin_y) = origin(
        watermark.anchor,
        width as f32,
        height as f32,
        text_width,
        text_height,
        margin,
    );

    for glyph in glyphs {
        let Some(outlined) = font.outline_glyph(glyph) else {
            continue; // a space, or a character this face has no outline for
        };
        let bounds = outlined.px_bounds();
        outlined.draw(|gx, gy, coverage| {
            let x = origin_x + bounds.min.x + gx as f32;
            let y = origin_y + bounds.min.y + gy as f32;
            if x < 0.0 || y < 0.0 {
                return;
            }
            let (x, y) = (x as u32, y as u32);
            if x >= width || y >= height {
                return;
            }
            let alpha = coverage.clamp(0.0, 1.0) * watermark.opacity as f32;
            let offset = (y as usize * width as usize + x as usize) * 3;
            for (channel, value) in color.iter().enumerate() {
                let under = f32::from(rgb8[offset + channel]);
                let over = f32::from(*value);
                rgb8[offset + channel] = (under + (over - under) * alpha).round() as u8;
            }
        });
    }
    Ok(())
}

/// Top-left corner of the text box for an anchor, in pixels.
fn origin(
    anchor: WatermarkAnchor,
    width: f32,
    height: f32,
    text_width: f32,
    text_height: f32,
    margin: f32,
) -> (f32, f32) {
    let right = width - text_width - margin;
    let bottom = height - text_height - margin;
    match anchor {
        WatermarkAnchor::BottomRight => (right, bottom),
        WatermarkAnchor::BottomLeft => (margin, bottom),
        WatermarkAnchor::TopRight => (right, margin),
        WatermarkAnchor::TopLeft => (margin, margin),
        WatermarkAnchor::Center => ((width - text_width) / 2.0, (height - text_height) / 2.0),
    }
}

/// Parses `#RRGGBB`.
fn parse_color(color: &str) -> Result<[u8; 3], ExportError> {
    let bad =
        || ExportError::InvalidSettings(format!("watermark.color must be #RRGGBB, got {color:?}"));
    let hex = color.strip_prefix('#').ok_or_else(bad)?;
    if hex.len() != 6 {
        return Err(bad());
    }
    let mut channels = [0u8; 3];
    for (channel, pair) in channels.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| bad())?;
        *channel = u8::from_str_radix(pair, 16).map_err(|_| bad())?;
    }
    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A watermark whose text is drawn white at full opacity, for tests that
    /// need to see the pixels move.
    fn opaque(anchor: WatermarkAnchor) -> Watermark {
        Watermark {
            text: "©".to_owned(),
            opacity: 1.0,
            size: 20.0,
            anchor,
            ..Watermark::default()
        }
    }

    /// Fully black pixels, so any change is the watermark.
    fn black(width: u32, height: u32) -> Vec<u8> {
        vec![0u8; width as usize * height as usize * 3]
    }

    #[test]
    fn a_watermark_marks_the_corner_it_is_anchored_to() {
        let (w, h) = (200u32, 100u32);
        // Which half of the frame has ink in it.
        let inked_quadrants = |anchor| {
            let mut pixels = black(w, h);
            draw(w, h, &mut pixels, &opaque(anchor)).unwrap();
            let mut left = 0u32;
            let mut right = 0u32;
            let mut top = 0u32;
            let mut bottom = 0u32;
            for y in 0..h {
                for x in 0..w {
                    if pixels[(y as usize * w as usize + x as usize) * 3] > 0 {
                        if x < w / 2 {
                            left += 1;
                        } else {
                            right += 1;
                        }
                        if y < h / 2 {
                            top += 1;
                        } else {
                            bottom += 1;
                        }
                    }
                }
            }
            (left, right, top, bottom)
        };

        let (left, right, top, bottom) = inked_quadrants(WatermarkAnchor::BottomRight);
        assert!(
            right > left && bottom > top,
            "{left}/{right} {top}/{bottom}"
        );
        let (left, right, top, bottom) = inked_quadrants(WatermarkAnchor::TopLeft);
        assert!(
            left > right && top > bottom,
            "{left}/{right} {top}/{bottom}"
        );
    }

    #[test]
    fn opacity_scales_the_ink_and_zero_leaves_the_image_alone() {
        let (w, h) = (200u32, 100u32);
        let mut half = black(w, h);
        draw(
            w,
            h,
            &mut half,
            &Watermark {
                opacity: 0.5,
                ..opaque(WatermarkAnchor::Center)
            },
        )
        .unwrap();
        let mut none = black(w, h);
        draw(
            w,
            h,
            &mut none,
            &Watermark {
                opacity: 0.0,
                ..opaque(WatermarkAnchor::Center)
            },
        )
        .unwrap();
        assert_eq!(none, black(w, h), "a transparent watermark draws nothing");
        let brightest = half.iter().copied().max().unwrap();
        assert!(
            (100..=160).contains(&brightest),
            "half-opacity white on black should peak near 128, got {brightest}"
        );
    }

    #[test]
    fn the_color_is_the_one_asked_for() {
        let (w, h) = (200u32, 100u32);
        let mut pixels = black(w, h);
        draw(
            w,
            h,
            &mut pixels,
            &Watermark {
                color: "#FF0000".to_owned(),
                ..opaque(WatermarkAnchor::Center)
            },
        )
        .unwrap();
        let reds = pixels.chunks_exact(3).filter(|p| p[0] > 200).count();
        assert!(reds > 0, "expected red ink");
        assert!(
            pixels.chunks_exact(3).all(|p| p[1] == 0 && p[2] == 0),
            "a red watermark must not touch the other channels"
        );
    }

    #[test]
    fn an_image_too_small_for_a_glyph_is_left_untouched() {
        let (w, h) = (4u32, 4u32);
        let mut pixels = black(w, h);
        draw(
            w,
            h,
            &mut pixels,
            &Watermark {
                size: 0.5,
                ..opaque(WatermarkAnchor::Center)
            },
        )
        .unwrap();
        assert_eq!(pixels, black(w, h));
    }

    #[test]
    fn out_of_range_recipes_are_refused_by_name() {
        let cases = [
            Watermark {
                text: "   ".to_owned(),
                ..Watermark::default()
            },
            Watermark {
                size: 0.0,
                ..opaque(WatermarkAnchor::Center)
            },
            Watermark {
                size: 60.0,
                ..opaque(WatermarkAnchor::Center)
            },
            Watermark {
                opacity: 1.5,
                ..opaque(WatermarkAnchor::Center)
            },
            Watermark {
                color: "white".to_owned(),
                ..opaque(WatermarkAnchor::Center)
            },
            Watermark {
                color: "#FFF".to_owned(),
                ..opaque(WatermarkAnchor::Center)
            },
        ];
        for case in cases {
            assert!(case.validate().is_err(), "expected {case:?} to be refused");
        }
    }

    #[test]
    fn colors_parse_as_six_hex_digits() {
        assert_eq!(parse_color("#FFFFFF").unwrap(), [255, 255, 255]);
        assert_eq!(parse_color("#0a0B0c").unwrap(), [10, 11, 12]);
        assert!(parse_color("#GGGGGG").is_err());
    }
}
