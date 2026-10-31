//! How transcripts of the same page by different models differ, for judging the models against
//! each other.

use notebook_core::cache::Cache;
use notebook_core::llm_model::Model;
use notebook_core::transcript::Transcript;

/// How a new transcript of a page differs from an older one, e.g. by another model.
#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    /// Lines, new then old.
    pub lines: (usize, usize),
    /// Uncertain words, new then old.
    pub uncertain: (usize, usize),
    /// Of the two markdowns' characters, the share in common: 1 if they're the same.
    pub similarity: f32,
    /// Characters inserted or deleted to get from the old markdown to the new.
    pub changed_chars: usize,
}

/// Compares `new` with `old`, character by character.
pub fn compare(new: &Transcript, old: &Transcript) -> Comparison {
    let (new_md, old_md) = (new.markdown(), old.markdown());
    let diff = similar::TextDiff::from_chars(old_md.as_str(), new_md.as_str());
    let changed_chars = diff
        .iter_all_changes()
        .filter(|c| c.tag() != similar::ChangeTag::Equal)
        .map(|c| c.value().chars().count())
        .sum();
    Comparison {
        lines: (new.lines.len(), old.lines.len()),
        uncertain: (new.uncertain.len(), old.uncertain.len()),
        similarity: diff.ratio(),
        changed_chars,
    }
}

/// How `transcript` of the photo `page_bytes` differs from each other model's cached transcript of
/// it. Empty if no other model has one.
pub fn with_cached(
    cache: &Cache,
    page_bytes: &[u8],
    transcript: &Transcript,
) -> Vec<(Model, Comparison)> {
    let made_by = Model::from_id(&transcript.model);
    Model::ALL
        .into_iter()
        .filter(|&model| Some(model) != made_by)
        .filter_map(|model| {
            Some((
                model,
                compare(
                    transcript,
                    &cache.get(page_bytes, model, &transcript.style)?,
                ),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use notebook_core::claude_transcribe::{Fetched, OcrError};
    use notebook_core::clean::Crop;
    use notebook_core::markdown_style::MarkdownStyle;
    use notebook_core::transcript::{Line, Uncertain};

    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../core/tests/fixtures/claude/success.json"),
        )
        .unwrap()
    }

    /// Opus's transcript of `photo`, stored in `cache` from the recorded response.
    fn cached_opus(cache: &Cache, photo: &[u8]) -> Transcript {
        cache
            .transcript_or(photo, Model::Opus, &MarkdownStyle::default(), || {
                Ok::<_, OcrError>(Fetched {
                    status: 200,
                    body: fixture(),
                    crop: Crop::whole(100, 100),
                })
            })
            .unwrap()
    }

    fn transcript(lines: &[&str], uncertain: &[&str]) -> Transcript {
        let line = |text: &&str| Line {
            text_md: (*text).to_owned(),
            bbox: None,
        };
        let word = |text: &&str| Uncertain {
            line: 0,
            text: (*text).to_owned(),
            bbox: None,
            alternatives: Vec::new(),
            resolved: false,
            deleted: None,
        };
        let dir = tempfile::tempdir().unwrap();
        Transcript {
            lines: lines.iter().map(line).collect(),
            uncertain: uncertain.iter().map(word).collect(),
            ..cached_opus(&Cache::new(dir.path()), b"photo")
        }
    }

    #[test]
    fn comparing_transcripts() {
        let old = transcript(&["buy eggs", "call Sam"], &["eggs"]);
        let same = compare(&old, &old);
        assert_eq!(same.similarity, 1.0);
        assert_eq!(same.changed_chars, 0);
        assert_eq!((same.lines, same.uncertain), ((2, 2), (1, 1)));

        let new = transcript(&["buy eggs", "call Sal", ""], &[]);
        let diff = compare(&new, &old);
        assert_eq!((diff.lines, diff.uncertain), ((3, 2), (0, 1)));
        // "m" out, "l" and the paragraph break's newline in.
        assert_eq!(diff.changed_chars, 3);
        assert!((0.8..1.0).contains(&diff.similarity), "{}", diff.similarity);
    }

    #[test]
    fn compared_with_other_models_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path());
        let photo: &[u8] = b"photo bytes";
        let opus = cached_opus(&cache, photo);
        let sonnet = Transcript {
            model: Model::Sonnet.id().to_owned(),
            ..opus.clone()
        };
        let comparisons = with_cached(&cache, photo, &sonnet);
        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].0, Model::Opus);
        assert_eq!(comparisons[0].1.similarity, 1.0);

        assert!(
            with_cached(&cache, photo, &opus).is_empty(),
            "not with its own model's entry"
        );
        assert!(with_cached(&cache, b"another photo", &sonnet).is_empty());
    }
}
