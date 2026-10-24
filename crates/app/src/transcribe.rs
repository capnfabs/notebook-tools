//! Transcribes pages in the background, a thread per page, so the UI keeps running during the
//! minute or so Claude takes. Finished transcripts come out of [`Transcriptions::next`]; failures
//! are kept by page name until retried.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use notebook_core::CleanParams;
use notebook_core::ocr::Transcript;

/// Transcribes a page from its file's bytes.
pub type Transcriber = Arc<dyn Fn(&[u8], &CleanParams) -> Result<Transcript, String> + Send + Sync>;

type Finished = (String, Result<Transcript, String>);

pub struct Transcriptions {
    transcriber: Option<Transcriber>,
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
    /// Pages being transcribed, by name.
    in_flight: HashSet<String>,
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
            in_flight: HashSet::new(),
            errors: HashMap::new(),
        }
    }

    pub fn available(&self) -> bool {
        self.transcriber.is_some()
    }

    /// Starts transcribing the page named `name`, forgetting any earlier failure.
    pub fn start(&mut self, name: &str, bytes: Vec<u8>, params: CleanParams, ctx: &egui::Context) {
        let Some(transcriber) = self.transcriber.clone() else {
            return;
        };
        if !self.in_flight.insert(name.to_owned()) {
            return;
        }
        self.errors.remove(name);
        log::info!("{name}: transcribing");
        let (name, tx, ctx) = (name.to_owned(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = transcriber(&bytes, &params);
            // The receiver lives as long as the app, so a failed send only happens during shutdown.
            tx.send((name, result)).ok();
            ctx.request_repaint();
        });
    }

    /// The next page that finished transcribing, if any. Failures are kept for [`Self::error`].
    pub fn next(&mut self) -> Option<(String, Transcript)> {
        while let Ok((name, result)) = self.rx.try_recv() {
            self.in_flight.remove(&name);
            match result {
                Ok(transcript) if transcript.from_cache => {
                    log::info!("{name}: from cache, transcribed by {}", transcript.model);
                    return Some((name, transcript));
                }
                Ok(transcript) => {
                    let usage = transcript.usage;
                    log::info!(
                        "{name}: transcribed by {}, {} input + {} output tokens",
                        transcript.model,
                        usage.input_tokens,
                        usage.output_tokens
                    );
                    return Some((name, transcript));
                }
                Err(err) => {
                    log::warn!("{name}: transcription failed: {err}");
                    self.errors.insert(name, err);
                }
            }
        }
        None
    }

    pub fn in_flight(&self, name: &str) -> bool {
        self.in_flight.contains(name)
    }

    /// Forgets why the page named `name` failed, as when it's deleted.
    pub fn forget(&mut self, name: &str) {
        self.errors.remove(name);
    }

    pub fn error(&self, name: &str) -> Option<&str> {
        self.errors.get(name).map(String::as_str)
    }
}
