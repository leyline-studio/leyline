//! The user guide, and the one way it reaches a browser (ADR 0151).
//!
//! The pages are compiled into the binary: three deliverables are packaged
//! three different ways, and a resource each of them must remember to list is
//! a resource one of them will be missing, silently (ADR 0151 §2).

use std::path::{Path, PathBuf};

/// Every guide that ships, keyed by the language tag `Tr::guide_language`
/// returns for it. English first: it is the fallback, and the list is read
/// in order.
///
/// Adding a language costs a line here beside its `.po` and its entry in
/// [`crate::preferences::TRANSLATED_LANGUAGES`]; a tag with no page of its
/// own simply reads the guide in English, which is a worse read and never a
/// refusal.
const GUIDES: &[(&str, &str)] = &[
    ("en", include_str!("../../../docs/guide/en.html")),
    ("fr", include_str!("../../../docs/guide/fr.html")),
];

/// The page to show for a language tag, and the tag it was actually found
/// under — which is what names the file, so a reader who falls back to
/// English can see that they did.
pub(crate) fn page(tag: &str) -> (&'static str, &'static str) {
    GUIDES
        .iter()
        .find(|(known, _)| *known == tag)
        .copied()
        .unwrap_or(GUIDES[0])
}

/// Where the extracted page goes: the cache directory, because it is a copy
/// of something the binary already holds and deleting it costs nothing —
/// the next Help ▸ User Guide writes it again.
fn cache_dir() -> PathBuf {
    directories::ProjectDirs::from("", "", "Leyline")
        .map_or_else(std::env::temp_dir, |dirs| dirs.cache_dir().to_path_buf())
}

/// Writes the guide for `tag` into `dir` and returns its path.
///
/// Rewritten on every call rather than written once: the page must be the
/// one this binary holds, and a file left by an older install looks exactly
/// like a current one (ADR 0151 §2).
pub(crate) fn write_guide(dir: &Path, tag: &str) -> Result<PathBuf, String> {
    let (tag, html) = page(tag);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("leyline-guide-{tag}.html"));
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Writes the guide and hands it to the platform opener.
///
/// Returns the path on success so the caller can say where it landed: a
/// browser that will not start costs a copy-paste, exactly as the report
/// folder does (ADR 0123 §2).
pub(crate) fn show(tag: &str) -> Result<PathBuf, String> {
    let path = write_guide(&cache_dir(), tag)?;
    open_externally(&path);
    Ok(path)
}

/// Opens a path with whatever the desktop uses for it.
///
/// Best-effort and deliberately ignored: every caller shows the path too, so
/// the failure mode is a copy-paste and not a dead end.
pub(crate) fn open_externally(path: &Path) {
    #[cfg(target_os = "windows")]
    let command = "explorer";
    #[cfg(target_os = "macos")]
    let command = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let command = "xdg-open";
    let _ = std::process::Command::new(command).arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_language_has_a_guide() {
        // The `.po` list and the guide list are two places that can drift.
        // English has no `.po` of its own and is the first entry above.
        for (tag, _) in crate::preferences::TRANSLATED_LANGUAGES {
            let (found, _) = page(tag);
            assert_eq!(
                found, *tag,
                "language {tag} ships a translation but no docs/guide/{tag}.html"
            );
        }
    }

    #[test]
    fn an_unknown_tag_reads_english() {
        let (tag, html) = page("de");
        assert_eq!(tag, "en");
        assert!(html.contains("<html lang=\"en\">"));
    }

    #[test]
    fn each_guide_is_a_whole_page() {
        // Cheap, and it is the one thing include_str! cannot promise: a
        // truncated or renamed file still compiles.
        for (tag, html) in GUIDES {
            assert!(html.starts_with("<!DOCTYPE html>"), "{tag} lost its head");
            assert!(html.trim_end().ends_with("</html>"), "{tag} lost its tail");
        }
    }

    #[test]
    fn the_written_file_is_named_after_the_language_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = write_guide(dir.path(), "fr").expect("write");
        assert!(path.ends_with("leyline-guide-fr.html"));

        // A tag with no page writes the English one, under the English name:
        // the file says which guide it is.
        let path = write_guide(dir.path(), "de").expect("write");
        assert!(path.ends_with("leyline-guide-en.html"));
        let written = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(written, page("en").1);
    }
}
