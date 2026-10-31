//! A page's transcription: Claude's answer, where it came from, and finding its uncertain words in
//! the text. The user's corrections are in [`corrections`]; mapping boxes onto other corners is in
//! [`crate::reframe`] ([`Transcript::reframed`], [`Transcript::fit`]).
//!
//! Response shape: `decisions/2026-10-23-ocr-response-shape.md`.

mod corrections;

use std::ops::Range;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer};

use crate::clean::Crop;
use crate::llm_model::Usage;
use crate::markdown_style::MarkdownStyle;

/// How many lines either side of its claimed line an uncertain word is looked for in.
/// See `decisions/2026-10-31-finding-uncertain-words.md`.
pub const LINE_SEARCH_RADIUS: usize = 2;

/// Wraps struck-through text, as in `~~tap~~`.
const STRIKE: &str = "~~";

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
    /// The style Claude's markup was converted to when it was parsed.
    pub style: MarkdownStyle,
    /// The model that answered; differs from the one asked for if a fallback ran.
    pub model: String,
    /// What the request cost; for a cached transcript, what it cost when it was made.
    pub usage: Usage,
    /// Loaded from the [`crate::cache`] rather than just transcribed.
    pub from_cache: bool,
    /// When the [`crate::cache`] stored it. `None` if it wasn't stored.
    pub stored_at: Option<SystemTime>,
    /// The lines and uncertain words before the user's first correction, for
    /// [`Transcript::reset`]. `None` until then.
    pub before_corrections: Option<Box<BeforeCorrections>>,
}

/// A [`Transcript`]'s text before the user corrected it.
#[derive(Clone, Debug, PartialEq)]
pub struct BeforeCorrections {
    pub lines: Vec<Line>,
    pub uncertain: Vec<Uncertain>,
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
    /// As it appears in that line's `text_md`. Empty once deleted.
    pub text: String,
    #[serde(deserialize_with = "lenient_bbox")]
    pub bbox: Option<Bbox>,
    pub alternatives: Vec<String>,
    /// The user corrected or accepted this word. Never part of Claude's answer.
    #[serde(skip)]
    pub resolved: bool,
    /// Where the word was, if the user deleted it, so a reading can be put back there. Never part
    /// of Claude's answer.
    #[serde(skip)]
    pub deleted: Option<Deleted>,
}

/// Where a deleted [`Uncertain`] word was: `at` bytes into line `line`, which lost a space on
/// `space`'s side along with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deleted {
    pub line: usize,
    pub at: usize,
    pub space: Option<Side>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

/// Where an [`Uncertain`] word is: a byte range in one line's `text_md`. Empty for a deleted
/// word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub range: Range<usize>,
}

/// Why an [`Uncertain`] word isn't found in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unfound {
    /// It was in line `line` until the user edited the line.
    EditedOut { line: usize },
    /// Claude's own text doesn't have it near where it said.
    NotInText,
}

impl Transcript {
    /// A transcript just made by `model`, before any corrections.
    pub fn new(
        lines: Vec<Line>,
        uncertain: Vec<Uncertain>,
        crop: Crop,
        style: MarkdownStyle,
        model: String,
        usage: Usage,
    ) -> Self {
        Self {
            lines,
            uncertain,
            crop,
            style,
            model,
            usage,
            from_cache: false,
            stored_at: None,
            before_corrections: None,
        }
    }

    /// The page as markdown: one line per handwritten line, plus a blank line before a rule that
    /// follows text, which would otherwise make the text a heading.
    pub fn markdown(&self) -> String {
        let mut markdown = String::new();
        let mut previous: Option<&str> = None;
        for line in &self.lines {
            let text = line.text_md.as_str();
            if let Some(previous) = previous {
                markdown.push('\n');
                if !previous.trim().is_empty() && is_setext_underline(text) {
                    markdown.push('\n');
                }
            }
            markdown.push_str(text);
            previous = Some(text);
        }
        markdown
    }

    /// Where each [`Uncertain`] is, in the same order. Its text is looked for in the lines whose
    /// box holds its box's centre, then in its claimed line and the nearest lines within
    /// [`LINE_SEARCH_RADIUS`]: Claude's line index can be far off, its boxes rarely are. The first
    /// match in a line wins. A deleted word is where it was deleted from. `None` if it isn't found.
    pub fn uncertain_spans(&self) -> Vec<Option<Span>> {
        self.uncertain.iter().map(|u| self.locate(u)).collect()
    }

    fn locate(&self, u: &Uncertain) -> Option<Span> {
        locate(&self.lines, u)
    }

    /// Why uncertain word `index` isn't found in the text; `None` if it is.
    pub fn why_unfound(&self, index: usize) -> Option<Unfound> {
        let u = self.uncertain.get(index)?;
        if self.locate(u).is_some() {
            return None;
        }
        let was = self
            .before_corrections
            .as_ref()
            .and_then(|o| locate(&o.lines, o.uncertain.get(index)?));
        Some(match was {
            Some(Span { line, .. }) => Unfound::EditedOut { line },
            None => Unfound::NotInText,
        })
    }

    /// Makes uncertain words that are all of some struck-through text, `tap` in `~~tap~~`, take in
    /// the strike, and their alternatives with them, so a correction keeps it.
    pub fn widen_to_strikes(&mut self) {
        for i in 0..self.uncertain.len() {
            let Some(Span { line, range }) = self.locate(&self.uncertain[i]) else {
                continue;
            };
            let Some(widened) = range
                .start
                .checked_sub(STRIKE.len())
                .map(|start| start..range.end + STRIKE.len())
            else {
                continue;
            };
            if !struck_ranges(&self.lines[line].text_md).contains(&widened) {
                continue;
            }
            let u = &mut self.uncertain[i];
            for text in std::iter::once(&mut u.text).chain(&mut u.alternatives) {
                *text = format!("{STRIKE}{text}{STRIKE}");
            }
        }
    }

    /// Uncertain words still to correct or keep: found in the text and not yet resolved.
    pub fn open_uncertain(&self) -> usize {
        self.uncertain
            .iter()
            .filter(|u| !u.resolved && self.locate(u).is_some())
            .count()
    }
}

impl Uncertain {
    /// What the word reads as: its text, or for a deleted word, the reading it had.
    pub fn reading(&self) -> &str {
        match self.text.as_str() {
            "" => self.alternatives.first().map_or("", String::as_str),
            text => text,
        }
    }
}

/// Where `u` is in `lines`: see [`Transcript::uncertain_spans`].
fn locate(lines: &[Line], u: &Uncertain) -> Option<Span> {
    if let Some(Deleted { line, at, .. }) = u.deleted {
        let text = &lines.get(line)?.text_md;
        return text.is_char_boundary(at).then_some(Span {
            line,
            range: at..at,
        });
    }
    if u.text.is_empty() {
        return None;
    }
    let by_box = u.bbox.map(|b| lines_by_box(lines, b)).unwrap_or_default();
    let nearest_lines = (0..=LINE_SEARCH_RADIUS)
        .flat_map(|d| [u.line.checked_sub(d), u.line.checked_add(d)])
        .flatten();
    by_box.into_iter().chain(nearest_lines).find_map(|line| {
        let start = lines.get(line)?.text_md.find(&u.text)?;
        Some(Span {
            line,
            range: start..start + u.text.len(),
        })
    })
}

/// The lines whose boxes hold the centre of `bbox`, vertically nearest first.
fn lines_by_box(lines: &[Line], [x0, y0, x1, y1]: Bbox) -> Vec<usize> {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let mut lines: Vec<(usize, f32)> = lines
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

/// The struck-through text in `text`, each `~~…~~` with its tildes, in order.
pub fn struck_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find(STRIKE).map(|i| from + i) {
        let inside = start + STRIKE.len();
        let Some(end) = text[inside..]
            .find(STRIKE)
            .map(|i| inside + i + STRIKE.len())
        else {
            break;
        };
        if end - start > 2 * STRIKE.len() {
            ranges.push(start..end);
        }
        from = end;
    }
    ranges
}

/// Only `-`s or only `=`s, which right under text make it a heading.
fn is_setext_underline(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && (text.chars().all(|c| c == '-') || text.chars().all(|c| c == '='))
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
        let model = crate::llm_model::Model::Opus.id().to_owned();
        let crop = Crop::whole(100, 100);
        Self::new(
            lines,
            uncertain,
            crop,
            MarkdownStyle::default(),
            model,
            Usage::default(),
        )
    }
}

#[cfg(test)]
impl Uncertain {
    /// An unresolved word in line `line`, with no box and no other readings.
    pub(crate) fn for_test(line: usize, text: &str) -> Self {
        Self {
            line,
            text: text.to_owned(),
            bbox: None,
            alternatives: Vec::new(),
            resolved: false,
            deleted: None,
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! Helpers for this module's tests and [`super::corrections`]'.

    use super::*;

    pub fn line(text_md: &str) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: (!text_md.is_empty()).then_some([0.0, 0.0, 10.0, 10.0]),
        }
    }

    /// Found by its line index alone, with one other reading.
    pub fn uncertain(line: usize, text: &str) -> Uncertain {
        Uncertain {
            alternatives: vec!["other".to_owned()],
            ..Uncertain::for_test(line, text)
        }
    }

    /// A line with a box at row `row`, 100px tall.
    pub fn line_at(text_md: &str, row: f32) -> Line {
        Line {
            text_md: text_md.to_owned(),
            bbox: Some([0.0, row * 100.0, 1000.0, row * 100.0 + 100.0]),
        }
    }

    /// Text of `t`'s uncertain word `index` as found, and its line.
    pub fn found(t: &Transcript, index: usize) -> Option<(usize, &str)> {
        let Span { line, range } = t.uncertain_spans()[index].clone()?;
        Some((line, &t.lines[line].text_md[range]))
    }

    pub fn transcript(lines: &[&str], uncertain: Vec<Uncertain>) -> Transcript {
        Transcript::for_test(lines.iter().copied().map(line).collect(), uncertain)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn markdown_joins_lines_with_paragraph_breaks() {
        let t = transcript(&["# Monday", "- eggs", "", "Call Sam"], vec![]);
        assert_eq!(t.markdown(), "# Monday\n- eggs\n\nCall Sam");
    }

    #[test]
    fn markdown_keeps_rules_from_making_headings() {
        let t = transcript(&["---", "Fri", "---", "", "---", "Sat", "==="], vec![]);
        assert_eq!(t.markdown(), "---\nFri\n\n---\n\n---\nSat\n\n===");
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
    }

    #[test]
    fn struck_ranges_take_in_the_tildes() {
        assert_eq!(struck_ranges("re: ~~tap~~ shower"), vec![(4..11)]);
        assert_eq!(struck_ranges("~~a~~ b ~~c d~~"), [0..5, 8..15]);
        assert_eq!(
            struck_ranges("~~~~ ~~open"),
            [] as [Range<usize>; 0],
            "empty, unclosed"
        );
    }

    #[test]
    fn uncertain_words_widen_to_their_strike() {
        let mut t = transcript(
            &["re: ~~tap~~ shower", "~~tap shower~~"],
            vec![
                Uncertain {
                    alternatives: vec!["top".into()],
                    ..uncertain(0, "tap")
                },
                uncertain(0, "shower"),
                uncertain(1, "tap"),
            ],
        );
        t.widen_to_strikes();
        let texts: Vec<_> = t.uncertain.iter().map(|u| u.text.as_str()).collect();
        assert_eq!(texts, ["~~tap~~", "shower", "tap"], "only a whole strike");
        assert_eq!(t.uncertain[0].alternatives, ["~~top~~"]);
        assert_eq!(found(&t, 0), Some((0, "~~tap~~")));
    }

    #[test]
    fn a_deleted_word_reads_as_its_old_reading() {
        let mut t = transcript(&["re: ~~tap~~ shower"], vec![uncertain(0, "~~tap~~")]);
        assert_eq!(t.uncertain[0].reading(), "~~tap~~");
        t.delete_struck(0);
        assert_eq!(t.uncertain[0].text, "");
        assert_eq!(t.uncertain[0].reading(), "~~tap~~");
        assert_eq!(Uncertain::for_test(0, "").reading(), "", "nothing to read");
    }

    #[test]
    fn malformed_box_becomes_none() {
        let line: Line = serde_json::from_str(r#"{"text_md": "hi", "bbox": [1, 2, 3]}"#).unwrap();
        assert_eq!(line.bbox, None);
    }
}
