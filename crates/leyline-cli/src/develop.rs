//! Commands that develop a photo: `develop` and its local adjustments, reprocessing,
//! history, versions, camera profiles and LUTs.

use super::*;

/// Imports a `.dcp` camera profile into the library (ADR 0035). The
/// library-relative path it prints is what `develop <v> camera-profile
/// <path>` takes.
pub(crate) fn camera_profile(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline camera-profile <library> <file.dcp>".to_owned());
    };
    let imported = open(root)?
        .import_camera_profile(Path::new(source))
        .map_err(|e| e.to_string())?;
    println!("{}  {}", imported.relative_path, imported.checksum);
    // The matrix path is implemented to spec but unvalidated against
    // Adobe's own renders (`docs/pipeline.md` process 11) — say so at the
    // point the user opts in, not only in the docs.
    eprintln!(
        "warning: camera profiles are experimental; their colors have not been validated \
         against reference renders"
    );
    Ok(())
}

pub(crate) fn camera_profiles(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline camera-profiles <library>".to_owned());
    };
    let profiles = open(root)?.camera_profiles().map_err(|e| e.to_string())?;
    for profile in &profiles {
        println!("{}  {}", profile.relative_path, profile.checksum);
    }
    println!("{} camera profile(s)", profiles.len());
    Ok(())
}

/// Imports a `.cube` LUT into the library (ADR 0053 §1).
pub(crate) fn lut_import(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, source] = positional.as_slice() else {
        return Err("usage: leyline lut <library> <file.cube>".to_owned());
    };
    let imported = open(root)?
        .import_lut(Path::new(source))
        .map_err(|e| e.to_string())?;
    println!("{}  {}", imported.relative_path, imported.checksum);
    Ok(())
}

pub(crate) fn luts(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root] = positional.as_slice() else {
        return Err("usage: leyline luts <library>".to_owned());
    };
    let luts = open(root)?.luts().map_err(|e| e.to_string())?;
    for lut in &luts {
        println!("{}  {}", lut.relative_path, lut.checksum);
    }
    println!("{} LUT(s)", luts.len());
    Ok(())
}

pub(crate) fn develop(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version, param, rest @ ..] = positional.as_slice() else {
        return Err("usage: leyline develop <library> <version-id> <param> <value...>".to_owned());
    };
    let version = VersionId::new(
        version
            .parse()
            .map_err(|_| format!("bad version id {version:?}"))?,
    );
    let at = |i: usize| -> Result<&str, String> {
        rest.get(i)
            .map(String::as_str)
            .ok_or_else(|| format!("{param} expects {} value(s)", i + 1))
    };
    let float_at = |i: usize| -> Result<f64, String> {
        let v = at(i)?;
        v.parse().map_err(|_| format!("bad decimal {v:?}"))
    };
    let int_at = |i: usize| -> Result<i32, String> {
        let v = at(i)?;
        v.parse().map_err(|_| format!("bad integer {v:?}"))
    };

    let library = open(root)?;
    let mut session = library.edit(version).map_err(|e| e.to_string())?;

    let (param, value) = match param.as_str() {
        "exposure" => (Param::Exposure, Value::Float(float_at(0)?)),
        "rotation" => (Param::Rotation, Value::Float(float_at(0)?)),
        "contrast" => (Param::Contrast, Value::Int(int_at(0)?)),
        "highlights" => (Param::Highlights, Value::Int(int_at(0)?)),
        "shadows" => (Param::Shadows, Value::Int(int_at(0)?)),
        "whites" => (Param::Whites, Value::Int(int_at(0)?)),
        "blacks" => (Param::Blacks, Value::Int(int_at(0)?)),
        "clarity" => (Param::Clarity, Value::Int(int_at(0)?)),
        "texture" => (Param::Texture, Value::Int(int_at(0)?)),
        "dehaze" => (Param::Dehaze, Value::Int(int_at(0)?)),
        "moire" => (Param::Moire, Value::Int(int_at(0)?)),
        "highlight-rolloff" => (Param::HighlightRolloff, Value::Int(int_at(0)?)),
        "highlight-reconstruction" => {
            let mode = match at(0)? {
                "clip" | "none" => HighlightReconstruction::Clip,
                "blend" => HighlightReconstruction::Blend,
                "rebuild" => HighlightReconstruction::Rebuild,
                other => {
                    return Err(format!(
                        "unknown highlight reconstruction {other:?}, expected clip/blend/rebuild"
                    ));
                }
            };
            (
                Param::HighlightReconstruction,
                Value::HighlightReconstruction(mode),
            )
        }
        "demosaic" => {
            let algorithm = match at(0)? {
                "ahd" => Demosaic::Ahd,
                "vng" => Demosaic::Vng,
                "dcb" => Demosaic::Dcb,
                "dht" => Demosaic::Dht,
                other => {
                    return Err(format!(
                        "unknown demosaic algorithm {other:?}, expected ahd/vng/dcb/dht"
                    ));
                }
            };
            (Param::Demosaic, Value::Demosaic(algorithm))
        }
        // Four values in one word, like `white-balance`: a shape and a
        // strength are one tool (ADR 0090 §2), and the three shape values
        // mean nothing without the amount they modulate.
        "vignette" => (
            Param::Vignette,
            Value::Vignette(leyline_sdk::Vignette {
                amount: int_at(0)?,
                midpoint: rest.get(1).map_or(Ok(50), |_| int_at(1))?,
                roundness: rest.get(2).map_or(Ok(0), |_| int_at(2))?,
                feather: rest.get(3).map_or(Ok(50), |_| int_at(3))?,
            }),
        ),
        "grain" => (
            Param::Grain,
            Value::Grain(leyline_sdk::Grain {
                amount: int_at(0)?,
                size: rest.get(1).map_or(Ok(25), |_| int_at(1))?,
                roughness: rest.get(2).map_or(Ok(50), |_| int_at(2))?,
                color: rest.get(3).map_or(Ok(0), |_| int_at(3))?,
            }),
        ),
        "vibrance" => (Param::Vibrance, Value::Int(int_at(0)?)),
        "saturation" => (Param::Saturation, Value::Int(int_at(0)?)),
        "monochrome" => (
            Param::Monochrome,
            Value::Bool(match at(0)? {
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                other => return Err(format!("monochrome takes on or off, got {other:?}")),
            }),
        ),
        "white-balance" => {
            let wb = match at(0)? {
                "none" => None,
                // A fixed preset by name (ADR 0091 §4): the same table
                // Studio shows as chips.
                name if leyline_sdk::WHITE_BALANCE_PRESETS
                    .iter()
                    .any(|p| p.name == name) =>
                {
                    #[allow(
                        clippy::expect_used,
                        reason = "the match guard just found this name in the same table"
                    )]
                    let preset = leyline_sdk::WHITE_BALANCE_PRESETS
                        .iter()
                        .find(|p| p.name == name)
                        .expect("just matched");
                    Some(WhiteBalance {
                        temperature: preset.temperature,
                        tint: preset.tint,
                    })
                }
                _ => Some(WhiteBalance {
                    temperature: int_at(0)?
                        .try_into()
                        .map_err(|_| "temperature must be positive".to_owned())?,
                    tint: int_at(1)?,
                }),
            };
            (Param::WhiteBalance, Value::WhiteBalance(wb))
        }
        "camera-profile" => {
            let profile = match at(0)? {
                "none" | "reset" => None,
                state @ ("on" | "off") => {
                    let mut profile =
                        session.settings().camera_profile.clone().ok_or_else(|| {
                            "no camera profile referenced yet; pass a library-relative path first"
                                .to_owned()
                        })?;
                    profile.enabled = state == "on";
                    Some(profile)
                }
                // The checksum is never typed by hand: it comes from the
                // engine's own listing of what has been imported, so the
                // revision records the bytes that are on disk right now.
                path => {
                    let imported = library
                        .camera_profiles()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|p| p.relative_path == path)
                        .ok_or_else(|| {
                            format!(
                                "unknown camera profile {path:?}; \
                                 run `leyline camera-profiles <library>` to list them"
                            )
                        })?;
                    Some(CameraProfile {
                        enabled: true,
                        path: imported.relative_path,
                        checksum: imported.checksum,
                    })
                }
            };
            (Param::CameraProfile, Value::CameraProfile(profile))
        }
        // The same shape as `camera-profile`: the checksum is never typed by
        // hand, it comes from the engine's own listing of what was imported
        // (ADR 0053 §1).
        "lut" => {
            let lut = match at(0)? {
                "none" | "reset" => None,
                state @ ("on" | "off") => {
                    let mut lut = session.settings().lut.clone().ok_or_else(|| {
                        "no LUT referenced yet; pass a library-relative path first".to_owned()
                    })?;
                    lut.enabled = state == "on";
                    Some(lut)
                }
                path => {
                    let imported = library
                        .luts()
                        .map_err(|e| e.to_string())?
                        .into_iter()
                        .find(|l| l.relative_path == path)
                        .ok_or_else(|| {
                            format!(
                                "unknown LUT {path:?}; run `leyline luts <library>` to list them"
                            )
                        })?;
                    // A path given alone keeps whatever dose was already set,
                    // so swapping looks does not silently reset it.
                    let strength = session
                        .settings()
                        .lut
                        .as_ref()
                        .map_or(100, |previous| previous.strength);
                    Some(Lut {
                        enabled: true,
                        path: imported.relative_path,
                        checksum: imported.checksum,
                        strength,
                    })
                }
            };
            (Param::Lut, Value::Lut(lut))
        }
        "lut-strength" => {
            let mut lut = session
                .settings()
                .lut
                .clone()
                .ok_or_else(|| "no LUT referenced yet; pass a path first".to_owned())?;
            lut.strength = int_at(0)?;
            (Param::Lut, Value::Lut(Some(lut)))
        }
        "lens-correction" => {
            let enabled = match at(0)? {
                "on" => true,
                "off" => false,
                other => return Err(format!("expected on/off, got {other:?}")),
            };
            // The measured coefficients survive the switch: they are not
            // Lensfun's, and turning the profile correction off is not a
            // reason to lose them (ADR 0111 §5).
            (
                Param::LensCorrection,
                Value::LensCorrection(LensCorrection {
                    enabled,
                    ..session.settings().lens_correction.clone()
                }),
            )
        }
        // Manual transverse chromatic aberration (ADR 0111): two percents of
        // the radius, as `auto-tca` measures them.
        "lens-tca" => {
            let lens = LensCorrection {
                tca_red: float_at(0)?,
                tca_blue: float_at(1)?,
                ..session.settings().lens_correction.clone()
            };
            (Param::LensCorrection, Value::LensCorrection(lens))
        }
        // Defringe (ADR 0113): two amounts, the bands are fixed.
        "defringe" => (
            Param::Defringe,
            Value::Defringe(Defringe {
                purple: int_at(0)?,
                green: rest.get(1).map_or(Ok(0), |_| int_at(1))?,
            }),
        ),
        "noise-reduction" => (
            Param::NoiseReduction,
            Value::NoiseReduction(NoiseReduction {
                luminance: int_at(0)?,
                color: int_at(1)?,
            }),
        ),
        "sharpening" => (
            Param::Sharpening,
            Value::Sharpening(Sharpening {
                amount: int_at(0)?,
                radius: float_at(1)?,
                // Optional (ADR 0096): every script written against two
                // arguments keeps working and keeps meaning what it meant.
                masking: match rest.get(2) {
                    Some(_) => int_at(2)?,
                    None => 0,
                },
            }),
        ),
        "perspective" => {
            let perspective = match at(0)? {
                "none" | "reset" => None,
                _ => Some(Perspective {
                    vertical: int_at(0)?,
                    horizontal: int_at(1)?,
                }),
            };
            (Param::Perspective, Value::Perspective(perspective))
        }
        "crop" => {
            let crop = match at(0)? {
                "none" | "reset" => None,
                _ => Some(Crop {
                    x: float_at(0)? / 100.0,
                    y: float_at(1)? / 100.0,
                    width: float_at(2)? / 100.0,
                    height: float_at(3)? / 100.0,
                }),
            };
            (Param::Crop, Value::Crop(crop))
        }
        // Levels (ADR 0164): input black, input white and gamma, and the
        // output limits optionally; `reset` puts the five back to neutral.
        "levels" => {
            let levels = if at(0)? == "reset" {
                Levels::default()
            } else {
                Levels {
                    input_black: int_at(0)?,
                    input_white: int_at(1)?,
                    gamma: float_at(2)?,
                    output_black: match rest.get(3) {
                        Some(_) => int_at(3)?,
                        None => 0,
                    },
                    output_white: match rest.get(4) {
                        Some(_) => int_at(4)?,
                        None => 255,
                    },
                }
            };
            (Param::Levels, Value::Levels(levels))
        }
        // The parametric curve (ADR 0137): seven numbers in one verb, since
        // the stage takes one value and a partial one has no meaning.
        "parametric-curve" => {
            let mut curve = session.settings().parametric_curve;
            for (index, field) in [
                "shadows",
                "darks",
                "lights",
                "highlights",
                "shadow-split",
                "midtone-split",
                "highlight-split",
            ]
            .into_iter()
            .enumerate()
            {
                // Anything not given keeps its current value, so one field
                // is changed without restating the other six.
                let Some(raw) = rest.get(index) else { break };
                if raw == "-" {
                    continue;
                }
                let value: i32 = raw
                    .parse()
                    .map_err(|_| format!("bad {field} {raw:?}, expected a whole number"))?;
                match field {
                    "shadows" => curve.shadows = value,
                    "darks" => curve.darks = value,
                    "lights" => curve.lights = value,
                    "highlights" => curve.highlights = value,
                    "shadow-split" => curve.shadow_split = value,
                    "midtone-split" => curve.midtone_split = value,
                    _ => curve.highlight_split = value,
                }
            }
            (Param::ParametricCurve, Value::ParametricCurve(curve))
        }
        "tone-curve" => {
            // An optional channel name comes first (ADR 0098 §4); absent,
            // the points are the master curve, so every invocation written
            // before this keeps meaning what it meant.
            let channel = match at(0)? {
                name @ ("master" | "red" | "green" | "blue") => Some(name.to_owned()),
                _ => None,
            };
            let points_from = usize::from(channel.is_some());
            let reset = at(points_from)? == "reset";
            let points = if reset {
                Vec::new()
            } else {
                rest[points_from..]
                    .iter()
                    .map(|p| {
                        let (x, y) = p
                            .split_once(',')
                            .ok_or_else(|| format!("bad point {p:?}, expected x,y"))?;
                        Ok(CurvePoint {
                            x: x.parse().map_err(|_| format!("bad decimal {x:?}"))?,
                            y: y.parse().map_err(|_| format!("bad decimal {y:?}"))?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?
            };
            // A channel curve is written onto the curve already stored, so
            // setting the red one does not clear the master or the blue.
            let mut curve = session.settings().tone_curve.clone();
            match channel.as_deref() {
                Some("red") => curve.red = points,
                Some("green") => curve.green = points,
                Some("blue") => curve.blue = points,
                _ => curve.points = points,
            }
            (Param::ToneCurve, Value::ToneCurve(curve))
        }
        // One handle appended per call, or `reshape reset` to clear the
        // list — the shape `red-eye` and `spot-removal` already have
        // (ADR 0109 §2).
        "reshape" => {
            let points = if at(0)? == "reset" {
                Vec::new()
            } else {
                let mut points = session.settings().reshape.clone();
                points.push(ReshapePoint {
                    from: Point {
                        x: float_at(0)? / 100.0,
                        y: float_at(1)? / 100.0,
                    },
                    to: Point {
                        x: float_at(2)? / 100.0,
                        y: float_at(3)? / 100.0,
                    },
                    radius: float_at(4)? / 100.0,
                    strength: rest.get(5).map_or(Ok(1.0), |_| float_at(5))?,
                });
                points
            };
            (Param::Reshape, Value::Reshape(points))
        }
        "backdrop" => {
            // The whole extension at once, written exactly as
            // `settings_json` stores it, or `reset` to remove it (ADR 0172
            // §5). Subjects are protected afterwards by `detect --backdrop`,
            // so a payload is about polygons, not masks.
            let backdrop = match at(0)? {
                "reset" => Backdrop::default(),
                payload => {
                    let json = match payload.strip_prefix('@') {
                        Some(path) => std::fs::read_to_string(path)
                            .map_err(|e| format!("cannot read {path}: {e}"))?,
                        None => payload.to_owned(),
                    };
                    serde_json::from_str(&json).map_err(|e| format!("bad backdrop payload: {e}"))?
                }
            };
            (Param::Backdrop, Value::Backdrop(backdrop))
        }
        "red-eye" => {
            // Same shape as `spot-removal` below, one entry appended per
            // call, or `red-eye reset` to clear the list (ADR 0103).
            let eyes = if at(0)? == "reset" {
                Vec::new()
            } else {
                let mut eyes = session.settings().red_eye.clone();
                eyes.push(RedEye {
                    center: Point {
                        x: float_at(0)? / 100.0,
                        y: float_at(1)? / 100.0,
                    },
                    radius: float_at(2)? / 100.0,
                    feather: float_at(3)?,
                    darken: float_at(4)?,
                });
                eyes
            };
            (Param::RedEye, Value::RedEye(eyes))
        }
        "spot-removal" => {
            let spots = if at(0)? == "reset" {
                Vec::new()
            } else {
                let spot = SpotRemoval {
                    target: Point {
                        x: float_at(0)? / 100.0,
                        y: float_at(1)? / 100.0,
                    },
                    source: Point {
                        x: float_at(2)? / 100.0,
                        y: float_at(3)? / 100.0,
                    },
                    radius: float_at(4)? / 100.0,
                    feather: float_at(5)?,
                    opacity: float_at(6)?,
                };
                let mut spots = session.settings().spot_removal.clone();
                spots.push(spot);
                spots
            };
            (Param::SpotRemoval, Value::SpotRemoval(spots))
        }
        // The only command taking a JSON payload (ADR 0049 §4), and the only
        // one whose argument can stand for several edits at once — so it
        // commits on its own instead of falling through to the single
        // `set` below.
        "local-adjustment" => {
            let updates = local_adjustment_updates(
                at(0)?,
                rest.get(1).map(String::as_str),
                &session.settings().local_adjustments,
            )?;
            for (param, value) in updates {
                session.set(param, value).map_err(|e| e.to_string())?;
            }
            let revision = session.commit().map_err(|e| e.to_string())?;
            drop(session);
            println!("committed revision {revision}");
            return Ok(());
        }
        "hsl-band" => {
            const HSL_BAND_NAMES: [&str; 8] = [
                "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
            ];
            let name = at(0)?;
            let index = HSL_BAND_NAMES
                .iter()
                .position(|&n| n == name)
                .ok_or_else(|| {
                    format!(
                        "unknown hsl band {name:?}, expected one of {}",
                        HSL_BAND_NAMES.join("/")
                    )
                })?;
            let band = HslBand {
                hue: int_at(1)?,
                saturation: int_at(2)?,
                luminance: int_at(3)?,
            };
            (Param::HslBand(index), Value::HslBand(band))
        }
        "color-grading" => {
            let mut grading = session.settings().color_grading;
            match at(0)? {
                "reset" => grading = ColorGrading::default(),
                "balance" => grading.balance = int_at(1)?,
                "blending" => grading.blending = int_at(1)?,
                zone_name @ ("shadows" | "midtones" | "highlights") => {
                    let zone = ColorGradingZone {
                        hue: int_at(1)?,
                        saturation: int_at(2)?,
                        luminance: int_at(3)?,
                    };
                    match zone_name {
                        "shadows" => grading.shadows = zone,
                        "midtones" => grading.midtones = zone,
                        _ => grading.highlights = zone,
                    }
                }
                other => {
                    return Err(format!(
                        "unknown color-grading target {other:?}, expected shadows/midtones/highlights/balance/blending/reset"
                    ));
                }
            }
            (Param::ColorGrading, Value::ColorGrading(grading))
        }
        other => return Err(format!("unknown develop parameter {other:?}")),
    };

    session.set(param, value).map_err(|e| e.to_string())?;
    let revision = session.commit().map_err(|e| e.to_string())?;
    drop(session);
    println!("committed revision {revision}");
    Ok(())
}

/// Decodes the `local-adjustment` develop argument (ADR 0049 §4) into the
/// edits it stands for, against the adjustments `current`ly stored.
///
/// `action` is `reset`, `rm`, or the payload itself — a serialized
/// [`LocalAdjustment`] exactly as `settings_json` holds it, or `@path` to read
/// that payload from a file (a brush stroke's dabs do not fit on a command
/// line). `argument` is the index `rm` removes, and is ignored otherwise.
///
/// A payload appends, so its index is the current length — the append
/// convention of [`Value::LocalAdjustment`]. `reset` removes from the last
/// index down, since removing an entry shifts every later one.
pub(crate) fn local_adjustment_updates(
    action: &str,
    argument: Option<&str>,
    current: &[LocalAdjustment],
) -> Result<Vec<(Param, Value)>, String> {
    match action {
        "reset" => Ok((0..current.len())
            .rev()
            .map(|i| (Param::LocalAdjustment(i), Value::LocalAdjustment(None)))
            .collect()),
        "rm" => {
            let index =
                argument.ok_or_else(|| "local-adjustment rm expects an index".to_owned())?;
            let index: usize = index.parse().map_err(|_| format!("bad index {index:?}"))?;
            if index >= current.len() {
                return Err(format!(
                    "no local adjustment at index {index}; there {} {}",
                    if current.len() == 1 { "is" } else { "are" },
                    current.len()
                ));
            }
            Ok(vec![(
                Param::LocalAdjustment(index),
                Value::LocalAdjustment(None),
            )])
        }
        payload => {
            let json = match payload.strip_prefix('@') {
                Some(path) => {
                    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?
                }
                None => payload.to_owned(),
            };
            let adjustment: LocalAdjustment = serde_json::from_str(&json)
                .map_err(|e| format!("bad local adjustment payload: {e}"))?;
            Ok(vec![(
                Param::LocalAdjustment(current.len()),
                Value::LocalAdjustment(Some(adjustment)),
            )])
        }
    }
}

/// Migrates each version to the engine's current stage versions
/// (`docs/engine-api.md` §10.4): same parameter values, re-rendered under a
/// newer process contract (e.g. picking up lens correction).
pub(crate) fn reprocess(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, versions @ ..] = positional.as_slice() else {
        return Err("usage: leyline reprocess <library> <version-id>...".to_owned());
    };
    if versions.is_empty() {
        return Err("usage: leyline reprocess <library> <version-id>...".to_owned());
    }
    let ids = versions
        .iter()
        .map(|v| {
            v.parse()
                .map(VersionId::new)
                .map_err(|_| format!("bad version id {v:?}"))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let library = open(root)?;
    let report = library
        .reprocess(&ids, |_, _| {})
        .map_err(|e| e.to_string())?;
    println!(
        "reprocessed {}, already current {}, failed {}",
        report.reprocessed.len(),
        report.already_current.len(),
        report.failed.len()
    );
    for failed in &report.failed {
        println!("  failed: version {} — {}", failed.version, failed.reason);
    }
    Ok(())
}

pub(crate) fn history(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version] = positional.as_slice() else {
        return Err("usage: leyline history <library> <version-id>".to_owned());
    };
    let version = VersionId::new(
        version
            .parse()
            .map_err(|_| format!("bad version id {version:?}"))?,
    );
    let library = open(root)?;
    let chain = library
        .catalog()
        .version_history(version)
        .map_err(|e| e.to_string())?;
    // What a revision changed is only sayable against the one before it
    // (ADR 0142 §2), so the chain is read backwards to build the lines — and
    // printed head first, the order `version_history` documents and the
    // order a `HEAD` marker on the first line means.
    let parsed: Vec<Settings> = chain
        .iter()
        .map(|row| Settings::parse(&row.settings_json).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    for (index, (row, settings)) in chain.iter().zip(&parsed).enumerate() {
        let marker = if index == 0 { "HEAD" } else { "    " };
        let stages = settings
            .stages
            .iter()
            .map(|(name, version)| format!("{name}:{version}"))
            .collect::<Vec<_>>()
            .join(" ");
        let changed = match parsed.get(index + 1) {
            None => "import".to_owned(),
            Some(before) => {
                let keys = leyline_sdk::changed_settings(before, settings);
                if keys.is_empty() {
                    "no change".to_owned()
                } else {
                    keys.join(" ")
                }
            }
        };
        // Unknown origins (ADR 0154 §6) print nothing rather than a guess.
        let origin = row
            .origin
            .as_ref()
            .map(|origin| format!("  by {origin}"))
            .unwrap_or_default();
        println!(
            "{marker} r{:<6} {changed}{origin}  (schema {}, stages [{stages}])",
            row.revision, settings.schema
        );
    }
    Ok(())
}

/// Parses a comma-separated list of preset group names (`docs/presets.md` §3.1).
pub(crate) fn groups(value: &str) -> Result<Vec<SettingsGroup>, String> {
    value
        .split(',')
        .map(|name| match name {
            "white_balance" => Ok(SettingsGroup::WhiteBalance),
            "tone" => Ok(SettingsGroup::Tone),
            "presence" => Ok(SettingsGroup::Presence),
            "tone_curve" => Ok(SettingsGroup::ToneCurve),
            "color_mixer" => Ok(SettingsGroup::ColorMixer),
            "color_grading" => Ok(SettingsGroup::ColorGrading),
            "camera_profile" => Ok(SettingsGroup::CameraProfile),
            "creative_lut" => Ok(SettingsGroup::CreativeLut),
            "effects" => Ok(SettingsGroup::Effects),
            "lens_correction" => Ok(SettingsGroup::LensCorrection),
            "detail" => Ok(SettingsGroup::Detail),
            "rendering" => Ok(SettingsGroup::Rendering),
            "geometry" => Ok(SettingsGroup::Geometry),
            "reshape" => Ok(SettingsGroup::Reshape),
            "spot_removal" => Ok(SettingsGroup::SpotRemoval),
            "red_eye" => Ok(SettingsGroup::RedEye),
            "local_adjustments" => Ok(SettingsGroup::LocalAdjustments),
            "backdrop" => Ok(SettingsGroup::Backdrop),
            other => Err(format!("unknown preset group {other:?}")),
        })
        .collect()
}

/// Lists the develop versions of a version's asset (`docs/adr/0094` §3).
pub(crate) fn versions(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version] = positional.as_slice() else {
        return Err("usage: leyline versions <library> <version-id>".to_owned());
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let catalog = library.catalog();
    let asset = catalog.version_asset(version).map_err(|e| e.to_string())?;
    let current = catalog.current_version(asset).map_err(|e| e.to_string())?;
    for info in catalog.versions(asset).map_err(|e| e.to_string())? {
        let marker = if info.version == current { "*" } else { " " };
        println!("{marker} {:<8} {}", info.version.get(), info.name);
    }
    Ok(())
}

/// Branches a second development and makes it current (§3).
pub(crate) fn version_create(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let (root, version, name) = match positional.as_slice() {
        [root, version] => (root, version, None),
        [root, version, name] => (root, version, Some(name.clone())),
        _ => {
            return Err("usage: leyline version-create <library> <version-id> [name]".to_owned());
        }
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    let name = match name {
        Some(name) => name,
        None => format!(
            "Version {}",
            library
                .catalog()
                .versions(asset)
                .map_err(|e| e.to_string())?
                .len()
                + 1
        ),
    };
    let created = library
        .catalog_mut()
        .create_version(version, &name, None)
        .map_err(|e| e.to_string())?;
    library
        .catalog_mut()
        .set_current_version(asset, created)
        .map_err(|e| e.to_string())?;
    println!("created version {created} ({name}), now current");
    Ok(())
}

/// Points the asset at another of its developments (§3). The graph is not
/// rewritten: a switch moves a pointer (ADR 0008).
pub(crate) fn version_switch(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version] = positional.as_slice() else {
        return Err("usage: leyline version-switch <library> <version-id>".to_owned());
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    library
        .catalog_mut()
        .set_current_version(asset, version)
        .map_err(|e| e.to_string())?;
    println!("version {version} is now current");
    Ok(())
}

/// Prints what the range eyedropper reads at one point (`docs/adr/0093`):
/// display-axis luminance and hue, for scripts that write their own
/// `LocalAdjustment` payloads.
pub(crate) fn sample_range(args: &[String]) -> Result<(), String> {
    let (positional, _) = parse(args, &[])?;
    let [root, version, point] = positional.as_slice() else {
        return Err("usage: leyline sample-range <library> <version-id> <x,y>".to_owned());
    };
    let library = open(root)?;
    let version = VersionId::new(version.parse().map_err(|_| "version id must be a number")?);
    let asset = library
        .catalog()
        .version_asset(version)
        .map_err(|e| e.to_string())?;
    let (x, y) = point
        .split_once(',')
        .and_then(|(x, y)| Some((x.trim().parse::<f64>().ok()?, y.trim().parse::<f64>().ok()?)))
        .ok_or_else(|| format!("expected x,y in [0,1], got {point:?}"))?;
    let sample = library
        .sample_range(asset, x, y)
        .map_err(|e| e.to_string())?;
    println!("luminance {:.3}  hue {:.1}", sample.luminance, sample.hue);
    Ok(())
}

#[cfg(test)]
mod tests {
    use leyline_sdk::{LocalAdjustmentValues, Mask};

    use super::*;

    /// One stored radial adjustment, as a payload and as a value.
    fn radial_json() -> &'static str {
        r#"{"mask":{"type":"radial","cx":0.5,"cy":0.5,"rx":0.3,"ry":0.2,
            "angle":0,"feather":0.5,"inverted":false},
            "opacity":1,"adjustments":{"exposure":-0.5}}"#
    }

    fn radial() -> LocalAdjustment {
        LocalAdjustment {
            mask: Mask::Radial {
                cx: 0.5,
                cy: 0.5,
                rx: 0.3,
                ry: 0.2,
                angle: 0.0,
                feather: 0.5,
                inverted: false,
            },
            range: None,
            opacity: 1.0,
            adjustments: LocalAdjustmentValues {
                exposure: Some(-0.5),
                ..LocalAdjustmentValues::default()
            },
        }
    }

    /// ADR 0108's five keys travel the payload path with no CLI code of
    /// their own — the payload *is* `settings_json`, so a new value in the
    /// document is a new key here by construction. This test is what keeps
    /// that claim honest.
    #[test]
    fn a_payload_carries_the_five_neighbourhood_values() {
        let json = r#"{"mask":{"type":"everything"},"opacity":0.8,
            "adjustments":{"clarity":30,"texture":-45,"sharpness":20,
            "noise_luminance":15,"noise_color":10}}"#;
        let updates = local_adjustment_updates(json, None, &[]).unwrap();
        let [(_, Value::LocalAdjustment(Some(entry)))] = updates.as_slice() else {
            panic!("expected one appended entry, got {updates:?}");
        };
        assert_eq!(entry.adjustments.clarity, Some(30));
        assert_eq!(entry.adjustments.texture, Some(-45));
        assert_eq!(entry.adjustments.sharpness, Some(20));
        assert_eq!(entry.adjustments.noise_luminance, Some(15));
        assert_eq!(entry.adjustments.noise_color, Some(10));
    }

    #[test]
    fn a_payload_appends_at_the_current_length() {
        assert_eq!(
            local_adjustment_updates(radial_json(), None, &[]),
            Ok(vec![(
                Param::LocalAdjustment(0),
                Value::LocalAdjustment(Some(radial()))
            )])
        );
        let existing = [radial(), radial()];
        assert_eq!(
            local_adjustment_updates(radial_json(), None, &existing),
            Ok(vec![(
                Param::LocalAdjustment(2),
                Value::LocalAdjustment(Some(radial()))
            )])
        );
    }

    #[test]
    fn an_at_prefixed_payload_is_read_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mask.json");
        std::fs::write(&file, radial_json()).unwrap();
        assert_eq!(
            local_adjustment_updates(&format!("@{}", file.display()), None, &[]),
            Ok(vec![(
                Param::LocalAdjustment(0),
                Value::LocalAdjustment(Some(radial()))
            )])
        );
        assert!(
            local_adjustment_updates("@no/such/file.json", None, &[])
                .unwrap_err()
                .starts_with("cannot read ")
        );
    }

    #[test]
    fn a_malformed_payload_is_named_rather_than_ignored() {
        let error = local_adjustment_updates("{\"mask\":\"radial\"}", None, &[]).unwrap_err();
        assert!(
            error.starts_with("bad local adjustment payload: "),
            "{error}"
        );
    }

    #[test]
    fn rm_removes_one_index_and_refuses_a_missing_one() {
        assert_eq!(
            local_adjustment_updates("rm", Some("1"), &[radial(), radial()]),
            Ok(vec![(
                Param::LocalAdjustment(1),
                Value::LocalAdjustment(None)
            )])
        );
        assert_eq!(
            local_adjustment_updates("rm", Some("2"), &[radial(), radial()]),
            Err("no local adjustment at index 2; there are 2".to_owned())
        );
        assert_eq!(
            local_adjustment_updates("rm", None, &[radial()]),
            Err("local-adjustment rm expects an index".to_owned())
        );
        assert!(
            local_adjustment_updates("rm", Some("x"), &[radial()])
                .unwrap_err()
                .starts_with("bad index ")
        );
    }

    /// Removing shifts every later index down, so a reset that walked
    /// forwards would run off the end of a shrinking list.
    #[test]
    fn reset_removes_from_the_last_index_down() {
        assert_eq!(
            local_adjustment_updates("reset", None, &[radial(), radial(), radial()]),
            Ok(vec![
                (Param::LocalAdjustment(2), Value::LocalAdjustment(None)),
                (Param::LocalAdjustment(1), Value::LocalAdjustment(None)),
                (Param::LocalAdjustment(0), Value::LocalAdjustment(None)),
            ])
        );
        assert_eq!(local_adjustment_updates("reset", None, &[]), Ok(vec![]));
    }
}
