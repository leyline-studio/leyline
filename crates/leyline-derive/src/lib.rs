//! External pixel processors: the contract, the discovery and the invocation
//! ([ADR 0107](../../../docs/adr/0107-derived-assets-and-the-pixel-socket.md)).
//!
//! **This crate processes nothing.** It knows how to find the executables
//! that do, how to call one, and what shape the images they exchange take:
//!
//! ```text
//! <command> <args…> --image <in.tif> --operation <id> --out <out.tif>
//! ```
//!
//! Both files are 16-bit RGB TIFFs holding **linear Rec. 2020 with white at
//! 1.0** — the develop buffer as it stands before rank 20, which is the one
//! place in the pipeline where it has a single meaning whatever the revision
//! says ([ADR 0107](../../../docs/adr/0107-derived-assets-and-the-pixel-socket.md)
//! §4). The output must have the **same dimensions** as the input: what
//! comes back inherits the parent's development, and a development cannot
//! be moved to another geometry.
//!
//! What the caller does with the answer is the other half, and it is
//! deliberately not here: the engine imports it as a **new asset**, never a
//! stage ([ADR 0102](../../../docs/adr/0102-paid-extensions-and-the-pixel-boundary.md)),
//! so a library that has never seen a processor still opens, renders and
//! exports what one produced.
//!
//! That smallness is the point. A processor is a *separate process*
//! exchanging two files, so it links nothing of Leyline, can be written in
//! twenty lines of any language, and cannot take the application down with
//! it.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How long one operation may take before it is killed (ADR 0107 §2).
///
/// Five times the detector's budget, because the work is not the same: a
/// segmentation runs on a 1024 px preview, a denoise runs on all thirty
/// million pixels of the original. Generous on purpose: this is a safety
/// net against a wedged process, not a performance target.
const TIMEOUT: Duration = Duration::from_secs(600);

/// How often the wait loop looks at the child.
const POLL: Duration = Duration::from_millis(50);

/// What can go wrong between "the user asked" and "there is a processed
/// image on disk".
#[derive(Debug, thiserror::Error)]
pub enum DeriveError {
    /// The manifest does not declare this operation.
    #[error("this processor offers no `{0}` operation")]
    UnknownOperation(String),
    /// The executable could not be started, or the pipes could not be read.
    #[error("cannot run {command}: {source}")]
    Spawn {
        /// The command as the manifest spelled it.
        command: String,
        /// What the operating system said.
        source: std::io::Error,
    },
    /// It ran and refused. `stderr` is passed through verbatim: the
    /// processor is the only one who knows why, and swallowing its message
    /// would leave the user with nothing.
    #[error("{command} failed ({status}){}", detail(.stderr))]
    Failed {
        /// The command as the manifest spelled it.
        command: String,
        /// Exit status, or a description of the signal that ended it.
        status: String,
        /// Whatever the processor wrote on its error stream, trimmed.
        stderr: String,
    },
    /// It ran past [`TIMEOUT`] and was killed.
    #[error("{command} did not finish within {} s", TIMEOUT.as_secs())]
    TimedOut {
        /// The command as the manifest spelled it.
        command: String,
    },
    /// It reported success and wrote nothing usable.
    #[error("{command} reported success but wrote no image to {}", .out.display())]
    NoOutput {
        /// The command as the manifest spelled it.
        command: String,
        /// Where the image was expected.
        out: PathBuf,
    },
    /// It wrote a file that is not a readable image.
    #[error("{command} wrote an image that cannot be read: {reason}")]
    Unreadable {
        /// The command as the manifest spelled it.
        command: String,
        /// What the image decoder said.
        reason: String,
    },
    /// It wrote an image of another size.
    #[error(
        "{command} returned {got_width}x{got_height} for a {want_width}x{want_height} image; \
         a derived asset inherits its parent's development, which cannot move to another geometry"
    )]
    WrongSize {
        /// The command as the manifest spelled it.
        command: String,
        /// Width it returned.
        got_width: u32,
        /// Height it returned.
        got_height: u32,
        /// Width it was given.
        want_width: u32,
        /// Height it was given.
        want_height: u32,
    },
    /// An exchange file could not be written or read.
    #[error("cannot {action} the exchange image {}: {reason}", .path.display())]
    Exchange {
        /// `write` or `read`.
        action: &'static str,
        /// The file involved.
        path: PathBuf,
        /// What the encoder or decoder said.
        reason: String,
    },
}

/// Appends a processor's own error message, when it left one.
fn detail(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

type Result<T> = std::result::Result<T, DeriveError>;

/// One operation a processor offers — "denoise", "upscale".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    /// Passed back to the executable as `--operation`.
    pub id: String,
    /// What the menu shows. The processor's own words: it is the only one
    /// who knows what its model does, and translating them here would mean
    /// maintaining a list of operations nobody has installed.
    pub label: String,
}

/// One installed processor, as its manifest describes it (ADR 0107 §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessorSource {
    /// Stable identifier, and the manifest's file name.
    pub id: String,
    /// Name shown next to its operations.
    pub label: String,
    /// The executable. Absolute, or a bare name to be found on `PATH`.
    pub command: PathBuf,
    /// Fixed arguments placed before the ones this crate adds.
    #[serde(default)]
    pub args: Vec<String>,
    /// What it can do. A source offering nothing is not a source.
    pub operations: Vec<Operation>,
}

impl ProcessorSource {
    /// Why the manifest cannot be offered, or `None` when it can.
    ///
    /// Each condition says its own name: a manifest is set aside silently at
    /// a launch, but an author asking why deserves the reason — the defect
    /// ADR 0105 §4 paid for on the detector socket, fixed here before it
    /// could happen twice.
    fn unusable_reason(&self) -> Option<String> {
        if self.id.is_empty() {
            return Some("\"id\" is empty".to_owned());
        }
        if self.operations.is_empty() {
            return Some(
                "\"operations\" is empty — a source offering none is not a source".to_owned(),
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

/// Where manifests live: `<user config>/Leyline/processors`.
///
/// Per user and per machine, never inside a library: a library is portable
/// and self-contained (`docs/catalog.md` §37), and what is installed on
/// *this* computer has no business travelling with someone's photos. The
/// folder sits beside `detectors/`, for the same reason and by the same
/// rule.
#[must_use]
pub fn manifests_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "Leyline")?;
    Some(dirs.config_dir().join("processors"))
}

/// Every usable processor installed for this user, sorted by identifier.
///
/// Nothing here fails: no directory, no manifests, an unreadable file or a
/// malformed one all mean the same thing to a caller — nothing to offer. A
/// processor is an accessory, and an accessory does not get to break a
/// launch.
#[must_use]
pub fn discover() -> Vec<ProcessorSource> {
    manifests_dir()
        .map(|dir| discover_in(&dir))
        .unwrap_or_default()
}

/// [`discover`] over an explicit directory — what the tests use, and what an
/// author trying an executable before installing it uses.
#[must_use]
pub fn discover_in(dir: &Path) -> Vec<ProcessorSource> {
    let mut sources: Vec<ProcessorSource> = manifests_in(dir)
        .iter()
        .filter_map(|path| read_manifest(path).ok())
        .collect();
    sources.sort_by(|a, b| a.id.cmp(&b.id));
    sources.dedup_by(|a, b| a.id == b.id);
    sources
}

/// A manifest that was read and set aside, and why.
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
/// while this runs when somebody asks.
#[must_use]
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
fn read_manifest(path: &Path) -> std::result::Result<ProcessorSource, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot be read: {e}"))?;
    let source: ProcessorSource =
        serde_json::from_str(&text).map_err(|e| format!("is not a valid manifest: {e}"))?;
    match source.unusable_reason() {
        Some(reason) => Err(reason),
        None => Ok(source),
    }
}

/// An image on its way to or from a processor: linear Rec. 2020, white at
/// 1.0, interleaved RGB (ADR 0107 §3).
///
/// `samples` holds `width * height * 3` values. They are `f32` here and
/// 16-bit unsigned on disk, which is the trade [`write_exchange`] documents:
/// a value above white does not survive the round trip.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` linear samples, nominally in `[0, 1]`.
    pub samples: Vec<f32>,
}

/// Writes the image a processor is about to be given (ADR 0107 §3).
///
/// **Where the headroom goes.** The buffer is unbounded above 1.0 and a
/// 16-bit integer is not, so a value above white is clipped here. It is the
/// price of a container every image library on earth reads, it is stated in
/// the ADR rather than discovered, and it is a real reason to prefer the
/// original file for a photograph whose highlights matter more than its
/// noise.
pub fn write_exchange(path: &Path, image: &Exchange) -> Result<()> {
    let mut buffer =
        image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::new(image.width, image.height);
    for (pixel, chunk) in buffer.pixels_mut().zip(image.samples.chunks_exact(3)) {
        *pixel = image::Rgb([
            to_sample(chunk[0]),
            to_sample(chunk[1]),
            to_sample(chunk[2]),
        ]);
    }
    buffer.save(path).map_err(|error| DeriveError::Exchange {
        action: "write",
        path: path.to_path_buf(),
        reason: error.to_string(),
    })
}

/// Reads an image a processor produced, back into linear samples.
///
/// An answer that is not sixteen bits is **refused**, not promoted. The
/// `image` crate would widen an 8-bit file without a word, and the result
/// would look right and hold 255 of every 256 levels less than it should —
/// a silent quality loss in the one place this socket exists to avoid one.
pub fn read_exchange(path: &Path) -> Result<Exchange> {
    let decoded = image::open(path).map_err(|error| DeriveError::Exchange {
        action: "read",
        path: path.to_path_buf(),
        reason: error.to_string(),
    })?;
    if decoded.color().bits_per_pixel() / u16::from(decoded.color().channel_count()) < 16 {
        return Err(DeriveError::Exchange {
            action: "read",
            path: path.to_path_buf(),
            reason: format!(
                "it holds {:?}; the exchange is 16 bits per channel, and reading fewer as more \
                 would hide the levels that were thrown away",
                decoded.color()
            ),
        });
    }
    let rgb = decoded.into_rgb16();
    let (width, height) = (rgb.width(), rgb.height());
    let samples = rgb
        .into_raw()
        .into_iter()
        .map(|v| f32::from(v) / f32::from(u16::MAX))
        .collect();
    Ok(Exchange {
        width,
        height,
        samples,
    })
}

/// One linear sample, quantized to the exchange's sixteen bits.
fn to_sample(value: f32) -> u16 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (value.clamp(0.0, 1.0) * f32::from(u16::MAX)).round() as u16
    }
}

/// Runs one operation over `image` and returns what came back.
///
/// The whole gesture in one call: the input is written here, the processor
/// fills a temporary output, the answer is read and checked, and both files
/// are dropped. Callers get pixels and never handle the protocol's plumbing
/// — which is what keeps two clients from disagreeing about it.
///
/// The size check is here rather than in a client for the same reason: it is
/// the one rule of this protocol that decides whether the answer can be used
/// at all.
pub fn process(source: &ProcessorSource, operation: &str, image: &Exchange) -> Result<Exchange> {
    let command = source.command.display().to_string();
    let dir = tempfile::Builder::new()
        .prefix("leyline-derive-")
        .tempdir()
        .map_err(|source| DeriveError::Spawn {
            command: command.clone(),
            source,
        })?;
    let input = dir.path().join("in.tif");
    let output = dir.path().join("out.tif");
    write_exchange(&input, image)?;
    run(source, operation, &input, &output)?;
    let answer = read_exchange(&output)?;
    if (answer.width, answer.height) != (image.width, image.height) {
        return Err(DeriveError::WrongSize {
            command,
            got_width: answer.width,
            got_height: answer.height,
            want_width: image.width,
            want_height: image.height,
        });
    }
    Ok(answer)
}

/// Runs one operation: `image` in, a processed image at `out`.
///
/// Returns once the file is there. The caller owns `out` — a temporary path
/// it is about to read and drop — so an existing file is overwritten by the
/// processor rather than defended here; nothing in a library is at stake.
pub fn run(source: &ProcessorSource, operation: &str, image: &Path, out: &Path) -> Result<()> {
    if !source.operations.iter().any(|o| o.id == operation) {
        return Err(DeriveError::UnknownOperation(operation.to_owned()));
    }
    let command = source.command.display().to_string();
    let mut child = std::process::Command::new(&source.command)
        .args(&source.args)
        .arg("--image")
        .arg(image)
        .arg("--operation")
        .arg(operation)
        .arg("--out")
        .arg(out)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|source| DeriveError::Spawn {
            command: command.clone(),
            source,
        })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(source) => {
                return Err(DeriveError::Spawn { command, source });
            }
        }
        if started.elapsed() >= TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DeriveError::TimedOut { command });
        }
        std::thread::sleep(POLL);
    }

    let finished = child
        .wait_with_output()
        .map_err(|source| DeriveError::Spawn {
            command: command.clone(),
            source,
        })?;
    if !finished.status.success() {
        return Err(DeriveError::Failed {
            command,
            status: finished.status.to_string(),
            stderr: String::from_utf8_lossy(&finished.stderr).trim().to_owned(),
        });
    }
    // Success is not the processor's word for it: an empty or missing file
    // would otherwise surface much later, as an image decoding error with no
    // hint of where it came from.
    if !out.is_file() || std::fs::metadata(out).map(|m| m.len()).unwrap_or(0) == 0 {
        return Err(DeriveError::NoOutput {
            command,
            out: out.to_path_buf(),
        });
    }
    Ok(())
}

/// What a conformance run found (ADR 0107 §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conformance {
    /// Failures: the processor does not speak the protocol. Empty means it
    /// does.
    pub failures: Vec<String>,
    /// Things that are legal and probably not what the author meant — an
    /// output identical to the input above all, which is what a processor
    /// returns when its model did not load.
    pub warnings: Vec<String>,
}

impl Conformance {
    /// Whether the processor speaks the protocol. Warnings do not make it
    /// false: they are legal answers.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The image a conformance run feeds a processor: a synthetic scene with a
/// smooth gradient and a noisy patch, so a denoiser has something to answer
/// and no photograph of the user's is involved.
fn conformance_image(width: u32, height: u32) -> Exchange {
    let mut samples = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let base = f32::from(u16::try_from(x.min(65_535)).unwrap_or(u16::MAX))
                / f32::from(u16::try_from(width.max(1) - 1).unwrap_or(1).max(1));
            // A deterministic speckle on the lower half: something to remove,
            // and the same speckle on every run.
            let noise = if y > height / 2 {
                (((x * 7 + y * 13) % 32) as f32) / 512.0
            } else {
                0.0
            };
            samples.push((base + noise).clamp(0.0, 1.0));
            samples.push((base * 0.8 + noise).clamp(0.0, 1.0));
            samples.push((base * 0.6 + noise).clamp(0.0, 1.0));
        }
    }
    Exchange {
        width,
        height,
        samples,
    }
}

/// Runs `operation` against a synthetic image and checks the **protocol**,
/// never the quality of the processing (ADR 0107 §8).
///
/// The checks, in order: it ran and wrote its file, the file reads as a
/// 16-bit image, its dimensions match the input — all three enforced by
/// [`process`], whose error is passed through — and it changed something.
/// Only the last is a warning: returning the image untouched is legal
/// output, and a harness that refuses legal output teaches people to ignore
/// it. It is also exactly what a processor does when its model failed to
/// load, which is why it is said aloud.
#[must_use]
pub fn check_conformance(source: &ProcessorSource, operation: &str) -> Conformance {
    let mut failures = Vec::new();
    let mut warnings = Vec::new();
    let given = conformance_image(256, 256);

    let answer = match process(source, operation, &given) {
        Ok(answer) => answer,
        Err(error) => {
            failures.push(error.to_string());
            return Conformance { failures, warnings };
        }
    };
    if answer.samples.len() != given.samples.len() {
        failures.push(format!(
            "the answer holds {} samples for a {}-sample image; it is not interleaved RGB",
            answer.samples.len(),
            given.samples.len()
        ));
        return Conformance { failures, warnings };
    }
    // The input has already been through the same quantization on its way
    // out, so comparing the answer to a re-read of what was written is the
    // only comparison that means anything.
    let written = given.samples.iter().map(|&v| to_sample(v));
    let returned = answer.samples.iter().map(|&v| to_sample(v));
    if written.eq(returned) {
        warnings.push(
            "the answer is identical to the image it was given: legal, but it is also what a \
             processor returns when its model did not load"
                .to_owned(),
        );
    }
    Conformance { failures, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A processor under test, running `body`.
    #[cfg(unix)]
    fn fake(dir: &Path, name: &str, body: &str) -> ProcessorSource {
        ProcessorSource {
            id: name.to_owned(),
            label: name.to_owned(),
            command: script(dir, &format!("{name}.sh"), body),
            args: Vec::new(),
            operations: vec![Operation {
                id: "denoise".to_owned(),
                label: "Denoise".to_owned(),
            }],
        }
    }

    /// A shell script standing in for a processor.
    #[cfg(unix)]
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Copies `--image` to `--out`: the identity processor, and the shape
    /// every other test script starts from.
    #[cfg(unix)]
    const COPY: &str = r#"image=""; out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --image) image="$2"; shift 2;;
    --out) out="$2"; shift 2;;
    *) shift;;
  esac
done
cp "$image" "$out""#;

    /// The exchange survives a write and a read: 16 bits in, 16 bits out,
    /// and the values land where they were put. Without this the whole
    /// socket exchanges noise.
    #[test]
    fn the_exchange_round_trips_through_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exchange.tif");
        let given = Exchange {
            width: 2,
            height: 1,
            samples: vec![0.0, 0.5, 1.0, 0.25, 0.75, 0.125],
        };
        write_exchange(&path, &given).unwrap();
        let back = read_exchange(&path).unwrap();
        assert_eq!((back.width, back.height), (2, 1));
        for (want, got) in given.samples.iter().zip(&back.samples) {
            assert!(
                (want - got).abs() < 1.0 / 65_535.0,
                "want {want}, got {got}"
            );
        }
    }

    /// ADR 0107 §3 states the price of sixteen bits rather than hiding it,
    /// so the clipping is a test and not a surprise.
    #[test]
    fn a_value_above_white_is_clipped_by_the_exchange() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clipped.tif");
        write_exchange(
            &path,
            &Exchange {
                width: 1,
                height: 1,
                samples: vec![4.0, -1.0, 1.0],
            },
        )
        .unwrap();
        let back = read_exchange(&path).unwrap();
        assert_eq!(back.samples, vec![1.0, 0.0, 1.0]);
    }

    /// An 8-bit answer is refused rather than widened: the promotion is
    /// exactly the kind of silent loss this socket exists to avoid.
    #[test]
    fn an_eight_bit_answer_is_refused_rather_than_promoted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("eight.tif");
        image::RgbImage::new(4, 4).save(&path).unwrap();
        let error = read_exchange(&path).unwrap_err();
        assert!(error.to_string().contains("16 bits"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn an_unknown_operation_is_refused_before_anything_runs() {
        let dir = tempfile::tempdir().unwrap();
        let source = fake(dir.path(), "copy", COPY);
        let error = process(&source, "upscale", &conformance_image(8, 8)).unwrap_err();
        assert!(matches!(error, DeriveError::UnknownOperation(op) if op == "upscale"));
    }

    /// The one rule that decides whether an answer can be used at all
    /// (ADR 0107 §2): a derived asset inherits a development, and a
    /// development cannot move to another geometry.
    #[cfg(unix)]
    #[test]
    fn an_answer_of_another_size_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let small = dir.path().join("small.tif");
        write_exchange(
            &small,
            &Exchange {
                width: 4,
                height: 4,
                samples: vec![0.5; 4 * 4 * 3],
            },
        )
        .unwrap();
        let body = format!(
            r#"out=""
while [ $# -gt 0 ]; do case "$1" in --out) out="$2"; shift 2;; *) shift;; esac; done
cp {} "$out""#,
            small.display()
        );
        let source = fake(dir.path(), "shrink", &body);
        let error = process(&source, "denoise", &conformance_image(256, 256)).unwrap_err();
        assert!(
            matches!(error, DeriveError::WrongSize { got_width: 4, .. }),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn conformance_passes_a_processor_that_answers_properly() {
        let dir = tempfile::tempdir().unwrap();
        let report = check_conformance(&fake(dir.path(), "copy", COPY), "denoise");
        assert!(report.passed(), "{report:?}");
        // The identity processor is exactly the case §8 warns about.
        assert_eq!(report.warnings.len(), 1, "{report:?}");
        assert!(
            report.warnings[0].contains("did not load"),
            "the warning must say what it usually means: {:?}",
            report.warnings
        );
    }

    #[cfg(unix)]
    #[test]
    fn conformance_names_what_a_processor_got_wrong() {
        let dir = tempfile::tempdir().unwrap();

        // Exits non-zero, and its own message is passed through.
        let report = check_conformance(
            &fake(dir.path(), "angry", "echo nope >&2; exit 3"),
            "denoise",
        );
        assert!(!report.passed(), "{report:?}");
        assert!(report.failures[0].contains("nope"), "{:?}", report.failures);

        // Exits zero and writes nothing.
        let report = check_conformance(&fake(dir.path(), "silent", "exit 0"), "denoise");
        assert!(!report.passed());
        assert!(
            report.failures[0].contains("no image"),
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
printf 'not a tiff' > "$out""#,
            ),
            "denoise",
        );
        assert!(!report.passed());
        assert!(
            report.failures[0].contains("cannot read"),
            "{:?}",
            report.failures
        );
    }

    /// A manifest is an accessory: unreadable, incomplete or pointing at
    /// nothing means "no menu entry", never a broken launch. And the reason
    /// is available to whoever asks — the defect ADR 0105 §4 paid for.
    #[test]
    fn a_rejected_manifest_names_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.json"), "{ not json").unwrap();
        std::fs::write(
            dir.path().join("empty.json"),
            r#"{"id":"e","label":"E","command":"/bin/sh","operations":[]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("good.json"),
            r#"{"id":"g","label":"G","command":"/bin/sh",
                "operations":[{"id":"denoise","label":"Denoise"}]}"#,
        )
        .unwrap();

        let found = discover_in(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "g");

        let rejected = rejected_in(dir.path());
        assert_eq!(rejected.len(), 2, "{rejected:?}");
        assert!(rejected.iter().any(|r| r.reason.contains("valid manifest")));
        assert!(rejected.iter().any(|r| r.reason.contains("operations")));
    }

    /// The field is named `operations`, and the code reads that name. ADR
    /// 0105 §4 exists because the detector manifest's list was documented
    /// under one name and read under another for four weeks.
    #[test]
    fn the_manifest_field_is_the_one_the_adr_documents() {
        let manifest = r#"{
            "id": "leyline-assist",
            "label": "Leyline Assist",
            "command": "/bin/sh",
            "args": ["derive"],
            "operations": [{ "id": "denoise", "label": "AI denoise" }]
        }"#;
        let source: ProcessorSource = serde_json::from_str(manifest).unwrap();
        assert_eq!(source.operations.len(), 1);
        assert_eq!(source.operations[0].id, "denoise");
        assert_eq!(source.args, vec!["derive".to_owned()]);
        assert_eq!(source.unusable_reason(), None);
    }
}
