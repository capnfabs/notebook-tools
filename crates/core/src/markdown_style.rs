//! Turns Claude's markup into the user's markdown. The prompt asks for HTML tags, which can't be
//! mistaken for what's written on the page; they're converted here, on every parse, so cached
//! transcripts follow the current [`MarkdownStyle`] too. See
//! `decisions/2026-10-30-markdown-from-html-tags.md`.

use crate::transcript::{Line, Uncertain};

/// How Claude's markup is written in the transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownStyle {
    /// Wraps underlined text, e.g. `_` for `_word_`.
    pub underline: String,
    /// Heading level of a line that's underlined as a whole.
    pub underlined_line_heading: usize,
}

impl Default for MarkdownStyle {
    fn default() -> Self {
        Self {
            underline: "_".to_owned(),
            underlined_line_heading: 2,
        }
    }
}

/// Converts `lines` and `uncertain` words to `style`, and drops alternatives that repeat the word
/// they're for: an uncertain word with none left still needs a look.
pub fn tidy(lines: &mut [Line], uncertain: &mut [Uncertain], style: &MarkdownStyle) {
    for line in lines {
        line.text_md = tidy_line(&line.text_md, style);
    }
    for u in uncertain {
        u.text = tidy_word(&u.text, style);
        for alternative in &mut u.alternatives {
            *alternative = tidy_word(alternative, style);
        }
        let text = &u.text;
        u.alternatives.retain(|a| a != text);
    }
}

/// One line of Claude's markdown in `style`. A line of dashes becomes `---`, a wholly underlined
/// line a heading, and other underlines `style.underline`.
pub fn tidy_line(text_md: &str, style: &MarkdownStyle) -> String {
    let trimmed = text_md.trim();
    if is_rule(trimmed) {
        return "---".to_owned();
    }
    if let Some(inner) = whole_underline(trimmed) {
        return format!(
            "{} {}",
            "#".repeat(style.underlined_line_heading),
            inner.trim()
        );
    }
    underlines(text_md, &style.underline)
}

/// An uncertain word in `style`. One that's underlined as a whole loses its tags, so it's found
/// both in a heading and between underline marks.
fn tidy_word(text: &str, style: &MarkdownStyle) -> String {
    match whole_underline(text) {
        Some(inner) => inner.trim().to_owned(),
        None => underlines(text, &style.underline),
    }
}

/// Only dashes (hyphens, en or em dashes) and spaces.
fn is_rule(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| matches!(c, '-' | '–' | '—') || c == ' ')
}

/// What's inside `text` if it's one `<u>` span.
fn whole_underline(text: &str) -> Option<&str> {
    let inner = text.strip_prefix("<u>")?.strip_suffix("</u>")?;
    (!inner.trim().is_empty() && !inner.contains("<u>") && !inner.contains("</u>")).then_some(inner)
}

/// `text` with each `<u>…</u>` span wrapped in `marker` instead. Spaces just inside the tags move
/// outside the marks, where markdown needs them. An unclosed `<u>` is left as it is.
fn underlines(text: &str, marker: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<u>") {
        let after = &rest[start + "<u>".len()..];
        let Some(end) = after.find("</u>") else {
            break;
        };
        out.push_str(&rest[..start]);
        let inner = &after[..end];
        let trimmed = inner.trim();
        if trimmed.is_empty() {
            out.push_str(inner);
        } else {
            let lead = &inner[..inner.len() - inner.trim_start().len()];
            let trail = &inner[inner.trim_end().len()..];
            out.push_str(&format!("{lead}{marker}{trimmed}{marker}{trail}"));
        }
        rest = &after[end + "</u>".len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Transcript;

    fn tidy_default(text_md: &str) -> String {
        tidy_line(text_md, &MarkdownStyle::default())
    }

    #[test]
    fn dash_lines_become_rules() {
        for dashes in ["-", "—", "–", "---", " — ", "- - -", "——"] {
            assert_eq!(tidy_default(dashes), "---", "{dashes:?}");
        }
    }

    #[test]
    fn bullets_and_blank_lines_are_not_rules() {
        assert_eq!(tidy_default("- item"), "- item");
        assert_eq!(tidy_default("-- note"), "-- note");
        assert_eq!(tidy_default(""), "");
    }

    #[test]
    fn underlined_line_becomes_heading() {
        assert_eq!(tidy_default("<u>Sat 26th</u>"), "## Sat 26th");
        assert_eq!(tidy_default(" <u>So 27th, 08:00</u> "), "## So 27th, 08:00");
        assert_eq!(
            tidy_default("<u>Sat 26<sup>th</sup></u>"),
            "## Sat 26<sup>th</sup>"
        );
    }

    #[test]
    fn underlined_spans_get_marks() {
        assert_eq!(tidy_default("I am <u>so excited</u>"), "I am _so excited_");
        assert_eq!(
            tidy_default("<u>Great</u> experience"),
            "_Great_ experience"
        );
        assert_eq!(
            tidy_default("makes me <u>so</u> <u>so</u> happy"),
            "makes me _so_ _so_ happy"
        );
        assert_eq!(tidy_default("<u>a</u> and <u>b</u>"), "_a_ and _b_");
        assert_eq!(tidy_default("it's <u>so </u>good"), "it's _so_ good");
    }

    #[test]
    fn leaves_other_text_alone() {
        assert_eq!(tidy_default("snake_case"), "snake_case");
        assert_eq!(tidy_default("Fri 25^th^"), "Fri 25^th^");
        assert_eq!(tidy_default("~~laksa~~ LAKSA"), "~~laksa~~ LAKSA");
        assert_eq!(tidy_default("an <u>unclosed tag"), "an <u>unclosed tag");
        assert_eq!(tidy_default("x<u></u>"), "x", "empty tags dropped");
    }

    #[test]
    fn follows_the_style() {
        let style = MarkdownStyle {
            underline: "*".to_owned(),
            underlined_line_heading: 3,
        };
        assert_eq!(tidy_line("<u>Notes</u>", &style), "### Notes");
        assert_eq!(tidy_line("a <u>big</u> day", &style), "a *big* day");
    }

    fn line(text_md: &str) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: None,
        }
    }

    fn uncertain(line: usize, text: &str, alternatives: &[&str]) -> Uncertain {
        Uncertain {
            line,
            text: text.to_owned(),
            bbox: None,
            alternatives: alternatives.iter().map(|&a| a.to_owned()).collect(),
            resolved: false,
        }
    }

    fn tidied(lines: &[&str], uncertain: Vec<Uncertain>) -> Transcript {
        let mut t = Transcript::for_test(lines.iter().copied().map(line).collect(), uncertain);
        tidy(&mut t.lines, &mut t.uncertain, &MarkdownStyle::default());
        t
    }

    #[test]
    fn drops_alternatives_that_repeat_the_word() {
        let t = tidied(
            &["Kemptner Hütte", "threatening"],
            vec![
                uncertain(0, "Kemptner", &["Kemptner", "Kempter"]),
                uncertain(1, "threatening", &["threatening"]),
            ],
        );
        assert_eq!(t.uncertain[0].alternatives, ["Kempter"]);
        assert!(
            t.uncertain[1].alternatives.is_empty(),
            "kept, with no alternatives"
        );
    }

    #[test]
    fn underlined_uncertain_words_are_still_found() {
        let t = tidied(
            &[
                "<u>Sat 26th</u>",
                "<u>Great</u> experience",
                "am <u>so</u> tired",
            ],
            vec![
                uncertain(0, "<u>Sat 26th</u>", &["<u>Sat 25th</u>"]),
                uncertain(1, "<u>Great</u>", &[]),
                uncertain(2, "<u>so</u> tired", &["<u>so</u> tied"]),
            ],
        );
        let found: Vec<_> = t
            .uncertain_spans()
            .into_iter()
            .map(|s| s.map(|s| &t.lines[s.line].text_md[s.range]))
            .collect();
        assert_eq!(found, [Some("Sat 26th"), Some("Great"), Some("_so_ tired")]);
        assert_eq!(t.uncertain[0].alternatives, ["Sat 25th"]);
        assert_eq!(t.uncertain[2].alternatives, ["_so_ tied"]);
    }
}
