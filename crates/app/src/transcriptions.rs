//! Transcribes pages in the background, so the UI keeps running during the minute or so Claude
//! takes. Pages are looked up in the cache as they arrive; at most [`MAX_CONCURRENT`] are sent to
//! Claude at once, the rest wait in a queue. Finished transcripts come out of
//! [`Transcriptions::next`]; failures are kept by page name until retried.
//!
//! Pages are sent to the [`Model`] selected at the time, and parsed in the
//! [`notebook_core::markdown_style::MarkdownStyle`]
//! selected then. A page cached by any model isn't sent unless forced.
//!
//! See `decisions/2026-10-24-confirm-before-transcribing.md` and
//! `decisions/2026-10-30-settings-dialog.md`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notebook_core::cache::Cache;
use notebook_core::claude_transcribe::{Fetched, OcrError};
use notebook_core::clean::CleanParams;
use notebook_core::geom::Quad;
use notebook_core::llm_model::Model;
use notebook_core::page::Page;
use notebook_core::settings::Settings;
use notebook_core::transcript::Transcript;

use crate::jobs::{Jobs, Waker};

/// Most pages sent to Claude at once. More risks rate limits and overloaded errors.
pub const MAX_CONCURRENT: usize = 4;

/// Sends a page to a Claude model from its file's bytes, cropped to the user's corners if given.
pub type Fetch = Arc<
    dyn Fn(&[u8], &CleanParams, Option<Quad>, Model) -> Result<Fetched, OcrError> + Send + Sync,
>;

/// Told a page's name, its file's bytes and its transcript whenever a page is sent to Claude and
/// comes back transcribed, but not for cached transcripts. Runs on the thread that sent it.
pub type OnTranscribeCompleted = Arc<dyn Fn(&str, &[u8], &Transcript) + Send + Sync>;

/// Transcribes pages with Claude.
#[derive(Clone)]
pub struct Transcriber {
    fetch: Fetch,
    on_transcribe_completed: Option<OnTranscribeCompleted>,
}

impl Transcriber {
    pub fn new(fetch: Fetch) -> Self {
        Self {
            fetch,
            on_transcribe_completed: None,
        }
    }

    /// Calls `on_transcribe_completed` for each page Claude transcribes, but not for cache hits.
    pub fn on_transcribe_completed(self, on_transcribe_completed: OnTranscribeCompleted) -> Self {
        Self {
            on_transcribe_completed: Some(on_transcribe_completed),
            ..self
        }
    }

    /// The page's transcript for `job`, through `cache` if there is one. Without a cache, every
    /// page is sent to Claude.
    fn run(&self, job: &Job, cache: Option<&Cache>) -> Result<Ran, OcrError> {
        let (bytes, model, style) = (&*job.bytes, job.settings.model, &job.settings.markdown);
        let mut elapsed = None;
        let mut fetch = || {
            let started = Instant::now();
            let fetched = (self.fetch)(bytes, &job.params, job.corners, model);
            elapsed = Some(started.elapsed());
            fetched
        };
        let transcript = match cache {
            Some(cache) if job.kind == Kind::Force => cache.refresh(bytes, model, style, fetch)?,
            Some(cache) => cache.transcript_or(bytes, model, style, fetch)?,
            None => fetch()?.parse(style)?,
        };
        if let Some(on_transcribe_completed) = &self.on_transcribe_completed
            && !transcript.from_cache
        {
            on_transcribe_completed(&job.name, bytes, &transcript);
        }
        Ok(Ran {
            transcript,
            elapsed,
        })
    }
}

/// What a page's job made of it.
struct Ran {
    transcript: Transcript,
    elapsed: Option<Duration>,
}

/// What a job does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Looks the page up in the cache, without sending it.
    Lookup,
    /// Uses the cache, else sends the page.
    Transcribe,
    /// Sends the page even if it's cached, replacing the entry.
    Force,
}

/// A page waiting to be, or being, sent to Claude.
struct Job {
    name: String,
    bytes: Arc<[u8]>,
    params: CleanParams,
    /// Where the user put the page's corners, if they did.
    corners: Option<Quad>,
    kind: Kind,
    settings: Settings,
}

/// A page that finished transcribing.
pub struct TranscriptReady {
    pub name: String,
    pub transcript: Transcript,
    /// Retranscribed on request, so it replaces the page's transcript, corrections and all.
    pub forced: bool,
    /// How long Claude took, if the page was sent rather than found in the cache.
    pub elapsed: Option<Duration>,
}

/// A job's transcript: `None` for a [`Kind::Lookup`] that missed.
type Outcome = Result<Option<Ran>, String>;

/// A job's page name and what it does.
type JobKey = (String, Kind);

pub struct Transcriptions {
    /// Sends pages to Claude. Without one, nothing is sent.
    transcriber: Option<Transcriber>,
    /// Without one, or while the settings switch caching off, nothing is looked up or stored.
    cache: Option<Cache>,
    /// By page name and what they do.
    jobs: Jobs<JobKey, Outcome>,
    /// Pages waiting to be sent, next first.
    queue: VecDeque<Job>,
    /// Pages being sent, by name.
    running: HashSet<String>,
    /// Pages being looked up in the cache, by name.
    checking: HashSet<String>,
    /// Why the last attempt failed, by page name.
    errors: HashMap<String, String>,
    /// Pages are sent to its model, and parsed in its style, from now on.
    settings: Settings,
}

impl Default for Transcriptions {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl Transcriptions {
    pub fn new(transcriber: Option<Transcriber>, cache: Option<Cache>) -> Self {
        Self {
            transcriber,
            cache,
            jobs: Jobs::on_own_threads(),
            queue: VecDeque::new(),
            running: HashSet::new(),
            checking: HashSet::new(),
            errors: HashMap::new(),
            settings: Settings::default(),
        }
    }

    /// Transcriptions with the same transcriber, cache and settings, and nothing else: for another
    /// project. Jobs started here still finish, but nothing collects them.
    pub fn emptied(&self) -> Self {
        Self {
            settings: self.settings.clone(),
            ..Self::new(self.transcriber.clone(), self.cache.clone())
        }
    }

    /// Calls `waker` whenever a page finishes, from the thread it ran on.
    pub fn set_waker(&mut self, waker: Waker) {
        self.jobs.set_waker(waker);
    }

    /// Which model pages are sent to, and preferred in the cache, and the style they're parsed
    /// in.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Sends and parses pages with `settings` from now on. Pages already queued or transcribed are
    /// unaffected: see [`Transcript::restyled`].
    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// Sends pages with `transcriber` from now on, or stops sending them. Pages already being sent
    /// carry on with the one they started with.
    pub fn set_transcriber(&mut self, transcriber: Option<Transcriber>) {
        self.transcriber = transcriber;
    }

    /// Whether pages can be sent to Claude.
    pub fn can_send(&self) -> bool {
        self.transcriber.is_some()
    }

    /// Whether pages can have transcripts: sent to Claude, or found in the cache.
    pub fn can_show(&self) -> bool {
        self.transcriber.is_some() || self.active_cache().is_some()
    }

    /// The system cache, whether or not the settings use it.
    pub fn cache(&self) -> Option<&Cache> {
        self.cache.as_ref()
    }

    /// The system cache, unless the settings switch caching off: then pages are neither looked up
    /// nor stored. See `decisions/2026-11-01-manual-cache-clearing.md`.
    fn active_cache(&self) -> Option<&Cache> {
        self.cache
            .as_ref()
            .filter(|_| self.settings.cache_transcripts)
    }

    /// Looks the page named `name` up in the cache, which can be done straight away: nothing is
    /// sent. A hit comes out of [`Self::next`].
    pub fn look_up(&mut self, name: &str, bytes: Arc<[u8]>) {
        let Some(cache) = self.active_cache().cloned() else {
            return;
        };
        self.checking.insert(name.to_owned());
        let (model, style) = (self.settings.model, self.settings.markdown.clone());
        let key = (name.to_owned(), Kind::Lookup);
        self.spawn(key, move || {
            Ok(cache.find(&bytes, model, &style).map(|transcript| Ran {
                transcript,
                elapsed: None,
            }))
        });
    }

    /// Transcribes `page` before any others waiting, forgetting any earlier failure.
    pub fn start(&mut self, page: &Page, params: CleanParams) {
        self.enqueue(page, params, Kind::Transcribe, true);
    }

    /// Transcribes `page` after any others waiting.
    pub fn start_later(&mut self, page: &Page, params: CleanParams) {
        self.enqueue(page, params, Kind::Transcribe, false);
    }

    /// Sends `page` to Claude even if it's cached, before any others waiting. The cached entry is
    /// replaced only if this succeeds.
    pub fn retranscribe(&mut self, page: &Page, params: CleanParams) {
        self.enqueue(page, params, Kind::Force, true);
    }

    /// Queues `page`, cropped to its corners as they are now.
    fn enqueue(&mut self, page: &Page, params: CleanParams, kind: Kind, first: bool) {
        let name = &page.name;
        if self.transcriber.is_none() || self.queued_or_running(name) {
            return;
        }
        self.errors.remove(name);
        let job = Job {
            name: name.clone(),
            bytes: page.bytes.clone(),
            params,
            corners: page.corners,
            kind,
            settings: self.settings.clone(),
        };
        if first {
            self.queue.push_front(job);
        } else {
            self.queue.push_back(job);
        }
        self.send_queued();
    }

    /// Starts queued pages while fewer than [`MAX_CONCURRENT`] are being sent.
    fn send_queued(&mut self) {
        let Some(transcriber) = self.transcriber.clone() else {
            return;
        };
        while self.running.len() < MAX_CONCURRENT
            && let Some(job) = self.queue.pop_front()
        {
            log::info!(
                "{}: transcribing with {}",
                job.name,
                job.settings.model.id()
            );
            self.running.insert(job.name.clone());
            // The cache as the settings are now, not as they were when the page was queued.
            let (transcriber, cache) = (transcriber.clone(), self.active_cache().cloned());
            let key = (job.name.clone(), job.kind);
            self.spawn(key, move || transcriber.run(&job, cache.as_ref()).map(Some));
        }
    }

    /// Runs `work` for the job `key` on a thread of its own, for [`Self::next`] to collect.
    fn spawn(
        &mut self,
        key: JobKey,
        work: impl FnOnce() -> Result<Option<Ran>, OcrError> + Send + 'static,
    ) {
        self.jobs
            .spawn(key, move || work().map_err(|e| crate::i18n::ocr_error(&e)));
    }

    /// The next page that finished transcribing, if any, sending the next queued page in its
    /// place. Failures are kept for [`Self::error`]; cache misses are dropped.
    pub fn next(&mut self) -> Option<TranscriptReady> {
        while let Some(((name, kind), result)) = self.jobs.next() {
            match kind {
                Kind::Lookup => {
                    self.checking.remove(&name);
                }
                Kind::Transcribe | Kind::Force => {
                    self.running.remove(&name);
                    self.send_queued();
                }
            }
            match result {
                Ok(None) => {}
                Ok(Some(ran)) => {
                    return Some(TranscriptReady {
                        name,
                        transcript: ran.transcript,
                        forced: kind == Kind::Force,
                        elapsed: ran.elapsed,
                    });
                }
                Err(err) => {
                    log::warn!("{name}: transcription failed: {err}");
                    self.errors.insert(name, err);
                }
            }
        }
        None
    }

    /// Whether the page named `name` is waiting to be sent, or being sent.
    pub fn queued_or_running(&self, name: &str) -> bool {
        self.running.contains(name) || self.queue.iter().any(|j| j.name == name)
    }

    /// Whether the page named `name` is being looked up in the cache.
    pub fn checking(&self, name: &str) -> bool {
        self.checking.contains(name)
    }

    /// How many pages are waiting to be sent or being sent.
    pub fn pending(&self) -> usize {
        self.queue.len() + self.running.len()
    }

    /// Forgets the page named `name`, as when it's deleted: why it failed, and its place in the
    /// queue. One already being sent finishes, and is ignored.
    pub fn forget(&mut self, name: &str) {
        self.errors.remove(name);
        self.queue.retain(|j| j.name != name);
    }

    pub fn error(&self, name: &str) -> Option<&str> {
        self.errors.get(name).map(String::as_str)
    }
}
