//! Reads files dropped onto the window into `(name, bytes)` pairs, queued for [`Ingest::next`].

use std::collections::VecDeque;

use eframe::egui;

/// A dropped file's name and contents, or why it couldn't be read.
pub type Received = (String, Result<Vec<u8>, String>);

#[derive(Default)]
pub struct Ingest {
    /// Files read but not yet returned by [`Self::next`].
    queue: VecDeque<Received>,
}

impl Ingest {
    /// Reads any files dropped this frame, returning how many there were.
    pub fn read_drops(&mut self, ctx: &egui::Context) -> usize {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        self.queue.extend(
            dropped
                .iter()
                .map(|file| (file_name(&**file), file.bytes())),
        );
        dropped.len()
    }

    /// The next file that has been read, if any.
    pub fn next(&mut self) -> Option<Received> {
        self.queue.pop_front()
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }
}

fn file_name(file: &dyn egui::DroppedFile) -> String {
    let path = file.path();
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
