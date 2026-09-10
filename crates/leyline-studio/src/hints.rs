//! The debt list of ADR 0133 §4, and the test that counts it.
//!
//! A hint exists where the interface is not self-evident. That makes the set
//! of hints a list of debts rather than a feature, and a list of debts nobody
//! counts grows quietly — so every slider in Develop must be **either hinted
//! or named below**, and a new one fails until somebody classifies it.
//!
//! Test-only: nothing here runs in the application. The list is documentation
//! with teeth, and adding to it is meant to be a visible act in a diff.

/// Labels whose own word says what the slider does (ADR 0133 §1).
///
/// Judged **with the sub-heading above them** since ADR 0140 §2: a label
/// short enough to fit its column often owes half its meaning to the
/// heading — `Amount` under « Netteté » is sharpening's amount, and the
/// list below reads in that company, not in isolation.
///
/// The test to apply before adding one: *could a photographer say what moving
/// this will do, from the label alone, before moving it?* And the second
/// clause — a label is judged **in the company it keeps**, which is why
/// `Whites` is not here despite being ordinary English: `Highlights` is four
/// rows above it and the two are indistinguishable from their labels alone.
const SELF_EVIDENT: &[&str] = &[
    "Exposure",
    "Contrast",
    "Saturation",
    "Hue",
    "Luminance",
    "Strength",
    "Opacity",
    "Rotation",
    "Tone ▸ Exposure",
    "Tone ▸ Contrast",
    "Presence ▸ Saturation",
    "Vignette ▸ Amount",
    "Grain ▸ Amount",
    "Grain ▸ Size",
    // The four crop numbers, labelled by their edge alone under the
    // « Recadrage » sub-heading (ADR 0140 §2). Written with the heading
    // because that is what makes them plain: `Left` on its own would be a
    // question, `Crop ▸ Left` is not.
    "Crop ▸ Left",
    "Crop ▸ Top",
    "Crop ▸ Width",
    "Crop ▸ Height",
    "Sharpening ▸ Amount",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// One `EditSlider` as the source declares it.
    struct Slider {
        line: usize,
        /// The label, prefixed by the sub-heading in scope when there is
        /// one — `Crop ▸ Left` — because that is how a reader meets it
        /// (ADR 0140 §2).
        label: String,
        hinted: bool,
    }

    /// Reads `develop.slint` and returns every slider in it.
    ///
    /// A parser of four lines, because the shape it reads is four lines: the
    /// panel writes `label:` first and `hint:` — when there is one — directly
    /// after it, which the insertion that added them guaranteed and this
    /// keeps true.
    fn sliders() -> Vec<Slider> {
        let source = include_str!("../ui/panels/develop.slint");
        let lines: Vec<&str> = source.lines().collect();
        let mut found = Vec::new();
        // The sub-heading a slider sits under, if any. It survives until the
        // next sub-heading or the next group — the same scope the eye gives
        // it.
        let mut heading: Option<String> = None;
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_end();
            if trimmed.ends_with("SubHeading {") {
                heading = lines
                    .get(index + 1)
                    .and_then(|l| translated(l, "label"))
                    .or(heading.take());
                continue;
            }
            if trimmed.ends_with("GroupHeader {") {
                heading = None;
                continue;
            }
            // A conditional block is a new context: the mask editor's
            // « Luminance range » and « Color range » open one each, and a
            // heading that leaked into them would name the wrong section.
            if trimmed.trim_start().starts_with("if ") && trimmed.ends_with('{') {
                heading = None;
                continue;
            }
            if !trimmed.ends_with("EditSlider {") {
                continue;
            }
            // The label is the first `label:` after the opening brace, and
            // the hint — if any — the `hint:` beside it. Four lines is
            // enough: nothing in this panel puts anything between them.
            let window = &lines[index + 1..(index + 5).min(lines.len())];
            let Some(label) = window.iter().find_map(|l| translated(l, "label")) else {
                panic!("line {}: an EditSlider with no label", index + 1);
            };
            found.push(Slider {
                line: index + 1,
                label: match &heading {
                    Some(heading) => format!("{heading} \u{25b8} {label}"),
                    None => label,
                },
                hinted: window.iter().any(|l| translated(l, "hint").is_some()),
            });
        }
        found
    }

    /// The text of a `<property>: @tr("…");` line, or `None`.
    fn translated(line: &str, property: &str) -> Option<String> {
        let rest = line
            .trim()
            .strip_prefix(property)?
            .strip_prefix(": @tr(\"")?;
        Some(rest.split('"').next()?.to_owned())
    }

    /// ADR 0133 §4 — neither answer is the default.
    #[test]
    fn every_develop_slider_is_either_hinted_or_declared_self_evident() {
        let sliders = sliders();
        assert!(
            sliders.len() > 50,
            "only {} sliders found — the parser has stopped seeing the panel",
            sliders.len()
        );
        let unclassified: Vec<String> = sliders
            .iter()
            .filter(|slider| !slider.hinted && !SELF_EVIDENT.contains(&slider.label.as_str()))
            .map(|slider| format!("{} (develop.slint:{})", slider.label, slider.line))
            .collect();
        assert!(
            unclassified.is_empty(),
            "these sliders have neither a hint nor a place in SELF_EVIDENT — say which \
             they are (ADR 0133 §4):\n  {}",
            unclassified.join("\n  ")
        );
    }

    /// The other direction: a label declared self-evident must not also carry
    /// a hint, or the list stops describing anything.
    #[test]
    fn nothing_is_both_self_evident_and_hinted() {
        let both: Vec<String> = sliders()
            .iter()
            .filter(|slider| slider.hinted && SELF_EVIDENT.contains(&slider.label.as_str()))
            .map(|slider| format!("{} (develop.slint:{})", slider.label, slider.line))
            .collect();
        assert!(
            both.is_empty(),
            "hinted and listed as self-evident at once — drop one:\n  {}",
            both.join("\n  ")
        );
    }

    /// ADR 0133 §2: one sentence, not documentation. A hint that needs two is
    /// a control that needs redesigning, and this is where that shows up.
    #[test]
    fn a_hint_stays_one_sentence() {
        let source = include_str!("../ui/panels/develop.slint");
        for (index, line) in source.lines().enumerate() {
            let Some(hint) = translated(line, "hint") else {
                continue;
            };
            assert!(
                hint.chars().count() <= 110,
                "develop.slint:{}: {} characters — ADR 0133 §2 caps a hint at 110:\n  {hint}",
                index + 1,
                hint.chars().count()
            );
            assert!(
                !hint.ends_with('.') || hint.matches(". ").count() <= 1,
                "develop.slint:{}: more than two sentences:\n  {hint}",
                index + 1
            );
        }
    }
}
