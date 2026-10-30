//! Transcribes pages in the background, so the UI keeps running during the minute or so Claude
//! takes. Pages are looked up in the cache as they arrive; at most [`MAX_CONCURRENT`] are sent to
//! Claude at once, the rest wait in a queue. Finished transcripts come out of
//! [`Transcriptions::next`]; failures are kept by page name until retried.
//!
//! Pages are sent to the [`Model`] selected at the time. A page cached by any model isn't sent
//! unless forced.
//!
//! See `decisions/2026-10-24-confirm-before-transcribing.md`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notebook_core::cache::Cache;
use notebook_core::llm_model::Model;
use notebook_core::ocr::{Fetched, OcrError, parse_response};
use notebook_core::page::Page;
use notebook_core::transcript::Transcript;
use notebook_core::{CleanParams, Quad};

use crate::jobs::Jobs;

/// Most pages sent to Claude at once. More risks rate limits and overloaded errors.
pub const MAX_CONCURRENT: usize = 4;

/// Sends a page to a Claude model from its file's bytes, cropped to the user's corners if given.
pub type Fetch = Arc<
    dyn Fn(&[u8], &CleanParams, Option<Quad>, Model) -> Result<Fetched, OcrError> + Send + Sync,
>;

/// Told a page's name, its file's bytes and its transcript whenever a page is sent to Claude and
/// comes back transcribed, but not for cached transcripts. Runs on the thread that sent it.
pub type OnTranscribeCompleted = Arc<dyn Fn(&str, &[u8], &Transcript) + Send + Sync>;

/// Wakes the UI, from the thread a page finished on, so it picks the result up.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

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
        let (bytes, model) = (&*job.bytes, job.model);
        let mut elapsed = None;
        let mut fetch = || {
            let started = Instant::now();
            let fetched = (self.fetch)(bytes, &job.params, job.corners, model);
            elapsed = Some(started.elapsed());
            fetched
        };
        let transcript = match cache {
            Some(cache) if job.kind == Kind::Force => cache.refresh(bytes, model, fetch)?,
            Some(cache) => cache.transcript_or(bytes, model, fetch)?,
            None => {
                let Fetched { status, body, crop } = fetch()?;
                parse_response(status, &body, crop)?
            }
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
    model: Model,
}

/// A page that finished transcribing.
pub struct Done {
    pub name: String,
    pub transcript: Transcript,
    /// Retranscribed on request, so it replaces the page's transcript, corrections and all.
    pub forced: bool,
    /// How long Claude took, if the page was sent rather than found in the cache.
    pub elapsed: Option<Duration>,
}

/// A job's transcript: `None` for a [`Kind::Lookup`] that missed.
type Finished = Result<Option<Ran>, String>;

/// A job's page name and what it does.
type Key = (String, Kind);

pub struct Transcriptions {
    /// Sends pages to Claude. Without one, nothing is sent.
    transcriber: Option<Transcriber>,
    /// Without one, nothing is looked up.
    cache: Option<Cache>,
    /// By page name and what they do.
    jobs: Jobs<Key, Finished>,
    /// Pages waiting to be sent, next first.
    queue: VecDeque<Job>,
    /// Pages being sent, by name.
    running: HashSet<String>,
    /// Pages being looked up in the cache, by name.
    checking: HashSet<String>,
    /// Why the last attempt failed, by page name.
    errors: HashMap<String, String>,
    /// Pages are sent to it from now on.
    model: Model,
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
            model: Model::default(),
        }
    }

    /// Whether [`Self::set_waker`] has been called.
    pub fn has_waker(&self) -> bool {
        self.jobs.has_waker()
    }

    /// Calls `waker` whenever a page finishes, from the thread it ran on.
    pub fn set_waker(&mut self, waker: Waker) {
        self.jobs.set_waker(waker);
    }

    /// Which model pages are sent to, and preferred in the cache.
    pub fn model(&self) -> Model {
        self.model
    }

    /// Sends pages to `model` from now on. Pages already queued or transcribed are unaffected.
    pub fn set_model(&mut self, model: Model) {
        self.model = model;
    }

    /// Whether pages can be sent to Claude.
    pub fn available(&self) -> bool {
        self.transcriber.is_some()
    }

    /// Looks the page named `name` up in the cache, which can be done straight away: nothing is
    /// sent. A hit comes out of [`Self::next`].
    pub fn look_up(&mut self, name: &str, bytes: Arc<[u8]>) {
        let Some(cache) = self.cache.clone() else {
            return;
        };
        self.checking.insert(name.to_owned());
        let model = self.model;
        self.spawn((name.to_owned(), Kind::Lookup), move || {
            Ok(cache.find(&bytes, model).map(|transcript| Ran {
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
    pub fn force(&mut self, page: &Page, params: CleanParams) {
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
            model: self.model,
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
            log::info!("{}: transcribing with {}", job.name, job.model.id());
            self.running.insert(job.name.clone());
            let (transcriber, cache) = (transcriber.clone(), self.cache.clone());
            self.spawn((job.name.clone(), job.kind), move || {
                transcriber.run(&job, cache.as_ref()).map(Some)
            });
        }
    }

    /// Runs `work` for the job `key` on a thread of its own, for [`Self::next`] to collect.
    fn spawn(
        &mut self,
        key: Key,
        work: impl FnOnce() -> Result<Option<Ran>, OcrError> + Send + 'static,
    ) {
        self.jobs
            .spawn(key, move || work().map_err(|e| e.to_string()));
    }

    /// The next page that finished transcribing, if any, sending the next queued page in its
    /// place. Failures are kept for [`Self::error`]; cache misses are dropped.
    pub fn next(&mut self) -> Option<Done> {
        while let Some(((name, kind), result)) = self.jobs.next() {
            if kind == Kind::Lookup {
                self.checking.remove(&name);
            } else {
                self.running.remove(&name);
                self.send_queued();
            }
            match result {
                Ok(None) => {}
                Ok(Some(ran)) => {
                    return Some(Done {
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
