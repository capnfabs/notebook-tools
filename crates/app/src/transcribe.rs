//! Transcribes pages in the background, so the UI keeps running during the minute or so Claude
//! takes. Pages are looked up in the cache as they arrive; at most [`MAX_CONCURRENT`] are sent to
//! Claude at once, the rest wait in a queue. Finished transcripts come out of
//! [`Transcriptions::next`]; failures are kept by page name until retried.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use notebook_core::CleanParams;
use notebook_core::cache::{Cache, Fetched};
use notebook_core::ocr::{OcrError, Transcript, parse_response};

/// Most pages sent to Claude at once. More risks rate limits and overloaded errors.
pub const MAX_CONCURRENT: usize = 4;

/// Sends a page to Claude from its file's bytes.
pub type Fetch = Arc<dyn Fn(&[u8], &CleanParams) -> Result<Fetched, OcrError> + Send + Sync>;

/// Transcribes pages with Claude, caching what comes back.
#[derive(Clone)]
pub struct Transcriber {
    fetch: Fetch,
    cache: Option<Arc<Cache>>,
}

impl Transcriber {
    /// Without a `cache`, every page is sent to Claude.
    pub fn new(fetch: Fetch, cache: Option<Cache>) -> Self {
        Self {
            fetch,
            cache: cache.map(Arc::new),
        }
    }

    /// The page's transcript for `kind`. `None` only for a [`Kind::Lookup`] that missed.
    fn run(
        &self,
        kind: Kind,
        bytes: &[u8],
        params: &CleanParams,
    ) -> Result<Option<Transcript>, OcrError> {
        let fetch = || (self.fetch)(bytes, params);
        let transcript = match (kind, &self.cache) {
            (Kind::Lookup, cache) => return Ok(cache.as_ref().and_then(|c| c.get(bytes))),
            (Kind::Transcribe, Some(cache)) => cache.transcript_or(bytes, fetch)?,
            (Kind::Force, Some(cache)) => cache.refresh(bytes, fetch)?,
            (_, None) => {
                let (status, body, size) = fetch()?;
                parse_response(status, &body, size)?
            }
        };
        Ok(Some(transcript))
    }
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

struct Job {
    name: String,
    bytes: Vec<u8>,
    params: CleanParams,
    kind: Kind,
}

/// A page that finished transcribing.
pub struct Done {
    pub name: String,
    pub transcript: Transcript,
    /// Retranscribed on request, so it replaces the page's transcript, corrections and all.
    pub forced: bool,
}

type Finished = (String, Kind, Result<Option<Transcript>, String>);

pub struct Transcriptions {
    transcriber: Option<Transcriber>,
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
    /// Pages waiting to be sent, next first.
    queue: VecDeque<Job>,
    /// Pages being sent, by name.
    running: HashSet<String>,
    /// Pages being looked up in the cache, by name.
    checking: HashSet<String>,
    /// Why the last attempt failed, by page name.
    errors: HashMap<String, String>,
}

impl Default for Transcriptions {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Transcriptions {
    pub fn new(transcriber: Option<Transcriber>) -> Self {
        let (tx, rx) = channel();
        Self {
            transcriber,
            tx,
            rx,
            queue: VecDeque::new(),
            running: HashSet::new(),
            checking: HashSet::new(),
            errors: HashMap::new(),
        }
    }

    pub fn available(&self) -> bool {
        self.transcriber.is_some()
    }

    /// Looks the page named `name` up in the cache, which can be done straight away: nothing is
    /// sent. A hit comes out of [`Self::next`].
    pub fn look_up(&mut self, name: &str, bytes: Vec<u8>, ctx: &egui::Context) {
        if self.transcriber.as_ref().is_none_or(|t| t.cache.is_none()) {
            return;
        }
        self.checking.insert(name.to_owned());
        self.spawn(
            Job {
                name: name.to_owned(),
                bytes,
                params: CleanParams::default(),
                kind: Kind::Lookup,
            },
            ctx,
        );
    }

    /// Transcribes the page named `name` before any others waiting, forgetting any earlier
    /// failure.
    pub fn start(&mut self, name: &str, bytes: Vec<u8>, params: CleanParams, ctx: &egui::Context) {
        self.enqueue(name, bytes, params, Kind::Transcribe, true, ctx);
    }

    /// Transcribes the page named `name` after any others waiting.
    pub fn start_later(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        params: CleanParams,
        ctx: &egui::Context,
    ) {
        self.enqueue(name, bytes, params, Kind::Transcribe, false, ctx);
    }

    /// Sends the page named `name` to Claude even if it's cached, before any others waiting. The
    /// cached entry is replaced only if this succeeds.
    pub fn force(&mut self, name: &str, bytes: Vec<u8>, params: CleanParams, ctx: &egui::Context) {
        self.enqueue(name, bytes, params, Kind::Force, true, ctx);
    }

    fn enqueue(
        &mut self,
        name: &str,
        bytes: Vec<u8>,
        params: CleanParams,
        kind: Kind,
        first: bool,
        ctx: &egui::Context,
    ) {
        if self.transcriber.is_none() || self.queued_or_running(name) {
            return;
        }
        self.errors.remove(name);
        let job = Job {
            name: name.to_owned(),
            bytes,
            params,
            kind,
        };
        if first {
            self.queue.push_front(job);
        } else {
            self.queue.push_back(job);
        }
        self.send_queued(ctx);
    }

    /// Starts queued pages while fewer than [`MAX_CONCURRENT`] are being sent.
    fn send_queued(&mut self, ctx: &egui::Context) {
        while self.running.len() < MAX_CONCURRENT
            && let Some(job) = self.queue.pop_front()
        {
            log::info!("{}: transcribing", job.name);
            self.running.insert(job.name.clone());
            self.spawn(job, ctx);
        }
    }

    fn spawn(&self, job: Job, ctx: &egui::Context) {
        let Some(transcriber) = self.transcriber.clone() else {
            return;
        };
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = transcriber
                .run(job.kind, &job.bytes, &job.params)
                .map_err(|e| e.to_string());
            // The receiver lives as long as the app, so a failed send only happens during shutdown.
            tx.send((job.name, job.kind, result)).ok();
            ctx.request_repaint();
        });
    }

    /// The next page that finished transcribing, if any, sending the next queued page in its
    /// place. Failures are kept for [`Self::error`]; cache misses are dropped.
    pub fn next(&mut self, ctx: &egui::Context) -> Option<Done> {
        while let Ok((name, kind, result)) = self.rx.try_recv() {
            if kind == Kind::Lookup {
                self.checking.remove(&name);
            } else {
                self.running.remove(&name);
                self.send_queued(ctx);
            }
            match result {
                Ok(None) => {}
                Ok(Some(transcript)) => {
                    log_done(&name, &transcript);
                    return Some(Done {
                        name,
                        transcript,
                        forced: kind == Kind::Force,
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

fn log_done(name: &str, transcript: &Transcript) {
    if transcript.from_cache {
        log::info!("{name}: from cache, transcribed by {}", transcript.model);
    } else {
        let usage = transcript.usage;
        log::info!(
            "{name}: transcribed by {}, {} input + {} output tokens",
            transcript.model,
            usage.input_tokens,
            usage.output_tokens
        );
    }
}
