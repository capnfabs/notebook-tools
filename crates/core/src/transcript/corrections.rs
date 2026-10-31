//! The user's corrections to a [`Transcript`]: resolving uncertain words, deleting struck text,
//! editing lines, and going back to Claude's transcription. See
//! `decisions/2026-10-23-transcript-corrections.md` and `decisions/2026-10-30-correction-ui.md`.

use std::ops::Range;

use super::{BeforeCorrections, Deleted, Side, Span, Transcript, struck_ranges};

impl Transcript {
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
        self.keep_before_corrections();
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
        self.keep_before_corrections();
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
        self.keep_before_corrections();
        self.splice(line, range, "");
        true
    }

    /// Replaces line `index`'s markdown. Uncertain words no longer in it stop being found, and
    /// words deleted from it can't be put back.
    pub fn set_line(&mut self, index: usize, text_md: String) {
        if self.lines.get(index).is_none_or(|l| l.text_md == text_md) {
            return;
        }
        self.keep_before_corrections();
        self.lines[index].text_md = text_md;
        for u in &mut self.uncertain {
            if u.deleted.is_some_and(|d| d.line == index) {
                u.deleted = None;
            }
        }
    }

    /// Drops the user's corrections, back to Claude's transcription.
    pub fn reset(&mut self) {
        if let Some(original) = self.before_corrections.take() {
            let BeforeCorrections { lines, uncertain } = *original;
            self.lines = lines;
            self.uncertain = uncertain;
        }
    }

    /// The user has corrected or kept an uncertain word, or edited a line: work that
    /// transcribing the page again would lose.
    pub fn corrected(&self) -> bool {
        self.before_corrections.is_some()
    }

    /// Keeps the text as it is, if it's still Claude's, before the user changes it.
    fn keep_before_corrections(&mut self) {
        if self.before_corrections.is_none() {
            self.before_corrections = Some(Box::new(BeforeCorrections {
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

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::super::{Uncertain, Unfound};

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
}
