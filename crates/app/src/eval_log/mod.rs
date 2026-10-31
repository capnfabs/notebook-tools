//! Loglines for judging how well each model transcribes, and what it costs: one per transcript,
//! comparison with another model's, and correction. They're logged with target `eval`, so
//! `RUST_LOG=eval=info` turns them on. See `decisions/2026-10-31-transcription-models.md`.

pub mod compare;

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use notebook_core::cache::Cache;
use notebook_core::llm_model::{Model, Usage};
use notebook_core::transcript::Transcript;

use crate::transcriptions::OnTranscribeCompleted;
use compare::Comparison;

/// Whether eval loglines are being logged.
pub fn enabled() -> bool {
    log::log_enabled!(target: "eval", log::Level::Info)
}

/// Logs how each page sent to Claude differs from other models' transcripts of it in `cache`, if
/// there are any. Reads and compares on the transcription threads, off the UI's.
pub fn log_comparisons(cache: Cache) -> OnTranscribeCompleted {
    Arc::new(move |page, page_bytes, transcript| {
        for (old, comparison) in compare::with_cached(&cache, page_bytes, transcript) {
            let line = comparison_line(page, &transcript.model, old, &comparison);
            log::info!(target: "eval", "{line}");
        }
    })
}

fn comparison_line(page: &str, model: &str, old: Model, c: &Comparison) -> String {
    format!(
        "compare page={page:?} new={model} old={} lines={}/{} uncertain={}/{} similarity={:.3} \
         changed_chars={}",
        old.name(),
        c.lines.0,
        c.lines.1,
        c.uncertain.0,
        c.uncertain.1,
        c.similarity,
        c.changed_chars
    )
}

/// Something worth knowing about a transcript. `model` is the API's name for the model that made
/// it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A page got a transcript, from Claude or the cache.
    Transcribed {
        page: String,
        model: String,
        from_cache: bool,
        usage: Usage,
        /// How long Claude took, if it was sent.
        elapsed: Option<Duration>,
        lines: usize,
        uncertain: usize,
    },
    /// The user picked a reading for uncertain word `word`.
    Corrected {
        page: String,
        model: String,
        word: usize,
        from: String,
        to: String,
        via: Via,
    },
    /// The last uncertain word that could be corrected was.
    AllResolved {
        page: String,
        model: String,
        uncertain: usize,
        /// Not found in the text, so never correctable.
        unfound: usize,
    },
    /// The user edited a line by hand.
    LineEdited {
        page: String,
        model: String,
        line: usize,
        from: String,
        to: String,
    },
    /// The user dropped their corrections to a page, back to Claude's transcription.
    Reset { page: String, model: String },
}

/// How the user picked a reading for an uncertain word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// One of Claude's other readings, 0 being the first offered.
    Alternative(usize),
    /// Claude's first reading.
    Keep,
    /// One of the user's own.
    Custom,
    /// Took out the word's struck-through text, perhaps all of it.
    DeleteStruck,
}

impl Event {
    /// The page's new `transcript`, which took `elapsed` if it was sent.
    pub fn transcribed(page: &str, transcript: &Transcript, elapsed: Option<Duration>) -> Self {
        Self::Transcribed {
            page: page.to_owned(),
            model: transcript.model.clone(),
            from_cache: transcript.from_cache,
            usage: transcript.usage,
            elapsed,
            lines: transcript.lines.len(),
            uncertain: transcript.uncertain.len(),
        }
    }

    fn page(&self) -> &str {
        match self {
            Self::Transcribed { page, .. }
            | Self::Corrected { page, .. }
            | Self::AllResolved { page, .. }
            | Self::LineEdited { page, .. }
            | Self::Reset { page, .. } => page,
        }
    }
}

/// The events so far this session, and what transcribing cost.
#[derive(Default)]
pub struct EvalLog {
    events: Vec<Event>,
    /// Dollars spent and pages sent, by model.
    spent: HashMap<Model, (f64, usize)>,
}

impl EvalLog {
    /// Logs `event` and keeps it. Pages sent to Claude add to the session's cost.
    pub fn record(&mut self, event: Event) {
        log::info!(target: "eval", "{}", self.line(&event));
        if let Event::Transcribed {
            model,
            from_cache: false,
            usage,
            ..
        } = &event
            && let Some(model) = Model::from_id(model)
        {
            let spent = self.spent.entry(model).or_default();
            spent.0 += usage.cost(model);
            spent.1 += 1;
            log::info!(target: "eval", "{}", self.session_cost());
        }
        self.events.push(event);
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Dollars spent and pages sent with `model` this session.
    pub fn spent(&self, model: Model) -> (f64, usize) {
        self.spent.get(&model).copied().unwrap_or_default()
    }

    fn session_cost(&self) -> String {
        let mut line = "session cost".to_owned();
        for model in Model::ALL {
            let (dollars, pages) = self.spent(model);
            write!(line, " {}=${dollars:.3} ({pages} pages)", model.name()).unwrap();
        }
        line
    }

    /// `event` as a logline.
    fn line(&self, event: &Event) -> String {
        match event {
            Event::Transcribed {
                page,
                model,
                from_cache,
                usage,
                elapsed,
                lines,
                uncertain,
            } => {
                let cost = Model::from_id(model)
                    .map_or("unknown".to_owned(), |m| format!("${:.4}", usage.cost(m)));
                let secs = elapsed.map_or("-".to_owned(), |e| format!("{:.1}", e.as_secs_f64()));
                format!(
                    "transcribed page={page:?} model={model} from_cache={from_cache} \
                     input={} output={} thinking={} cost={cost} secs={secs} lines={lines} \
                     uncertain={uncertain}",
                    usage.input_tokens, usage.output_tokens, usage.thinking_tokens
                )
            }
            Event::Corrected {
                page,
                model,
                word,
                from,
                to,
                via,
            } => format!(
                "correction page={page:?} model={model} word={word} from={from:?} to={to:?} \
                 via={}",
                match via {
                    Via::Alternative(i) => format!("alternative#{i}"),
                    Via::Keep => "keep".to_owned(),
                    Via::Custom => "custom".to_owned(),
                    Via::DeleteStruck => "delete-struck".to_owned(),
                }
            ),
            Event::AllResolved {
                page,
                model,
                uncertain,
                unfound,
            } => {
                let t = self.tally(page);
                format!(
                    "corrections done page={page:?} model={model} uncertain={uncertain} \
                     kept={} alternative={} custom={} struck_deleted={} unfound={unfound} \
                     line_edits={}",
                    t.kept, t.alternative, t.custom, t.struck_deleted, t.line_edits
                )
            }
            Event::LineEdited {
                page,
                model,
                line,
                from,
                to,
            } => {
                format!("line edit page={page:?} model={model} line={line} from={from:?} to={to:?}")
            }
            Event::Reset { page, model } => format!("reset page={page:?} model={model}"),
        }
    }

    /// How `page`'s current transcript was corrected: each word's last reading, and line edits,
    /// since it was transcribed or reset.
    fn tally(&self, page: &str) -> Tally {
        let since = self
            .events
            .iter()
            .rposition(|e| {
                matches!(e, Event::Transcribed { page: p, .. } | Event::Reset { page: p, .. } if p == page)
            })
            .map_or(0, |i| i + 1);
        let mut words = HashMap::new();
        let mut tally = Tally::default();
        for event in self.events[since..].iter().filter(|e| e.page() == page) {
            match event {
                Event::Corrected { word, via, .. } => {
                    words.insert(*word, *via);
                }
                Event::LineEdited { .. } => tally.line_edits += 1,
                _ => {}
            }
        }
        for via in words.into_values() {
            match via {
                Via::Alternative(_) => tally.alternative += 1,
                Via::Keep => tally.kept += 1,
                Via::Custom => tally.custom += 1,
                Via::DeleteStruck => tally.struck_deleted += 1,
            }
        }
        tally
    }
}

#[derive(Default)]
struct Tally {
    kept: usize,
    alternative: usize,
    custom: usize,
    struck_deleted: usize,
    line_edits: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPUS: &str = "claude-opus-5-5";

    fn transcribed(page: &str, model: &str, from_cache: bool) -> Event {
        Event::Transcribed {
            page: page.to_owned(),
            model: model.to_owned(),
            from_cache,
            usage: Usage {
                input_tokens: 5_000,
                output_tokens: 2_000,
                thinking_tokens: 800,
            },
            elapsed: (!from_cache).then_some(Duration::from_millis(31_250)),
            lines: 24,
            uncertain: 7,
        }
    }

    fn corrected(word: usize, via: Via) -> Event {
        Event::Corrected {
            page: "p1".to_owned(),
            model: OPUS.to_owned(),
            word,
            from: "plumber".to_owned(),
            to: "plumbers".to_owned(),
            via,
        }
    }

    fn line_edited(page: &str) -> Event {
        Event::LineEdited {
            page: page.to_owned(),
            model: OPUS.to_owned(),
            line: 5,
            from: "call Sam".to_owned(),
            to: "call Sal".to_owned(),
        }
    }

    fn all_resolved() -> Event {
        Event::AllResolved {
            page: "p1".to_owned(),
            model: OPUS.to_owned(),
            uncertain: 4,
            unfound: 1,
        }
    }

    #[test]
    fn loglines() {
        let log = EvalLog::default();
        assert_eq!(
            log.line(&transcribed("p1", "claude-sonnet-5-5", false)),
            "transcribed page=\"p1\" model=claude-sonnet-5-5 from_cache=false input=5000 \
             output=2000 thinking=800 cost=$0.0300 secs=31.2 lines=24 uncertain=7"
        );
        assert_eq!(
            log.line(&transcribed("p1", "claude-haiku-5-5", true)),
            "transcribed page=\"p1\" model=claude-haiku-5-5 from_cache=true input=5000 \
             output=2000 thinking=800 cost=unknown secs=- lines=24 uncertain=7"
        );
        let comparison = Comparison {
            lines: (24, 25),
            uncertain: (7, 9),
            similarity: 0.96213,
            changed_chars: 37,
        };
        assert_eq!(
            comparison_line("p1", "claude-sonnet-5-5", Model::Opus, &comparison),
            "compare page=\"p1\" new=claude-sonnet-5-5 old=opus lines=24/25 uncertain=7/9 \
             similarity=0.962 changed_chars=37"
        );
        assert_eq!(
            log.line(&corrected(3, Via::Alternative(1))),
            "correction page=\"p1\" model=claude-opus-5-5 word=3 from=\"plumber\" \
             to=\"plumbers\" via=alternative#1"
        );
        assert_eq!(
            log.line(&line_edited("p1")),
            "line edit page=\"p1\" model=claude-opus-5-5 line=5 from=\"call Sam\" \
             to=\"call Sal\""
        );
    }

    #[test]
    fn session_cost_is_split_by_model_and_skips_the_cache() {
        let mut log = EvalLog::default();
        log.record(transcribed("p1", OPUS, true));
        log.record(transcribed("p1", OPUS, false));
        log.record(transcribed("p2", "claude-sonnet-5-5", false));
        log.record(transcribed("p3", "claude-sonnet-5", false));
        let (opus, opus_pages) = log.spent(Model::Opus);
        assert!((opus - 0.06).abs() < 1e-9 && opus_pages == 1);
        let (sonnet, sonnet_pages) = log.spent(Model::Sonnet);
        assert!(
            (sonnet - 0.06).abs() < 1e-9 && sonnet_pages == 2,
            "fallback counts"
        );
        assert_eq!(
            log.session_cost(),
            "session cost opus=$0.060 (1 pages) sonnet=$0.060 (2 pages)"
        );
        assert_eq!(log.events().len(), 4);
    }

    #[test]
    fn done_tallies_each_words_last_reading_since_transcribed() {
        let mut log = EvalLog::default();
        log.record(transcribed("p1", OPUS, true));
        // Lost to the forced retranscription below.
        log.record(corrected(0, Via::Custom));
        log.record(line_edited("p1"));
        log.record(transcribed("p1", OPUS, false));
        log.record(corrected(0, Via::Custom));
        log.record(corrected(0, Via::Alternative(0)));
        log.record(corrected(1, Via::Keep));
        log.record(corrected(2, Via::Custom));
        log.record(line_edited("p1"));
        log.record(line_edited("another page"));
        assert_eq!(
            log.line(&all_resolved()),
            "corrections done page=\"p1\" model=claude-opus-5-5 uncertain=4 kept=1 \
             alternative=1 custom=1 struck_deleted=0 unfound=1 line_edits=1"
        );
    }

    #[test]
    fn done_tallies_since_the_last_reset() {
        let mut log = EvalLog::default();
        log.record(transcribed("p1", OPUS, false));
        log.record(corrected(0, Via::Custom));
        log.record(line_edited("p1"));
        let reset = Event::Reset {
            page: "p1".to_owned(),
            model: OPUS.to_owned(),
        };
        assert_eq!(log.line(&reset), "reset page=\"p1\" model=claude-opus-5-5");
        log.record(reset);
        log.record(corrected(0, Via::DeleteStruck));
        assert_eq!(
            log.line(&all_resolved()),
            "corrections done page=\"p1\" model=claude-opus-5-5 uncertain=4 kept=0 \
             alternative=0 custom=0 struck_deleted=1 unfound=1 line_edits=0"
        );
    }
}
