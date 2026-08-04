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
    /// Whether the manifest describes something that could actually run.
    ///
    /// A command holding a path separator must exist on disk — that is the
    /// usual case, an installer writing an absolute path, and an uninstalled
    /// detector should stop appearing in menus the moment its files are
    /// gone. A bare name is left alone: resolving `PATH` here would
    /// second-guess the operating system.
    fn is_usable(&self) -> bool {
        if self.id.is_empty() || self.detections.is_empty() {
            return false;
        }
        let command = self.command.as_os_str();
        if command.is_empty() {
            return false;
        }
        if self.command.components().count() > 1 {
            return self.command.is_file();
        }
        true
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
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sources: Vec<DetectorSource> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| std::fs::read_to_string(&path).ok())
        .filter_map(|text| serde_json::from_str::<DetectorSource>(&text).ok())
        .filter(DetectorSource::is_usable)
        .collect();
    sources.sort_by(|a, b| a.id.cmp(&b.id));
    sources.dedup_by(|a, b| a.id == b.id);
    sources
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

#[cfg(test)]
mod tests {
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
