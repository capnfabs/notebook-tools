//! A page's transcription: Claude's answer, where it came from, and the user's corrections.
//!
//! Response shape: `decisions/2026-10-23-ocr-response-shape.md`.

use std::ops::Range;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer};

use crate::clean::Crop;
use crate::llm_model::Usage;

/// How many lines either side of its claimed line an uncertain word is looked for in.
/// See `decisions/2026-10-23-uncertain-words-nearby-lines.md`.
pub const LINE_SEARCH_RADIUS: usize = 2;

/// `[x0, y0, x1, y1]` in pixels of the image sent.
pub type Bbox = [f32; 4];

/// A page's transcription, as Claude returned it.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub lines: Vec<Line>,
    pub uncertain: Vec<Uncertain>,
    /// Where the image sent was cut from the photo, and its size, which the boxes are measured in.
    /// [`crate::reframe`] maps them onto another crop.
    pub crop: Crop,
    /// The model that answered; differs from the one asked for if a fallback ran.
    pub model: String,
    /// What the request cost; for a cached transcript, what it cost when it was made.
    pub usage: Usage,
    /// Loaded from the [`crate::cache`] rather than just transcribed.
    pub from_cache: bool,
    /// When the [`crate::cache`] stored it. `None` if it wasn't stored.
    pub stored_at: Option<SystemTime>,
    /// The user edited a line by hand. Never part of Claude's answer.
    pub edited: bool,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Line {
    pub text_md: String,
    /// `None` for paragraph breaks, or if Claude's box wasn't four numbers.
    #[serde(deserialize_with = "lenient_bbox")]
    pub bbox: Option<Bbox>,
}

/// A word or phrase Claude wasn't sure of.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Uncertain {
    /// Index into [`Transcript::lines`]. Claude sometimes miscounts this by one or so.
    pub line: usize,
    /// As it appears in that line's `text_md`.
    pub text: String,
    #[serde(deserialize_with = "lenient_bbox")]
    pub bbox: Option<Bbox>,
    pub alternatives: Vec<String>,
    /// The user corrected or accepted this word. Never part of Claude's answer.
    #[serde(skip)]
    pub resolved: bool,
}

/// Where an [`Uncertain`] word is: a byte range in one line's `text_md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub range: Range<usize>,
}

impl Transcript {
    /// The page as markdown: one line per handwritten line.
    pub fn markdown(&self) -> String {
        let lines: Vec<&str> = self.lines.iter().map(|l| l.text_md.as_str()).collect();
        lines.join("\n")
    }

    /// Where each [`Uncertain`] is, in the same order. Its text is looked for in the lines whose
    /// box holds its box's centre, then in its claimed line and the nearest lines within
    /// [`LINE_SEARCH_RADIUS`]: Claude's line index can be far off, its boxes rarely are. The first
    /// match in a line wins. `None` if it isn't found.
    pub fn uncertain_spans(&self) -> Vec<Option<Span>> {
        self.uncertain.iter().map(|u| self.locate(u)).collect()
    }

    fn locate(&self, u: &Uncertain) -> Option<Span> {
        if u.text.is_empty() {
            return None;
        }
        let by_box = u.bbox.map(|b| self.lines_by_box(b)).unwrap_or_default();
        let nearest_lines = (0..=LINE_SEARCH_RADIUS)
            .flat_map(|d| [u.line.checked_sub(d), u.line.checked_add(d)])
            .flatten();
        by_box.into_iter().chain(nearest_lines).find_map(|line| {
            let start = self.lines.get(line)?.text_md.find(&u.text)?;
            Some(Span {
                line,
                range: start..start + u.text.len(),
            })
        })
    }

    /// The line whose box holds the centre of `bbox`; the vertically nearest if several do.
    pub fn line_by_box(&self, bbox: Bbox) -> Option<usize> {
        self.lines_by_box(bbox).first().copied()
    }

    /// The lines whose boxes hold the centre of `bbox`, vertically nearest first.
    fn lines_by_box(&self, [x0, y0, x1, y1]: Bbox) -> Vec<usize> {
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let mut lines: Vec<(usize, f32)> = self
            .lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| Some((i, l.bbox?)))
            .filter(|(_, [lx0, ly0, lx1, ly1])| {
                (*lx0..=*lx1).contains(&cx) && (*ly0..=*ly1).contains(&cy)
            })
            .map(|(i, [_, ly0, _, ly1])| (i, ((ly0 + ly1) / 2.0 - cy).abs()))
            .collect();
        lines.sort_by(|(_, a), (_, b)| a.total_cmp(b));
        lines.into_iter().map(|(i, _)| i).collect()
    }

    /// Replaces uncertain word `index` with `replacement` and marks it resolved, keeping the old
    /// reading among its alternatives. Returns false, changing nothing, if the word isn't found.
    pub fn resolve(&mut self, index: usize, replacement: &str) -> bool {
        let Some(u) = self.uncertain.get(index) else {
            return false;
        };
        let Some(Span { line, range }) = self.locate(u) else {
            return false;
        };
        self.lines[line].text_md.replace_range(range, replacement);
        let u = &mut self.uncertain[index];
        let old = std::mem::replace(&mut u.text, replacement.to_owned());
        if old != replacement && !u.alternatives.contains(&old) {
            u.alternatives.insert(0, old);
        }
        u.alternatives.retain(|a| a != replacement);
        u.line = line;
        u.resolved = true;
        true
    }

    /// Replaces line `index`'s markdown. Uncertain words no longer in it stop being found.
    pub fn set_line(&mut self, index: usize, text_md: String) {
        if let Some(line) = self.lines.get_mut(index)
            && line.text_md != text_md
        {
            line.text_md = text_md;
            self.edited = true;
        }
    }

    /// Uncertain words still to correct or keep: found in the text and not yet resolved.
    pub fn open_uncertain(&self) -> usize {
        self.uncertain
            .iter()
            .filter(|u| !u.resolved && self.locate(u).is_some())
            .count()
    }

    /// The user has corrected or kept an uncertain word, or edited a line: work that
    /// transcribing the page again would lose.
    pub fn corrected(&self) -> bool {
        self.edited || self.uncertain.iter().any(|u| u.resolved)
    }
}

/// A box that isn't exactly four numbers becomes `None` rather than failing the whole page.
fn lenient_bbox<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Bbox>, D::Error> {
    let bbox = Option::<Vec<f32>>::deserialize(d)?;
    Ok(bbox.and_then(|b| b.try_into().ok()))
}

#[cfg(test)]
impl Transcript {
    /// A fresh transcript by Opus of a whole 100x100 photo.
    pub(crate) fn for_test(lines: Vec<Line>, uncertain: Vec<Uncertain>) -> Self {
        Self {
            lines,
            uncertain,
            crop: Crop::whole(100, 100),
            model: crate::llm_model::Model::Opus.id().to_owned(),
            usage: Usage::default(),
            from_cache: false,
            stored_at: None,
            edited: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text_md: &str) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: (!text_md.is_empty()).then_some([0.0, 0.0, 10.0, 10.0]),
        }
    }

    fn uncertain(line: usize, text: &str) -> Uncertain {
        Uncertain {
            line,
            text: text.to_owned(),
            // Found by its line index alone.
            bbox: None,
            alternatives: vec!["other".to_owned()],
            resolved: false,
        }
    }

    /// A line with a box at row `row`, 100px tall.
    fn line_at(text_md: &str, row: f32) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: Some([0.0, row * 100.0, 1000.0, row * 100.0 + 100.0]),
        }
    }

    /// Text of `t`'s uncertain word `index` as found, and its line.
    fn found(t: &Transcript, index: usize) -> Option<(usize, &str)> {
        let Span { line, range } = t.uncertain_spans()[index].clone()?;
        Some((line, &t.lines[line].text_md[range]))
    }

    fn transcript(lines: &[&str], uncertain: Vec<Uncertain>) -> Transcript {
        Transcript::for_test(lines.iter().copied().map(line).collect(), uncertain)
    }

    #[test]
    fn markdown_joins_lines_with_paragraph_breaks() {
        let t = transcript(&["# Monday", "- eggs", "", "Call Sam"], vec![]);
        assert_eq!(t.markdown(), "# Monday\n- eggs\n\nCall Sam");
    }

    #[test]
    fn uncertain_spans_point_into_lines() {
        let t = transcript(
            &["# Monday", "", "buy eggs and eggs", "café visit"],
            vec![
                uncertain(2, "eggs"),
                uncertain(3, "visit"),
                uncertain(3, "café"),
                uncertain(0, "Tuesday"),
                uncertain(9, "eggs"),
                uncertain(2, ""),
            ],
        );
        let spans = t.uncertain_spans();
        assert_eq!(
            spans[0],
            Some(Span {
                line: 2,
                range: 4..8
            }),
            "first match in the line"
        );
        assert_eq!(found(&t, 1), Some((3, "visit")));
        assert_eq!(found(&t, 2), Some((3, "café")));
        assert_eq!(spans[3..], [None, None, None]);
    }

    /// Seen in a live response: line indices one off in both directions.
    #[test]
    fn uncertain_spans_tolerate_miscounted_lines() {
        let lines = ["a", "b", "eggs", "c", "d", "e", "eggs too"];
        let line_of = |claimed: usize, text: &str| {
            let t = transcript(&lines, vec![uncertain(claimed, text)]);
            Some(found(&t, 0)?.0)
        };
        assert_eq!(line_of(3, "eggs"), Some(2), "one too high");
        assert_eq!(line_of(5, "eggs"), Some(6), "one too low");
        assert_eq!(line_of(4, "eggs"), Some(2), "earlier line wins a tie");
        assert_eq!(line_of(0, "too"), None, "too far away");
    }

    /// Claude's line index can be ~15 off; its boxes are better.
    #[test]
    fn uncertain_spans_go_by_box_first() {
        let lines: Vec<Line> = ["eggs", "a", "b", "c", "d", "e", "eggs"]
            .iter()
            .zip(0..)
            .map(|(text, row)| line_at(text, row as f32))
            .collect();
        let word = |claimed, bbox| Uncertain {
            bbox,
            ..uncertain(claimed, "eggs")
        };
        let t = Transcript {
            lines,
            uncertain: vec![
                // Claims line 0, but the box is on line 6, and a little low.
                word(0, Some([10.0, 640.0, 60.0, 720.0])),
                // A box between lines falls back to the index.
                word(1, Some([10.0, 1000.0, 60.0, 1100.0])),
                // The box's line doesn't have the text.
                word(6, Some([10.0, 300.0, 60.0, 350.0])),
                word(0, None),
            ],
            ..transcript(&[], vec![])
        };
        let lines: Vec<_> = (0..4).map(|i| found(&t, i).map(|(line, _)| line)).collect();
        assert_eq!(lines, [Some(6), Some(0), Some(6), Some(0)]);
        assert_eq!(t.line_by_box([10.0, 640.0, 60.0, 720.0]), Some(6));
        assert_eq!(t.line_by_box([10.0, 1000.0, 60.0, 1100.0]), None);
    }

    #[test]
    fn resolving_replaces_the_word_and_keeps_the_old_reading() {
        let mut t = transcript(
            &["ring the plumber re: tap"],
            vec![Uncertain {
                alternatives: vec!["plumbers".into(), "plunber".into()],
                ..uncertain(0, "plumber")
            }],
        );
        assert!(t.resolve(0, "plumbers"));
        assert_eq!(t.lines[0].text_md, "ring the plumbers re: tap");
        let u = &t.uncertain[0];
        assert!(u.resolved);
        assert_eq!(u.text, "plumbers");
        assert_eq!(u.alternatives, ["plumber", "plunber"]);
        assert_eq!(found(&t, 0), Some((0, "plumbers")));

        // Back again, then a reading of the user's own.
        assert!(t.resolve(0, "plumber"));
        assert_eq!(t.uncertain[0].alternatives, ["plumbers", "plunber"]);
        assert!(t.resolve(0, "Plumbing Co"));
        assert_eq!(t.lines[0].text_md, "ring the Plumbing Co re: tap");
        assert_eq!(
            t.uncertain[0].alternatives,
            ["plumber", "plumbers", "plunber"]
        );
    }

    #[test]
    fn keeping_a_reading_resolves_it_unchanged() {
        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "eggs")]);
        assert!(t.resolve(0, "eggs"));
        assert_eq!(t.lines[0].text_md, "buy eggs");
        assert!(t.uncertain[0].resolved);
        assert_eq!(t.uncertain[0].alternatives, ["other"]);
    }

    #[test]
    fn unfound_words_cant_be_resolved() {
        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "milk")]);
        let before = t.clone();
        assert!(!t.resolve(0, "cheese"));
        assert!(!t.resolve(5, "cheese"));
        assert_eq!(t, before);
    }

    #[test]
    fn editing_a_line_replaces_its_markdown() {
        let mut t = transcript(&["buy eggs", "call Sam"], vec![uncertain(0, "eggs")]);
        t.set_line(0, "buy milk".into());
        assert_eq!(t.markdown(), "buy milk\ncall Sam");
        assert_eq!(t.uncertain_spans(), [None]);
        t.set_line(9, "ignored".into());
        assert_eq!(t.markdown(), "buy milk\ncall Sam");
    }

    #[test]
    fn corrections_are_noticed() {
        let mut t = transcript(&["buy eggs", "call Sam"], vec![uncertain(0, "eggs")]);
        assert!(!t.corrected());
        t.set_line(1, "call Sam".into());
        assert!(!t.corrected(), "unchanged line");
        t.set_line(1, "call Sam!".into());
        assert!(t.corrected());

        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "eggs")]);
        t.resolve(0, "eggs");
        assert!(t.corrected(), "kept a reading");
    }

    #[test]
    fn open_uncertain_counts_found_unresolved_words() {
        let mut t = transcript(
            &["buy eggs and milk"],
            vec![
                uncertain(0, "eggs"),
                uncertain(0, "milk"),
                uncertain(0, "cheese"),
            ],
        );
        assert_eq!(t.open_uncertain(), 2, "cheese isn't in the text");
        t.resolve(0, "eggs");
        assert_eq!(t.open_uncertain(), 1);
        t.resolve(1, "mint");
        assert_eq!(t.open_uncertain(), 0);
    }

    #[test]
    fn malformed_box_becomes_none() {
        let line: Line = serde_json::from_str(r#"{"text_md": "hi", "bbox": [1, 2, 3]}"#).unwrap();
        assert_eq!(line.bbox, None);
    }
}
