//! Reads files dropped onto the window into `(name, bytes)` pairs.
//!
//! Native reads are synchronous; browsers can only read files asynchronously, so on the web each
//! read is spawned and its result arrives on a later frame. Either way results come out of
//! [`Ingest::next`].

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

/// A dropped file's name and contents, or why it couldn't be read.
pub type Received = (String, Result<Vec<u8>, String>);

pub struct Ingest {
    tx: Sender<Received>,
    rx: Receiver<Received>,
    /// Files dropped but not yet returned by [`Self::next`].
    pending: usize,
}

impl Default for Ingest {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, pending: 0 }
    }
}

impl Ingest {
    /// Starts reading any files dropped this frame, returning how many there were.
    pub fn start_reading_drops(&mut self, ctx: &egui::Context) -> usize {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in &dropped {
            read(file.clone(), self.tx.clone(), ctx.clone());
        }
        self.pending += dropped.len();
        dropped.len()
    }

    /// The next file that has finished reading, if any.
    pub fn next(&mut self) -> Option<Received> {
        let received = self.rx.try_recv().ok()?;
        self.pending -= 1;
        Some(received)
    }

    pub fn pending(&self) -> usize {
        self.pending
    }
}

fn file_name(file: &dyn egui::DroppedFile) -> String {
    let path = file.path();
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn read(file: egui::DroppedFileHandle, tx: Sender<Received>, _ctx: egui::Context) {
    // The receiver lives as long as the app, so a failed send only happens during shutdown.
    tx.send((file_name(&*file), file.bytes())).ok();
}

#[cfg(target_arch = "wasm32")]
fn read(file: egui::DroppedFileHandle, tx: Sender<Received>, ctx: egui::Context) {
    wasm_bindgen_futures::spawn_local(async move {
        let bytes = file.bytes_async().await;
        tx.send((file_name(&*file), bytes)).ok();
        ctx.request_repaint();
    });
}
