//! A page's transcription: Claude's answer, where it came from, and the user's corrections.
//!
//! Response shape: `decisions/2026-10-23-ocr-response-shape.md`. Struck text and resetting:
//! `decisions/2026-10-30-correction-ui.md`.

use std::ops::Range;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer};

use crate::clean::Crop;
use crate::llm_model::Usage;
use crate::markdown_style::MarkdownStyle;

/// How many lines either side of its claimed line an uncertain word is looked for in.
/// See `decisions/2026-10-23-uncertain-words-nearby-lines.md`.
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
    pub original: Option<Box<Original>>,
}

/// A [`Transcript`]'s text before the user corrected it.
#[derive(Clone, Debug, PartialEq)]
pub struct Original {
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

    /// The line whose box holds the centre of `bbox`; the vertically nearest if several do.
    pub fn line_by_box(&self, bbox: Bbox) -> Option<usize> {
        lines_by_box(&self.lines, bbox).first().copied()
    }

    /// Why uncertain word `index` isn't found in the text; `None` if it is.
    pub fn why_unfound(&self, index: usize) -> Option<Unfound> {
        let u = self.uncertain.get(index)?;
        if self.locate(u).is_some() {
            return None;
        }
        let was = self
            .original
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

    /// Replaces uncertain word `index` with `replacement` and marks it resolved, keeping the old
    /// reading among its alternatives. A deleted word is put back where it was. Returns false,
    /// changing nothing, if the word isn't found.
    pub fn resolve(&mut self, index: usize, replacement: &str) -> bool {
        let Some(u) = self.uncertain.get(index) else {
            return false;
        };
        let Some(Span { line, range }) = self.locate(u) else {
            return false;
        };
        self.keep_original();
        let with = match self.uncertain[index].deleted.take().and_then(|d| d.space) {
            Some(Side::Before) => format!(" {replacement}"),
            Some(Side::After) => format!("{replacement} "),
            None => replacement.to_owned(),
        };
        self.splice(line, range, &with);
        let u = &mut self.uncertain[index];
        let old = std::mem::replace(&mut u.text, replacement.to_owned());
        if !old.is_empty() && old != replacement && !u.alternatives.contains(&old) {
            u.alternatives.insert(0, old);
        }
        u.alternatives.retain(|a| a != replacement);
        u.line = line;
        u.resolved = true;
        true
    }

    /// Takes the struck-through text out of uncertain word `index`, resolving it to what's left.
    /// If that's nothing, the word is deleted along with a space beside it, and its reading kept
    /// among its alternatives. Returns false, changing nothing, if the word isn't found or has no
    /// struck text.
    pub fn delete_struck(&mut self, index: usize) -> bool {
        let Some(u) = self.uncertain.get(index) else {
            return false;
        };
        let Some(Span { line, range }) = self.locate(u) else {
            return false;
        };
        let kept = without_strikes(&u.text);
        if kept == u.text {
            return false;
        }
        if !kept.is_empty() {
            return self.resolve(index, &kept);
        }
        self.keep_original();
        let (range, space) = with_a_space(&self.lines[line].text_md, range);
        self.splice(line, range.clone(), "");
        let u = &mut self.uncertain[index];
        let old = std::mem::take(&mut u.text);
        if !u.alternatives.contains(&old) {
            u.alternatives.insert(0, old);
        }
        u.deleted = Some(Deleted {
            line,
            at: range.start,
            space,
        });
        u.line = line;
        u.resolved = true;
        true
    }

    /// Deletes the struck-through text at `range` of line `line`, and a space beside it. Returns
    /// false, changing nothing, if there's no struck text there.
    pub fn delete_struck_at(&mut self, line: usize, range: Range<usize>) -> bool {
        let Some(text) = self.lines.get(line).map(|l| &l.text_md) else {
            return false;
        };
        if !struck_ranges(text).contains(&range) {
            return false;
        }
        let (range, _) = with_a_space(text, range);
        self.keep_original();
        self.splice(line, range, "");
        true
    }

    /// Replaces line `index`'s markdown. Uncertain words no longer in it stop being found, and
    /// words deleted from it can't be put back.
    pub fn set_line(&mut self, index: usize, text_md: String) {
        if self.lines.get(index).is_none_or(|l| l.text_md == text_md) {
            return;
        }
        self.keep_original();
        self.lines[index].text_md = text_md;
        for u in &mut self.uncertain {
            if u.deleted.is_some_and(|d| d.line == index) {
                u.deleted = None;
            }
        }
    }

    /// Drops the user's corrections, back to Claude's transcription.
    pub fn reset(&mut self) {
        if let Some(original) = self.original.take() {
            let Original { lines, uncertain } = *original;
            self.lines = lines;
            self.uncertain = uncertain;
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
        self.original.is_some()
    }

    /// Keeps the text as it is, if it's still Claude's, before the user changes it.
    fn keep_original(&mut self) {
        if self.original.is_none() {
            self.original = Some(Box::new(Original {
                lines: self.lines.clone(),
                uncertain: self.uncertain.clone(),
            }));
        }
    }

    /// Replaces `range` of line `line` with `with`, moving the places of words deleted from
    /// further along it to match.
    fn splice(&mut self, line: usize, range: Range<usize>, with: &str) {
        self.lines[line].text_md.replace_range(range.clone(), with);
        for d in self.uncertain.iter_mut().filter_map(|u| u.deleted.as_mut()) {
            if d.line == line && d.at >= range.end {
                d.at = d.at - range.len() + with.len();
            } else if d.line == line && d.at > range.start {
                d.at = range.start;
            }
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

/// `text` less its struck-through parts, with the spaces left behind tidied.
fn without_strikes(text: &str) -> String {
    let mut kept = String::new();
    let mut at = 0;
    for range in struck_ranges(text) {
        kept.push_str(&text[at..range.start]);
        at = range.end;
    }
    kept.push_str(&text[at..]);
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `range` of `text` and a space after it, else before it, if there is one, and which it took.
fn with_a_space(text: &str, range: Range<usize>) -> (Range<usize>, Option<Side>) {
    if text[range.end..].starts_with(' ') {
        (range.start..range.end + 1, Some(Side::After))
    } else if text[..range.start].ends_with(' ') {
        (range.start - 1..range.end, Some(Side::Before))
    } else {
        (range, None)
    }
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
        Self {
            lines,
            uncertain,
            crop: Crop::whole(100, 100),
            style: MarkdownStyle::default(),
            model: crate::llm_model::Model::Opus.id().to_owned(),
            usage: Usage::default(),
            from_cache: false,
            stored_at: None,
            original: None,
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
            deleted: None,
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
    fn deleting_a_struck_word_and_putting_it_back() {
        let mut t = transcript(
            &["re: ~~tap~~ shower, ~~sink~~"],
            vec![
                Uncertain {
                    alternatives: vec!["~~top~~".into()],
                    ..uncertain(0, "~~tap~~")
                },
                uncertain(0, "~~sink~~"),
                uncertain(0, "shower"),
            ],
        );
        assert!(t.delete_struck(1));
        assert_eq!(t.lines[0].text_md, "re: ~~tap~~ shower,", "space before");
        assert!(t.delete_struck(0));
        assert_eq!(t.lines[0].text_md, "re: shower,", "space after");
        let u = &t.uncertain[0];
        assert!(u.resolved);
        assert_eq!(
            (u.text.as_str(), &u.alternatives[..]),
            ("", &["~~tap~~".to_owned(), "~~top~~".to_owned()][..])
        );
        assert!(!t.delete_struck(0), "nothing left to delete");

        // Edits before a deleted word keep its place.
        assert!(t.resolve(2, "showers"));
        assert_eq!(t.lines[0].text_md, "re: showers,");
        assert!(t.resolve(1, "~~sink~~"));
        assert!(t.resolve(0, "~~top~~"));
        assert_eq!(t.lines[0].text_md, "re: ~~top~~ showers, ~~sink~~");
        assert_eq!(t.uncertain[0].alternatives, ["~~tap~~"]);
        assert_eq!(t.uncertain[0].deleted, None);
    }

    #[test]
    fn deleting_struck_text_keeps_the_rest_of_a_word() {
        let mut t = transcript(
            &["~~tap~~ shower here"],
            vec![uncertain(0, "~~tap~~ shower")],
        );
        assert!(t.delete_struck(0));
        assert_eq!(t.lines[0].text_md, "shower here");
        assert_eq!(t.uncertain[0].text, "shower");
        assert_eq!(t.uncertain[0].alternatives, ["~~tap~~ shower", "other"]);

        let mut t = transcript(&["buy eggs"], vec![uncertain(0, "eggs")]);
        assert!(!t.delete_struck(0), "nothing struck");
        assert!(!t.corrected());
    }

    #[test]
    fn deleting_plain_struck_text() {
        let mut t = transcript(&["re: ~~tap~~ shower"], vec![]);
        assert!(!t.delete_struck_at(0, 0..4), "not struck");
        assert!(!t.delete_struck_at(3, 4..11));
        assert!(!t.corrected());
        assert!(t.delete_struck_at(0, 4..11));
        assert_eq!(t.lines[0].text_md, "re: shower");
    }

    #[test]
    fn editing_a_deleted_words_line_forgets_its_place() {
        let mut t = transcript(&["~~tap~~ shower"], vec![uncertain(0, "~~tap~~")]);
        t.delete_struck(0);
        t.set_line(0, "shower head".into());
        assert_eq!(t.uncertain_spans(), [None]);
        assert_eq!(t.why_unfound(0), Some(Unfound::EditedOut { line: 0 }));
    }

    #[test]
    fn unfound_words_say_why() {
        let mut t = transcript(
            &["buy eggs", "call Sam"],
            vec![uncertain(1, "Sam"), uncertain(0, "milk")],
        );
        assert_eq!(t.why_unfound(0), None);
        assert_eq!(t.why_unfound(1), Some(Unfound::NotInText));
        t.resolve(0, "Sim");
        t.set_line(1, "call Pat".into());
        assert_eq!(t.why_unfound(0), Some(Unfound::EditedOut { line: 1 }));
        assert_eq!(t.why_unfound(1), Some(Unfound::NotInText));
        assert_eq!(t.why_unfound(9), None);
    }

    #[test]
    fn reset_goes_back_to_claudes_transcription() {
        let before = transcript(&["~~tap~~ eggs", "call Sam"], vec![uncertain(0, "eggs")]);
        let mut t = before.clone();
        t.reset();
        assert_eq!(t, before, "nothing to reset");
        t.resolve(0, "milk");
        t.delete_struck_at(0, 0..7);
        t.set_line(1, "call Pat".into());
        assert!(t.corrected());
        t.reset();
        assert_eq!(t, before);
        assert!(!t.corrected());
    }

    #[test]
    fn malformed_box_becomes_none() {
        let line: Line = serde_json::from_str(r#"{"text_md": "hi", "bbox": [1, 2, 3]}"#).unwrap();
        assert_eq!(line.bbox, None);
    }
}
