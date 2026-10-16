//! Every colour of the interface is spelled in `ui/theme.slint` and nowhere
//! else (ADR 0158 §1).
//!
//! Before that file existed, `#9a9a9a` was written 117 times and nothing
//! said whether a given one meant "secondary text" or "disabled border" —
//! which is why a second theme (ADR 0159 §6) needs this guard: one literal
//! left behind is a spot that stays dark in the light theme.

use std::fs;
use std::path::{Path, PathBuf};

/// The one file allowed to contain colour literals.
const PALETTE: &str = "theme.slint";

#[test]
fn no_colour_literal_outside_the_palette() {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut offenders = Vec::new();
    for file in slint_files(&ui) {
        if file.file_name().is_some_and(|name| name == PALETTE) {
            continue;
        }
        let source = fs::read_to_string(&file).expect("read a .slint file");
        for (number, line) in source.lines().enumerate() {
            if let Some(literal) = colour_literal(line) {
                offenders.push(format!(
                    "{}:{}: {literal}",
                    file.strip_prefix(&ui).unwrap_or(&file).display(),
                    number + 1
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "colour literals outside ui/{PALETTE} — give each a role in Theme, or in Hues \
         if it is drawn on the photograph:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn the_guard_sees_a_literal_and_ignores_comments_and_strings() {
    assert_eq!(colour_literal("    color: #9a9a9a;"), Some("#9a9a9a"));
    assert_eq!(
        colour_literal("    background: a ? #ffffff10 : b;"),
        Some("#ffffff10")
    );
    assert_eq!(colour_literal("    // was #9a9a9a"), None);
    assert_eq!(colour_literal(r##"    text: "#FFFFFF";"##), None);
    assert_eq!(colour_literal("    color: Theme.text;"), None);
}

fn slint_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).expect("read ui directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            found.extend(slint_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "slint") {
            found.push(path);
        }
    }
    found
}

/// The first `#rgb`…`#rrggbbaa` literal in the code part of a line, outside
/// string literals (a watermark's default colour is typed text, not a colour
/// of the interface).
fn colour_literal(line: &str) -> Option<&str> {
    let code = line.split("//").next().unwrap_or("");
    let mut in_string = false;
    for (start, character) in code.char_indices() {
        match character {
            '"' => in_string = !in_string,
            '#' if !in_string => {
                let digits = code[start + 1..]
                    .chars()
                    .take_while(char::is_ascii_hexdigit)
                    .count();
                if matches!(digits, 3 | 4 | 6 | 8) {
                    return Some(&code[start..=start + digits]);
                }
            }
            _ => {}
        }
    }
    None
}
