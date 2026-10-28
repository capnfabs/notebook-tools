//! Reads files dropped onto the window, and loads each one ([`Loaded::new`]) off the UI thread:
//! decoding and cleaning a big photo takes a while. Finished loads come out of [`Ingest::next`].

use std::sync::Arc;

use eframe::egui;
use notebook_core::{CleanParams, DecodeError, Loaded};

use crate::jobs::Jobs;
use crate::transcribe::Waker;

/// A dropped file's name and contents, or why it couldn't be read.
pub type Received = (String, Result<Vec<u8>, String>);

/// Loads a page from its file's bytes. Tests stand in for [`Loaded::new`] to hold loads up.
pub type Load = Arc<dyn Fn(&[u8], &CleanParams) -> Result<Loaded, DecodeError> + Send + Sync>;

/// A page's name and bytes, and what loading them made.
pub type Finished = (String, Arc<[u8]>, Result<Loaded, String>);

/// A page's name and bytes, which tell it from a later page of the same name.
type Key = (String, Arc<[u8]>);

pub struct Ingest {
    load: Load,
    jobs: Jobs<Key, Result<Loaded, String>>,
}

impl Default for Ingest {
    fn default() -> Self {
        Self::new(Arc::new(Loaded::new))
    }
}

impl Ingest {
    pub fn new(load: Load) -> Self {
        Self {
            load,
            jobs: Jobs::on_pool(),
        }
    }

    /// Calls `waker` whenever a page finishes loading.
    pub fn set_waker(&mut self, waker: Waker) {
        self.jobs.set_waker(waker);
    }

    /// Files dropped this frame, in filename order: once the pages have been rearranged, new ones
    /// go at the end in the order they're added.
    pub fn read_drops(&self, ctx: &egui::Context) -> Vec<Received> {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let mut received: Vec<Received> = dropped
            .iter()
            .map(|file| (file_name(&**file), file.bytes()))
            .collect();
        received.sort_by(|a, b| a.0.cmp(&b.0));
        received
    }

    /// Starts loading the page named `name`.
    pub fn load(&mut self, name: &str, bytes: Arc<[u8]>, params: CleanParams) {
        let load = self.load.clone();
        let key = (name.to_owned(), bytes.clone());
        self.jobs.spawn(key, move || {
            load(&bytes, &params).map_err(|e| e.to_string())
        });
    }

    /// The next page that finished loading, or failed to, if any.
    pub fn next(&mut self) -> Option<Finished> {
        let ((name, bytes), loaded) = self.jobs.next()?;
        Some((name, bytes, loaded))
    }

    /// How many pages are loading.
    pub fn pending(&self) -> usize {
        self.jobs.running()
    }
}

fn file_name(file: &dyn egui::DroppedFile) -> String {
    let path = file.path();
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
