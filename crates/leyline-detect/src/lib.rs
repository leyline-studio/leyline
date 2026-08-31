//! External mask detectors: the contract, the discovery and the invocation
//! ([ADR 0073](../../../docs/adr/0073-external-mask-detectors.md)).
//!
//! **This crate detects nothing.** It knows how to find the executables that
//! do, how to call one, and what shape its answer takes:
//!
//! ```text
//! <command> <args…> --image <in.png> --detector <id> --out <out.png>
//! ```
//!
//! `in.png` is a developed preview, `out.png` a 16-bit grey coverage — `0`
//! where the adjustment does not apply, `65535` where it applies fully, the
//! format [ADR 0070](../../../docs/adr/0070-stored-mask-coverage.md) §4 froze.
//! The caller turns that image into a mask with
//! `Library::store_mask_coverage`; a detector never opens the library, never
//! takes a lock, and never learns an asset identifier.
//!
//! That smallness is the point. A detector is a *separate process* exchanging
//! two files, so it links nothing of Leyline, can be written in twenty lines
//! of any language, and cannot take the application down with it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long a single detection may take before it is killed (ADR 0073 §2).
///
/// A detector runs a model, so seconds are normal and a fixed budget is the
/// only thing standing between a wedged executable and an interface that
/// never comes back. Generous on purpose: this is a safety net, not a
/// performance target.
const TIMEOUT: Duration = Duration::from_secs(120);

/// How often the wait loop looks at the child. Short enough that a fast
/// detector is not held back by the polling itself.
const POLL: Duration = Duration::from_millis(20);

/// What can go wrong between "the user clicked" and "there is a coverage
/// image on disk".
#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    /// The manifest does not declare this detection.
    #[error("this detector offers no `{0}` detection")]
    UnknownDetection(String),
    /// The executable could not be started, or the pipes could not be read.
    #[error("cannot run {command}: {source}")]
    Spawn {
        /// The command as the manifest spelled it.
        command: String,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// It ran and refused. `stderr` is passed through verbatim: the detector
    /// is the only one who knows why, and swallowing its message would leave
    /// the user with nothing.
    #[error("{command} failed ({status}){}", detail(.stderr))]
    Failed {
        /// The command as the manifest spelled it.
        command: String,
        /// Exit status, or a description of the signal that ended it.
        status: String,
        /// Whatever the detector wrote on its error stream, trimmed.
        stderr: String,
    },
    /// It ran past [`TIMEOUT`] and was killed.
    #[error("{command} did not finish within {} s", TIMEOUT.as_secs())]
    TimedOut {
        /// The command as the manifest spelled it.
        command: String,
    },
    /// It reported success and wrote nothing usable.
    #[error("{command} reported success but wrote no coverage to {}", .out.display())]
    NoOutput {
        /// The command as the manifest spelled it.
        command: String,
        /// Where the coverage was expected.
        out: PathBuf,
    },
    /// It wrote a file that is not a readable image.
    #[error("{command} wrote a coverage that is not a readable image: {reason}")]
    Unreadable {
        /// The command as the manifest spelled it.
        command: String,
        /// What the image decoder said.
        reason: String,
    },
}

/// Appends a detector's own error message, when it left one.
fn detail(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

type Result<T> = std::result::Result<T, DetectError>;

/// One detection a source offers — "the sky", "the subject".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Detection {
    /// Passed back to the executable as `--detector`.
    pub id: String,
    /// What the menu shows. The detector's own words: it is the only one who
    /// knows what its model was trained on, and translating them here would
    /// mean maintaining a list of detections nobody has installed.
    pub label: String,
}

/// One installed detector, as its manifest describes it (ADR 0073 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectorSource {
    /// Stable identifier, and the manifest's file name.
    pub id: String,
    /// Name shown next to its detections.
    pub label: String,
    /// The executable. Absolute, or a bare name to be found on `PATH`.
    pub command: PathBuf,
    /// Fixed arguments placed before the ones this crate adds.
    #[serde(default)]
    pub args: Vec<String>,
    /// What it can detect. A source offering none is not a source.
    pub detections: Vec<Detection>,
}

impl DetectorSource {
    /// Why the manifest cannot be offered, or `None` when it can.
    ///
    /// Each condition says its own name: a manifest is set aside silently
    /// at a launch, but an author asking why deserves the reason
    /// (ADR 0105 §4).
    ///
    /// A command holding a path separator must exist on disk — that is the
    /// usual case, an installer writing an absolute path, and an uninstalled
    /// detector should stop appearing in menus the moment its files are
    /// gone. A bare name is left alone: resolving `PATH` here would
    /// second-guess the operating system.
    fn unusable_reason(&self) -> Option<String> {
        if self.id.is_empty() {
            return Some("\"id\" is empty".to_owned());
        }
        if self.detections.is_empty() {
            return Some(
                "\"detections\" is empty — a source offering none is not a source".to_owned(),
            );
        }
        if self.command.as_os_str().is_empty() {
            return Some("\"command\" is empty".to_owned());
        }
        if self.command.components().count() > 1 && !self.command.is_file() {
            return Some(format!("command {} does not exist", self.command.display()));
        }
        None
    }
}

/// Where manifests live: `<user config>/Leyline/detectors`.
///
/// Per user and per machine, never inside a library: a library is portable
/// and self-contained (`docs/catalog.md` §37), and what is installed on
/// *this* computer has no business travelling with someone's photos. It is
/// the directory `recent_libraries.json` already occupies, for that same
/// reason.
pub fn manifests_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "Leyline")?;
    Some(dirs.config_dir().join("detectors"))
}

/// Every usable detector installed for this user, sorted by identifier.
///
/// Nothing here fails: no directory, no manifests, an unreadable file or a
/// malformed one all mean the same thing to a caller — no detection to
/// offer. A detector is an accessory, and an accessory does not get to break
/// a launch.
pub fn discover() -> Vec<DetectorSource> {
    manifests_dir()
        .map(|dir| discover_in(&dir))
        .unwrap_or_default()
}

/// [`discover`] over an explicit directory — what the tests use, and what a
/// packager would use to look somewhere else.
pub fn discover_in(dir: &Path) -> Vec<DetectorSource> {
    let mut sources: Vec<DetectorSource> = manifests_in(dir)
        .iter()
        .filter_map(|path| read_manifest(path).ok())
        .collect();
    sources.sort_by(|a, b| a.id.cmp(&b.id));
    sources.dedup_by(|a, b| a.id == b.id);
    sources
}

/// A manifest that was read and set aside, and why (ADR 0105 §4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejection {
    /// The file that was declined.
    pub path: PathBuf,
    /// What is wrong with it, in one sentence.
    pub reason: String,
}

/// The manifests in `dir` that [`discover_in`] declined, with the reason.
///
/// A second pass over the same directory rather than a second return value:
/// discovery runs at every launch and on a path where nobody is listening,
/// while this runs when somebody asks. Keeping them apart leaves the launch
/// exactly as cheap as it was.
pub fn rejected_in(dir: &Path) -> Vec<Rejection> {
    let mut rejections: Vec<Rejection> = manifests_in(dir)
        .into_iter()
        .filter_map(|path| {
            read_manifest(&path)
                .err()
                .map(|reason| Rejection { path, reason })
        })
        .collect();
    rejections.sort_by(|a, b| a.path.cmp(&b.path));
    rejections
}

/// Every `.json` file in `dir`, sorted; empty when the directory is not
/// there, which is the ordinary case.
fn manifests_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
}

/// Reads one manifest, or says what is wrong with it.
fn read_manifest(path: &Path) -> std::result::Result<DetectorSource, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot be read: {e}"))?;
    let source: DetectorSource =
        serde_json::from_str(&text).map_err(|e| format!("is not a valid manifest: {e}"))?;
    match source.unusable_reason() {
        Some(reason) => Err(reason),
        None => Ok(source),
    }
}

/// Runs one detection and returns the coverage it produced, as samples
/// ready for `Library::store_mask_coverage` (ADR 0105 §1).
///
/// The whole gesture in one call: a temporary file is made here, the
/// detector fills it, the answer is read through [`coverage_from_image`]
/// and the file is dropped. Clients get the samples and never handle the
/// protocol's plumbing — which is what keeps two clients from disagreeing
/// about it, and what keeps `image` out of a command-line binary that has
/// no other use for it.
pub fn detect_coverage(
    source: &DetectorSource,
    detection: &str,
    image_path: &Path,
) -> Result<(u32, u32, Vec<u16>)> {
    let command = source.command.display().to_string();
    let out = tempfile::Builder::new()
        .prefix("leyline-detected-")
        .suffix(".png")
        .tempfile()
        .map_err(|source| DetectError::Spawn {
            command: command.clone(),
            source,
        })?;
    detect(source, detection, image_path, out.path())?;
    let answer = image::open(out.path()).map_err(|error| DetectError::Unreadable {
        command,
        reason: error.to_string(),
    })?;
    Ok(coverage_from_image(&answer))
}

/// Reads a detector's answer as coverage samples (ADR 0105 §1).
///
/// **The second half of the protocol.** The first half says how a detector
/// is called; this says how what it wrote is understood, and it lives here
/// rather than in a client so that two clients cannot quietly disagree
/// about what a detector's output means.
///
/// An **opaque** image is read as grey, through Rec. 709 luma — the same
/// axis the develop panel's histogram uses, because a mask's grey is a
/// *display* grey and not linear light. An image carrying transparency is
/// read from its **alpha** channel instead, which is what a detector
/// writing an RGBA cut-out produces.
#[must_use]
pub fn coverage_from_image(image: &image::DynamicImage) -> (u32, u32, Vec<u16>) {
    let rgba = image.to_rgba16();
    let (width, height) = (rgba.width(), rgba.height());
    let opaque = rgba.pixels().all(|p| p.0[3] == u16::MAX);
    let samples = rgba
        .pixels()
        .map(|p| {
            let [r, g, b, a] = p.0;
            if opaque {
                let luma = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    luma.round().clamp(0.0, f64::from(u16::MAX)) as u16
                }
            } else {
                a
            }
        })
        .collect();
    (width, height, samples)
}

/// Runs one detection: `image` in, a coverage at `out`.
///
/// Returns once the file is there. The caller owns `out` — a temporary path
/// it is about to read and drop — so an existing file is overwritten by the
/// detector rather than defended here; nothing in a library is at stake.
pub fn detect(source: &DetectorSource, detection: &str, image: &Path, out: &Path) -> Result<()> {
    if !source.detections.iter().any(|d| d.id == detection) {
        return Err(DetectError::UnknownDetection(detection.to_owned()));
    }
    let command = source.command.display().to_string();
    let mut child = std::process::Command::new(&source.command)
        .args(&source.args)
        .arg("--image")
        .arg(image)
        .arg("--detector")
        .arg(detection)
        .arg("--out")
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|source| DetectError::Spawn {
            command: command.clone(),
            source,
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(source) => {
                return Err(DetectError::Spawn { command, source });
            }
        }
        if started.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DetectError::TimedOut { command });
        }
        std::thread::sleep(POLL);
    }

    let finished = child
        .wait_with_output()
        .map_err(|source| DetectError::Spawn {
            command: command.clone(),
            source,
        })?;
    if !finished.status.success() {
        return Err(DetectError::Failed {
            command,
            status: finished.status.to_string(),
            stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
        });
    }
    // Success is not the detector's word for it: an empty or missing file
    // would otherwise surface much later, as an image decoding error with no
    // hint of where it came from.
    if !out.is_file() || std::fs::metadata(out).map(|m| m.len()).unwrap_or(0) == 0 {
        return Err(DetectError::NoOutput {
            command,
            out: out.to_path_buf(),
        });
    }
    Ok(())
}

/// What a conformance run found (ADR 0105 §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conformance {
    /// Failures: the detector does not speak the protocol. Empty means it
    /// does.
    pub failures: Vec<String>,
    /// Things that are legal and probably not what the author meant — a
    /// uniform coverage above all, which is what a detector returns when
    /// its model did not load.
    pub warnings: Vec<String>,
}

impl Conformance {
    /// Whether the detector speaks the protocol. Warnings do not make it
    /// false: they are legal answers (ADR 0105 §3.5).
    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The image a conformance run feeds a detector: a synthetic scene with a
/// bright top and a darker textured bottom, so a sky detector has something
/// to answer and no photograph of the user's is involved.
fn conformance_image(width: u32, height: u32) -> image::RgbImage {
    image::RgbImage::from_fn(width, height, |x, y| {
        if y < height / 2 {
            // A gradient "sky", light and blue-ish.
            let t = f32::from(u8::try_from(y.min(255)).unwrap_or(255)) / 255.0;
            image::Rgb([
                (150.0 + 60.0 * t) as u8,
                (180.0 + 50.0 * t) as u8,
                (230.0 - 20.0 * t) as u8,
            ])
        } else {
            // A darker, textured "ground".
            let n = ((x * 7 + y * 13) % 40) as u8;
            image::Rgb([60 + n, 70 + n, 50 + n])
        }
    })
}

/// Runs `detection` against a synthetic image and checks the **protocol**,
/// never the quality of the segmentation (ADR 0105 §3).
///
/// The checks, in order: it ran and wrote its file (both enforced by
/// [`detect`] itself, whose error is passed through), the file reads as an
/// image, its dimensions match the input, and its samples span something.
/// Only the last is a warning — a uniform coverage is legal
/// output, and a harness that refuses legal output teaches people to
/// ignore it.
pub fn check_conformance(source: &DetectorSource, detection: &str) -> Conformance {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    let (width, height) = (256u32, 256u32);

    let Ok(dir) = tempfile::tempdir() else {
        failures.push("cannot create a temporary directory to run the check in".to_owned());
        return Conformance { failures, warnings };
    };
    let input = dir.path().join("conformance-in.png");
    if let Err(error) = conformance_image(width, height).save(&input) {
        failures.push(format!("cannot write the test image: {error}"));
        return Conformance { failures, warnings };
    }

    // 1. It ran, wrote its file, and the file reads — all three enforced
    // by `detect_coverage`, whose error is passed through: it names the
    // path it gave the detector, which a second check here could only say
    // worse.
    //
    // 2. The file reads as an image — `detect_coverage` says so.
    let (out_width, out_height, samples) = match detect_coverage(source, detection, &input) {
        Ok(coverage) => coverage,
        Err(error) => {
            failures.push(error.to_string());
            return Conformance { failures, warnings };
        }
    };
    if (out_width, out_height) != (width, height) {
        failures.push(format!(
            "the coverage is {out_width}x{out_height} but the image it was given is \
             {width}x{height}; a coverage is per-pixel and cannot be applied at another size"
        ));
        return Conformance { failures, warnings };
    }
    // 4. It says something. Legal if it does not, and worth saying aloud.
    let (min, max) = samples
        .iter()
        .fold((u16::MAX, 0u16), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    if min == max {
        warnings.push(format!(
            "the coverage is uniformly {min}: legal, but it is also what a detector returns \
             when its model did not load"
        ));
    }
    Conformance { failures, warnings }
}

#[cfg(test)]
mod tests {
    /// A detector under test, running `body`.
    #[cfg(unix)]
    fn fake(dir: &Path, name: &str, body: &str) -> DetectorSource {
        DetectorSource {
            id: name.to_owned(),
            label: name.to_owned(),
            command: script(dir, &format!("{name}.sh"), body),
            args: Vec::new(),
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Sky".to_owned(),
            }],
        }
    }

    /// ADR 0105 §3: the harness passes a detector that copies the image
    /// back — the answer is then the right size and spans a range, which
    /// is all the protocol asks.
    #[cfg(unix)]
    #[test]
    fn conformance_passes_a_detector_that_answers_properly() {
        let dir = tempfile::tempdir().unwrap();
        let report = check_conformance(&fake(dir.path(), "copy", COPY_IMAGE_TO_OUT), "sky");
        assert!(report.passed(), "{report:?}");
        assert!(report.warnings.is_empty(), "{report:?}");
    }

    /// The four failures, each caught and each named.
    #[cfg(unix)]
    #[test]
    fn conformance_names_what_a_detector_got_wrong() {
        let dir = tempfile::tempdir().unwrap();

        // Exits non-zero.
        let report = check_conformance(&fake(dir.path(), "angry", "echo nope >&2; exit 3"), "sky");
        assert!(!report.passed(), "{report:?}");

        // Exits zero and writes nothing: caught by `detect` itself, which
        // names the path it handed over.
        let report = check_conformance(&fake(dir.path(), "silent", "exit 0"), "sky");
        assert!(!report.passed());
        assert!(
            report.failures[0].contains("no coverage"),
            "{:?}",
            report.failures
        );

        // Writes something that is not an image.
        let report = check_conformance(
            &fake(
                dir.path(),
                "garbage",
                r#"out=""
while [ $# -gt 0 ]; do case "$1" in --out) out="$2"; shift 2;; *) shift;; esac; done
printf 'not a png' > "$out""#,
            ),
            "sky",
        );
        assert!(!report.passed());
        assert!(
            report.failures[0].contains("readable image"),
            "{:?}",
            report.failures
        );
    }

    /// ADR 0105 §3.4: a uniform coverage is legal, so it warns rather than
    /// fails — and it warns, because it is what a model that did not load
    /// returns.
    #[cfg(unix)]
    #[test]
    fn a_uniform_coverage_warns_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        // Answers with a black image of the right size: valid, and empty.
        let black = dir.path().join("black.png");
        image::RgbImage::new(256, 256).save(&black).unwrap();
        let body = format!(
            r#"out=""
while [ $# -gt 0 ]; do case "$1" in --out) out="$2"; shift 2;; *) shift;; esac; done
cp {} "$out""#,
            black.display()
        );
        let report = check_conformance(&fake(dir.path(), "empty", &body), "sky");
        assert!(report.passed(), "a uniform coverage is legal: {report:?}");
        assert_eq!(report.warnings.len(), 1, "{report:?}");
        assert!(
            report.warnings[0].contains("did not load"),
            "the warning must say what it usually means: {:?}",
            report.warnings
        );
    }

    /// ADR 0070 §7: a transparent selection is read through its alpha, an
    /// opaque grey image through its luminance. Reading the wrong one is a
    /// silently empty — or silently full — mask.
    #[test]
    fn a_transparent_selection_is_read_through_its_alpha() {
        // Black pixels, half of them transparent: luminance would say "no
        // coverage anywhere", alpha says "the opaque half".
        let mut selection = image::RgbaImage::new(2, 1);
        selection.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        selection.put_pixel(1, 0, image::Rgba([0, 0, 0, 0]));
        let (width, height, samples) =
            coverage_from_image(&image::DynamicImage::ImageRgba8(selection));
        assert_eq!((width, height), (2, 1));
        assert_eq!(samples, vec![u16::MAX, 0]);
    }

    #[test]
    fn an_opaque_grey_mask_is_read_through_its_luminance() {
        let mut painted = image::RgbaImage::new(3, 1);
        painted.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        painted.put_pixel(1, 0, image::Rgba([255, 255, 255, 255]));
        painted.put_pixel(2, 0, image::Rgba([128, 128, 128, 255]));
        let (_, _, samples) = coverage_from_image(&image::DynamicImage::ImageRgba8(painted));
        assert_eq!(samples[0], 0);
        assert_eq!(samples[1], u16::MAX);
        // Mid grey lands mid range, whatever the 8->16 bit expansion does.
        assert!(
            (samples[2] as i32 - (u16::MAX / 2) as i32).abs() < 600,
            "got {}",
            samples[2]
        );
    }

    /// A file with no alpha channel at all still imports, through luminance.
    #[test]
    fn an_image_without_alpha_imports_through_luminance() {
        let mut rgb = image::RgbImage::new(2, 1);
        rgb.put_pixel(0, 0, image::Rgb([255, 255, 255]));
        rgb.put_pixel(1, 0, image::Rgb([0, 0, 0]));
        let (_, _, samples) = coverage_from_image(&image::DynamicImage::ImageRgb8(rgb));
        assert_eq!(samples, vec![u16::MAX, 0]);
    }

    use super::*;

    /// Writes a manifest and, unless `command` names something else, a shell
    /// script standing in for a detector.
    fn manifest(dir: &Path, id: &str, command: &Path, detections: &[&str]) {
        let source = DetectorSource {
            id: id.to_owned(),
            label: id.to_owned(),
            command: command.to_path_buf(),
            args: Vec::new(),
            detections: detections
                .iter()
                .map(|d| Detection {
                    id: (*d).to_owned(),
                    label: (*d).to_owned(),
                })
                .collect(),
        };
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string(&source).unwrap(),
        )
        .unwrap();
    }

    /// A script that copies its `--image` to its `--out`, or fails on demand.
    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The usual argument dance, extracted once: `$1..$6` are the six
    /// arguments this crate always passes, in that order.
    #[cfg(unix)]
    const COPY_IMAGE_TO_OUT: &str = r#"
image=""; out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --image) image="$2"; shift 2;;
    --out) out="$2"; shift 2;;
    --detector) shift 2;;
    *) shift;;
  esac
done
cp "$image" "$out"
"#;

    #[test]
    fn a_directory_that_does_not_exist_offers_no_detector() {
        assert!(discover_in(Path::new("/nowhere/at/all")).is_empty());
    }

    /// Everything unusable is dropped silently, and only that: a launch must
    /// survive a half-installed detector.
    #[test]
    fn unusable_manifests_are_ignored_rather_than_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("detector");
        std::fs::write(&real, "").unwrap();

        std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "{}").unwrap();
        manifest(dir.path(), "no-detections", &real, &[]);
        manifest(
            dir.path(),
            "missing-command",
            Path::new("/no/such/binary"),
            &["sky"],
        );
        manifest(dir.path(), "good", &real, &["sky"]);

        let found = discover_in(dir.path());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].id, "good");
    }

    /// The same directory, asked the other question: what was thrown away,
    /// and why (ADR 0105 §4). The silence is right at a launch and wrong
    /// when an author is the one asking.
    #[test]
    fn what_discovery_declined_can_be_asked_for_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("detector");
        std::fs::write(&real, "").unwrap();

        std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();
        std::fs::write(dir.path().join("ignored.txt"), "{}").unwrap();
        manifest(dir.path(), "no-detections", &real, &[]);
        manifest(
            dir.path(),
            "missing-command",
            Path::new("/no/such/binary"),
            &["sky"],
        );
        manifest(dir.path(), "good", &real, &["sky"]);

        let rejected = rejected_in(dir.path());
        assert_eq!(rejected.len(), 3, "{rejected:?}");

        let reason = |stem: &str| {
            rejected
                .iter()
                .find(|r| r.path.file_name().unwrap() == format!("{stem}.json").as_str())
                .unwrap_or_else(|| panic!("{stem} not among {rejected:?}"))
                .reason
                .clone()
        };
        assert!(reason("broken").contains("not a valid manifest"));
        assert!(reason("no-detections").contains("detections"));
        assert!(reason("missing-command").contains("/no/such/binary"));

        // The one that works is not among them, and the non-JSON file is
        // not a rejected manifest — it was never a manifest.
        assert!(!rejected.iter().any(|r| r.reason.contains("good")));
    }

    /// The field the first detector outside this repository got wrong,
    /// because ADR 0073 §3 named it `detectors` until 2026-08-31. The
    /// point of the test is the *message*: a missing-field complaint
    /// naming `detections` is what turns an hour into a second.
    #[test]
    fn a_manifest_naming_the_list_detectors_is_told_which_field_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("assist.json"),
            r#"{"id":"a","label":"A","command":"a","detectors":[{"id":"sky","label":"Sky"}]}"#,
        )
        .unwrap();

        assert!(discover_in(dir.path()).is_empty());
        let rejected = rejected_in(dir.path());
        assert_eq!(rejected.len(), 1);
        assert!(rejected[0].reason.contains("detections"), "{rejected:?}");
    }

    #[test]
    fn a_directory_that_does_not_exist_declines_nothing() {
        assert!(rejected_in(Path::new("/nowhere/at/all")).is_empty());
    }

    /// A bare command is left to `PATH`, since resolving it here would
    /// second-guess the operating system.
    #[test]
    fn a_bare_command_is_kept_without_being_resolved() {
        let dir = tempfile::tempdir().unwrap();
        manifest(dir.path(), "bare", Path::new("some-detector"), &["sky"]);
        assert_eq!(discover_in(dir.path()).len(), 1);
    }

    /// Discovery is ordered, so a menu does not reshuffle itself between two
    /// launches for no reason.
    #[test]
    fn detectors_come_back_in_a_stable_order() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("detector");
        std::fs::write(&real, "").unwrap();
        for id in ["zebra", "alpha", "middle"] {
            manifest(dir.path(), id, &real, &["sky"]);
        }
        let ids: Vec<String> = discover_in(dir.path()).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["alpha", "middle", "zebra"]);
    }

    #[test]
    fn a_detection_the_manifest_does_not_offer_is_refused() {
        let source = DetectorSource {
            id: "x".to_owned(),
            label: "x".to_owned(),
            command: PathBuf::from("/bin/true"),
            args: Vec::new(),
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Sky".to_owned(),
            }],
        };
        let err = detect(
            &source,
            "subject",
            Path::new("in.png"),
            Path::new("out.png"),
        )
        .unwrap_err();
        assert!(matches!(err, DetectError::UnknownDetection(d) if d == "subject"));
    }

    #[test]
    fn a_command_that_cannot_start_names_itself() {
        let source = DetectorSource {
            id: "x".to_owned(),
            label: "x".to_owned(),
            command: PathBuf::from("/no/such/binary"),
            args: Vec::new(),
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Sky".to_owned(),
            }],
        };
        let err = detect(&source, "sky", Path::new("in.png"), Path::new("out.png")).unwrap_err();
        assert!(err.to_string().contains("/no/such/binary"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_detector_that_writes_its_coverage_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let command = script(dir.path(), "copy.sh", COPY_IMAGE_TO_OUT);
        let source = DetectorSource {
            id: "copy".to_owned(),
            label: "Copy".to_owned(),
            command,
            args: Vec::new(),
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Sky".to_owned(),
            }],
        };
        let image = dir.path().join("in.png");
        std::fs::write(&image, b"not really a png, but bytes").unwrap();
        let out = dir.path().join("out.png");
        detect(&source, "sky", &image, &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&image).unwrap());
    }

    /// The detector's own message reaches the user: it is the only one who
    /// knows why it refused.
    #[cfg(unix)]
    #[test]
    fn a_refusal_carries_the_detectors_own_words() {
        let dir = tempfile::tempdir().unwrap();
        let command = script(
            dir.path(),
            "fail.sh",
            "echo 'no sky in this frame' >&2\nexit 3",
        );
        let source = DetectorSource {
            id: "fail".to_owned(),
            label: "Fail".to_owned(),
            command,
            args: Vec::new(),
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Sky".to_owned(),
            }],
        };
        let err = detect(
            &source,
            "sky",
            &dir.path().join("in.png"),
            &dir.path().join("out.png"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no sky in this frame"), "{err}");
    }

    /// Exit code zero is not proof: a detector that writes nothing is caught
    /// here rather than three layers up, as an image decoding error.
    #[cfg(unix)]
    #[test]
    fn success_without_a_coverage_is_still_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let command = script(dir.path(), "silent.sh", "exit 0");
        let source = DetectorSource {
            id: "silent".to_owned(),
            label: "Silent".to_owned(),
            command,
            args: Vec::new(),
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Sky".to_owned(),
            }],
        };
        let out = dir.path().join("out.png");
        let err = detect(&source, "sky", &dir.path().join("in.png"), &out).unwrap_err();
        assert!(matches!(err, DetectError::NoOutput { .. }), "{err}");
    }

    /// A manifest round-trips through its own format: an installer writes
    /// what this crate reads.
    #[test]
    fn a_manifest_round_trips() {
        let source = DetectorSource {
            id: "leyline-assist".to_owned(),
            label: "Leyline Assist".to_owned(),
            command: PathBuf::from("/opt/leyline-assist/leyline-assist"),
            args: vec!["detect".to_owned()],
            detections: vec![Detection {
                id: "sky".to_owned(),
                label: "Ciel".to_owned(),
            }],
        };
        let text = serde_json::to_string(&source).unwrap();
        assert_eq!(
            serde_json::from_str::<DetectorSource>(&text).unwrap(),
            source
        );
        // `args` is optional in a hand-written manifest.
        let terse = r#"{"id":"a","label":"A","command":"/bin/true",
                        "detections":[{"id":"sky","label":"Sky"}]}"#;
        assert!(
            serde_json::from_str::<DetectorSource>(terse)
                .unwrap()
                .args
                .is_empty()
        );
    }
}
