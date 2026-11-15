//! Reads files dropped onto the window or picked with Add Pages…, and loads each one ([`Loaded::new`]) off the UI thread:
//! decoding and cleaning a big photo takes a while. Finished loads come out of [`Ingest::next`].
//!
//! See `decisions/2026-10-28-pages-cleaned-on-load.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use notebook_core::clean::CleanParams;
use notebook_core::decode::DecodeError;
use notebook_core::geom::Quad;
use notebook_core::loaded::Loaded;
use notebook_core::project;

use crate::jobs::Jobs;
use crate::jobs::Waker;

/// A dropped or picked file's name, where it is if known, and its contents or why they couldn't be
/// read.
pub type Received = (String, Option<PathBuf>, Result<Vec<u8>, String>);

/// Asks which files to pick: none if the user cancelled. Tests stand in for the file picker.
pub type PickFiles = Arc<dyn Fn(Pick) -> Vec<PathBuf> + Send + Sync>;

/// Files the system asked the app to open, a batch at a time: on macOS, from Finder or the Dock
/// (`crate::finder_open`). Each batch is taken as if it had been dropped on the window.
pub type OpenedFiles = std::sync::mpsc::Receiver<Vec<PathBuf>>;

/// What [`PickFiles`] asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Photos to add as pages.
    Photos,
    /// One project to open.
    Project,
}

/// What dropping some files does: a project among them opens, in place of adding pages. See
/// `decisions/2026-10-31-open-by-drop-and-argument.md`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropping {
    /// No project: the files are added as pages.
    AddPages,
    /// The one project among them opens, and the other files are left out.
    OpenProject {
        path: PathBuf,
        /// How many other files were dropped with it.
        others: usize,
    },
    /// More than one project, so none opens.
    SeveralProjects,
}

impl Dropping {
    /// What dropping files at `paths` does. A file dropped without a path isn't a project.
    pub fn of<'a>(paths: impl IntoIterator<Item = Option<&'a Path>>) -> Self {
        let (mut projects, mut others) = (Vec::new(), 0);
        for path in paths {
            match path.filter(|p| is_project(p)) {
                Some(path) => projects.push(path.to_path_buf()),
                None => others += 1,
            }
        }
        match projects.len() {
            0 => Self::AddPages,
            1 => Self::OpenProject {
                path: projects.remove(0),
                others,
            },
            _ => Self::SeveralProjects,
        }
    }
}

/// Whether `path` names a project file.
pub fn is_project(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(project::EXTENSION))
}

/// Loads a page from its file's bytes, cropped to the user's corners if given. Tests stand in for
/// [`Loaded::new`] to hold loads up.
pub type Load =
    Arc<dyn Fn(&[u8], &CleanParams, Option<Quad>) -> Result<Loaded, DecodeError> + Send + Sync>;

/// A page's name and bytes, and what loading them made.
pub(crate) type LoadOutcome = (String, Arc<[u8]>, Result<Loaded, String>);

/// A page's name and bytes, which tell it from a later page of the same name.
type LoadKey = (String, Arc<[u8]>);

pub struct Ingest {
    load: Load,
    jobs: Jobs<LoadKey, Result<Loaded, String>>,
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

    /// Loads with the same loader, with nothing loading: for another project. Pages still loading
    /// here finish, but nothing collects them.
    pub fn emptied(&self) -> Self {
        Self::new(self.load.clone())
    }

    /// Calls `waker` whenever a page finishes loading.
    pub fn set_waker(&mut self, waker: Waker) {
        self.jobs.set_waker(waker);
    }

    /// What dropping the files dropped this frame does.
    pub fn dropped(ctx: &egui::Context) -> Dropping {
        ctx.input(|i| {
            let paths = i.raw.dropped_files.iter().map(|f| dropped_path(f.path()));
            Dropping::of(paths)
        })
    }

    /// What dropping the files being dragged over the window would do, if any are.
    pub fn hovering(ctx: &egui::Context) -> Option<Dropping> {
        ctx.input(|i| {
            let hovered = &i.raw.hovered_files;
            let paths = hovered.iter().map(|f| f.path.as_deref());
            (!hovered.is_empty()).then(|| Dropping::of(paths))
        })
    }

    /// Files dropped this frame, in filename order: once the pages have been rearranged, new ones
    /// go at the end in the order they're added.
    pub fn read_drops(&self, ctx: &egui::Context) -> Vec<Received> {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        let received = dropped.iter().map(|file| {
            (
                file_name(file.path()),
                dropped_path(file.path()).map(Path::to_path_buf),
                file.bytes(),
            )
        });
        by_name(received.collect())
    }

    /// The files at `paths`, in filename order like [`Self::read_drops`].
    pub fn read_paths(paths: &[PathBuf]) -> Vec<Received> {
        let read = |path: &PathBuf| std::fs::read(path).map_err(|e| e.to_string());
        let received = paths
            .iter()
            .map(|path| (file_name(path), Some(path.clone()), read(path)));
        by_name(received.collect())
    }

    /// Starts loading the page named `name`, or cleaning it again with the user's `corners`.
    pub fn load(
        &mut self,
        name: &str,
        bytes: Arc<[u8]>,
        params: CleanParams,
        corners: Option<Quad>,
    ) {
        let load = self.load.clone();
        let key = (name.to_owned(), bytes.clone());
        self.jobs.spawn(key, move || {
            load(&bytes, &params, corners).map_err(|e| crate::i18n::decode_error(&e))
        });
    }

    /// The next page that finished loading, or failed to, if any.
    pub fn next(&mut self) -> Option<LoadOutcome> {
        let ((name, bytes), loaded) = self.jobs.next()?;
        Some((name, bytes, loaded))
    }

    /// How many pages are loading.
    pub fn pending(&self) -> usize {
        self.jobs.running()
    }
}

fn by_name(mut received: Vec<Received>) -> Vec<Received> {
    received.sort_by(|a, b| a.0.cmp(&b.0));
    received
}

/// Where a dropped file is: its path, unless it was dropped with only a name.
fn dropped_path(path: &Path) -> Option<&Path> {
    path.is_absolute().then_some(path)
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
