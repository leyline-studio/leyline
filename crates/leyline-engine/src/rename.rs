//! Renaming files on disk (ADR 0100).
//!
//! The only operation in the engine that moves a user's original file, and
//! therefore the only one where "non-destructive" has to be actively
//! defended rather than inherited from the architecture. Three rules carry
//! that weight:
//!
//! * a template yields a **filename**, never a path (§1) — renaming does
//!   not move a photo between folders;
//! * the **disk moves first** and the catalog follows only if it worked
//!   (§2) — the reverse order leaves a catalog naming a file that is not
//!   there, which is what makes a library look corrupted;
//! * a destination that already exists is **refused**, never overwritten
//!   (§2). This operation can destroy a photograph, and it declines to.

use std::path::Path;

use leyline_core::{AssetId, LeylineError, Result};

/// What one rename batch did (ADR 0100 §4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameReport {
    /// Assets renamed, with the name they now carry.
    pub renamed: Vec<RenamedAsset>,
    /// Assets left alone, with the reason.
    pub failed: Vec<FailedRename>,
}

/// One asset that was renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamedAsset {
    /// The asset.
    pub asset: AssetId,
    /// The name it had.
    pub from: String,
    /// The name it now has.
    pub to: String,
}

/// One asset a rename batch left alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedRename {
    /// The asset.
    pub asset: AssetId,
    /// Why it was not renamed, in the words the user should read.
    pub reason: String,
}

/// The facts one file offers a template (ADR 0100 §1).
#[derive(Debug, Clone, Copy)]
pub(crate) struct NameFacts<'a> {
    /// The current stem, extension excluded.
    pub stem: &'a str,
    /// Capture instant, UTC epoch milliseconds, when the file recorded one.
    pub capture_ms: Option<i64>,
    /// Position within the batch, 1-based.
    pub index: usize,
}

/// Expands a rename template into a filename stem.
///
/// Placeholders are `{name}`, `{date}`, `{time}` and `{seq}`; the extension
/// is kept by the caller and never named here. An **unknown placeholder is
/// refused** rather than left as literal text (ADR 0100 §1): `{sequence}`
/// typed instead of `{seq}` must say so, not rename ten thousand files to
/// `IMG{sequence}`.
///
/// A photograph with no capture date expands `{date}` and `{time}` to
/// nothing rather than to a made-up instant; a template that is *only*
/// those then yields an empty stem, which the caller refuses.
pub(crate) fn expand(template: &str, facts: &NameFacts<'_>) -> Result<String> {
    let refuse = |reason: String| Err(LeylineError::InvalidSettings(reason));
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return refuse(format!("unclosed '{{' in rename template {template:?}"));
        };
        let key = &after[..close];
        match key {
            "name" => out.push_str(facts.stem),
            "date" => {
                if let Some(ms) = facts.capture_ms {
                    out.push_str(&format_date(ms));
                }
            }
            "time" => {
                if let Some(ms) = facts.capture_ms {
                    out.push_str(&format_time(ms));
                }
            }
            "seq" => out.push_str(&format!("{:04}", facts.index)),
            other => {
                return refuse(format!(
                    "unknown placeholder {{{other}}} in rename template; \
                     known ones are {{name}}, {{date}}, {{time}}, {{seq}}"
                ));
            }
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);

    let out = out.trim().to_owned();
    if out.is_empty() {
        return refuse(format!(
            "rename template {template:?} produced an empty name"
        ));
    }
    // A filename, never a path (ADR 0100 §1). Both separators, since a
    // catalog written on one platform is opened on the other.
    if out.contains('/') || out.contains('\\') {
        return refuse(format!(
            "a rename template produces a file name, not a path: {out:?} contains a separator"
        ));
    }
    Ok(out)
}

/// `YYYY-MM-DD` of a UTC epoch-millisecond instant.
///
/// **UTC, not local time**, and deliberately: a filename that depended on
/// the machine's zone would give two names to one photograph depending on
/// where it was renamed, and the same batch run twice on two machines
/// would disagree. The same choice `format::capture_date` makes, for the
/// same reason.
fn format_date(ms: i64) -> String {
    let (year, month, day, _, _, _) = utc_parts(ms);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `HHMMSS` of the same instant, UTC.
fn format_time(ms: i64) -> String {
    let (_, _, _, hour, minute, second) = utc_parts(ms);
    format!("{hour:02}{minute:02}{second:02}")
}

/// Year, month, day, hour, minute, second of an epoch-millisecond instant,
/// UTC. Hand-rolled like the rest of the workspace's date arithmetic, so
/// renaming adds no dependency to a crate that already builds three ways.
fn utc_parts(ms: i64) -> (i64, u32, u32, i64, i64, i64) {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    (year, month, day, tod / 3600, tod % 3600 / 60, tod % 60)
}

/// Gregorian date from days since 1970-01-01 (Howard Hinnant's algorithm),
/// the same one `leyline-studio`'s formatter uses.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// The name a file would take: `expand`'s stem, plus the extension the file
/// already has. The extension is never renamed — it says what the file *is*.
pub(crate) fn target_name(current: &str, template: &str, facts: &NameFacts<'_>) -> Result<String> {
    let stem = expand(template, facts)?;
    match Path::new(current).extension().and_then(|e| e.to_str()) {
        Some(extension) => Ok(format!("{stem}.{extension}")),
        None => Ok(stem),
    }
}

/// Moves whichever sidecars sit beside `from` onto `to`, under both naming
/// conventions the repository reads (ADR 0100 §3).
///
/// Best effort and deliberately silent: a sidecar that cannot be moved is
/// a sidecar, and ADR 0047 §5 already makes a sidecar's trouble the
/// sidecar's own. The photograph has been renamed either way.
pub(crate) fn move_sidecars(from: &Path, to: &Path) {
    for (old, new) in crate::xmp::sidecar_candidates(from)
        .into_iter()
        .zip(crate::xmp::sidecar_candidates(to))
    {
        if old.exists() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(stem: &'a str, capture_ms: Option<i64>, index: usize) -> NameFacts<'a> {
        NameFacts {
            stem,
            capture_ms,
            index,
        }
    }

    #[test]
    fn a_template_expands_its_placeholders() {
        let f = facts("IMG_4231", Some(0), 7);
        assert_eq!(expand("{name}", &f).unwrap(), "IMG_4231");
        assert_eq!(expand("{seq}", &f).unwrap(), "0007");
        assert_eq!(
            expand("Heron-{seq}-{name}", &f).unwrap(),
            "Heron-0007-IMG_4231"
        );
        // Literal text with no placeholder at all is a legal template.
        assert_eq!(expand("fixed", &f).unwrap(), "fixed");
    }

    /// ADR 0100 §1: a typo must say so rather than land in ten thousand
    /// filenames.
    #[test]
    fn an_unknown_placeholder_is_refused() {
        let f = facts("IMG_4231", None, 1);
        let message = match expand("{sequence}", &f) {
            Err(LeylineError::InvalidSettings(message)) => message,
            other => panic!("expected a refusal, got {other:?}"),
        };
        assert!(message.contains("{sequence}"), "{message}");
        assert!(message.contains("{seq}"), "it must name the right one");

        assert!(expand("{name", &f).is_err(), "an unclosed brace is refused");
    }

    /// ADR 0100 §1: the result names a file, so a separator is refused —
    /// renaming never moves a photo to another folder.
    #[test]
    fn a_template_producing_a_path_is_refused() {
        let f = facts("IMG_4231", None, 1);
        assert!(expand("2026/{name}", &f).is_err());
        assert!(expand("sub\\{name}", &f).is_err());
        assert!(expand("   ", &f).is_err(), "an empty name is refused");
    }

    /// A photograph with no capture date yields nothing for `{date}`,
    /// rather than a made-up instant.
    #[test]
    fn a_missing_capture_date_expands_to_nothing() {
        let f = facts("IMG_4231", None, 3);
        assert_eq!(expand("{date}{name}", &f).unwrap(), "IMG_4231");
        // A template that is only a date it does not have has no name left.
        assert!(expand("{date}", &f).is_err());
    }

    #[test]
    fn the_extension_is_kept_and_never_renamed() {
        let f = facts("IMG_4231", None, 1);
        assert_eq!(
            target_name("IMG_4231.CR2", "{name}-edit", &f).unwrap(),
            "IMG_4231-edit.CR2"
        );
        // A file with no extension keeps having none.
        assert_eq!(target_name("README", "{name}", &f).unwrap(), "IMG_4231");
    }

    /// The date is formatted from the instant, zero-padded, sortable.
    #[test]
    fn the_date_is_written_sortably() {
        // UTC, so the assertion is exact and the same on every machine —
        // which is the property a filename needs (see `format_date`).
        let f = facts("x", Some(1_709_210_096_000), 1);
        assert_eq!(expand("{date}", &f).unwrap(), "2024-02-29");
        assert_eq!(expand("{time}", &f).unwrap(), "123456");
        assert_eq!(expand("{date}_{time}", &f).unwrap(), "2024-02-29_123456");
        // The epoch itself, and a date before it.
        assert_eq!(
            expand("{date}", &facts("x", Some(0), 1)).unwrap(),
            "1970-01-01"
        );
        assert_eq!(
            expand("{date}", &facts("x", Some(-86_400_000), 1)).unwrap(),
            "1969-12-31"
        );
    }
}
