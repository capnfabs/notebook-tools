//! The open project: New Project, Open Project…, Save and Save As…, whether there are unsaved
//! changes, and asking to save them before they'd be lost: on New, Open, closing the window and
//! quitting. Only one project is open at a time; the app starts with an untitled one.
//!
//! Unsaved changes are found by comparing the pages with what was last saved or opened, rather
//! than by noting each change, so none can be missed. See `decisions/2026-10-31-projects.md`.
//!
//! A project also opens when dropped on the window, or named on the command line: see
//! `decisions/2026-10-31-open-by-drop-and-argument.md`.

use std::path::{Path, PathBuf};

use eframe::egui;
use notebook_core::project::{self, EXTENSION, Missing, Opened, SavedPage};

use super::NotebookApp;
use crate::export::Export;
use crate::ingest::Pick;
use crate::t;
use crate::ui::dialog::{self, Choice, Stakes};

/// What to do once unsaved changes are saved or discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum After {
    NewProject,
    /// Ask which project to open, and open it.
    PickProject,
    /// Open the project at this path, which was dropped on the window.
    OpenProject(PathBuf),
    Quit,
}

/// The open project's file and what was in it.
#[derive(Default)]
pub(super) struct Project {
    /// `None` until it's saved.
    path: Option<PathBuf>,
    /// Pages whose photos weren't found when it opened, kept to save again.
    missing: Vec<Missing>,
    /// The pages as last saved or opened.
    saved: Vec<SavedPage>,
    /// Asking to save before doing this, until the user answers.
    confirm: Option<After>,
    /// The user chose to quit, so the window may close.
    quitting: bool,
    /// The window's title, as last set.
    title: Option<String>,
}

impl NotebookApp {
    /// Whether the pages differ from what was last saved or opened.
    pub fn has_unsaved_changes(&self) -> bool {
        let saved = &self.project.saved;
        saved.len() != self.pages.len()
            || !saved
                .iter()
                .zip(self.pages.iter())
                .all(|(s, p)| s.matches(p))
    }

    /// The project's name, as the window's title shows it: its file's name, or Untitled.
    pub fn project_name(&self) -> String {
        let name = self.project.path.as_deref().and_then(Path::file_stem);
        name.map_or_else(
            || t!("project-untitled"),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// What the window's title says: the project's name, and whether it has unsaved changes.
    pub fn window_title(&self) -> String {
        let name = self.project_name();
        if self.has_unsaved_changes() {
            t!("window-title-edited", name = name)
        } else {
            name
        }
    }

    /// Whether the user chose to quit, with nothing left unsaved that they meant to keep: the
    /// window is closing.
    pub fn quitting(&self) -> bool {
        self.project.quitting
    }

    /// Where the project is saved, if it is.
    pub fn project_path(&self) -> Option<&Path> {
        self.project.path.as_deref()
    }

    /// Whether Save As… has anything to save.
    pub(super) fn can_save_as(&self) -> bool {
        !self.pages.is_empty() || self.project.path.is_some()
    }

    pub(super) fn confirming_unsaved(&self) -> bool {
        self.project.confirm.is_some()
    }

    /// Does `after`, once any unsaved changes are saved or the user lets them go.
    pub(super) fn after_saving(&mut self, ctx: &egui::Context, after: After) {
        if self.has_unsaved_changes() {
            self.project.confirm = Some(after);
        } else {
            self.proceed(ctx, after);
        }
    }

    fn proceed(&mut self, ctx: &egui::Context, after: After) {
        match after {
            After::NewProject => self.new_project(),
            After::PickProject => self.open_picked(),
            After::OpenProject(path) => self.open_project(&path),
            After::Quit => {
                self.project.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Asks whether to save the changes before what the user asked for, if it's asking.
    pub(super) fn unsaved_dialog(&mut self, ctx: &egui::Context) {
        let Some(after) = self.project.confirm.clone() else {
            return;
        };
        let name = self.project_name();
        let choice = dialog::show(ctx, "unsaved changes", 360.0, |ui| {
            ui.label(egui::RichText::new(t!("unsaved-title", name = name)).strong());
            ui.label(t!("unsaved-detail"));
            dialog::buttons_with_discard(
                ui,
                &t!("unsaved-save"),
                &t!("unsaved-discard"),
                Stakes::Safe,
            )
        });
        let Some(choice) = choice else {
            return;
        };
        self.project.confirm = None;
        match choice {
            Choice::Affirmative => {
                if self.save() {
                    self.proceed(ctx, after);
                }
            }
            Choice::Discard => self.proceed(ctx, after),
            Choice::Cancel => {}
        }
    }

    /// Lets the window close, unless that would lose unsaved changes: then asks first.
    pub(super) fn handle_close_request(&mut self, ctx: &egui::Context) {
        let requested = ctx.input(|i| i.viewport().close_requested());
        if !requested || self.project.quitting || !self.has_unsaved_changes() {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        // Another dialog has to be answered first.
        if !self.dialog_open() {
            self.project.confirm = Some(After::Quit);
        }
    }

    /// Keeps the window's title up to date.
    pub(super) fn update_title(&mut self, ctx: &egui::Context) {
        let title = self.window_title();
        if self.project.title.as_ref() != Some(&title) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.project.title = Some(title);
        }
    }

    /// Saves to the project's file, asking where first if it hasn't one. Returns whether it saved.
    pub(super) fn save(&mut self) -> bool {
        match self.project.path.clone() {
            Some(path) => self.save_to(&path),
            None => self.save_as(),
        }
    }

    /// Asks where to save the project, and saves it there. Returns whether it saved.
    pub(super) fn save_as(&mut self) -> bool {
        let Some(save_path) = self.save_path.clone() else {
            return false;
        };
        self.asked_which_files = true;
        let suggested = format!("{}.{EXTENSION}", self.project_name());
        let Some(mut path) = save_path(&suggested) else {
            return false;
        };
        if path.extension().is_none_or(|e| e != EXTENSION) {
            path.as_mut_os_string().push(format!(".{EXTENSION}"));
        }
        self.save_to(&path)
    }

    fn save_to(&mut self, path: &Path) -> bool {
        match project::save(path, &self.pages, &self.project.missing) {
            Ok(()) => {
                self.project.path = Some(path.to_path_buf());
                self.mark_saved();
                true
            }
            Err(err) => {
                log::warn!("couldn't save {}: {err:?}", path.display());
                self.errors
                    .push(crate::i18n::save_project_error(path, &err));
                false
            }
        }
    }

    /// Takes the pages as they are now as what's saved.
    fn mark_saved(&mut self) {
        self.project.saved = self.pages.iter().filter_map(SavedPage::of).collect();
    }

    /// Asks which project to open, and opens it.
    fn open_picked(&mut self) {
        let Some(pick) = self.pick_files.clone() else {
            return;
        };
        self.asked_which_files = true;
        if let Some(path) = pick(Pick::Project).into_iter().next() {
            self.open_project(&path);
        }
    }

    /// Opens the project at `path` in place of this one, without asking to save this one: see
    /// [`Self::after_saving`]. If it can't be opened, this one stays.
    pub fn open_project(&mut self, path: &Path) {
        match project::open(path) {
            Ok(opened) => self.show_opened(path, opened),
            Err(err) => {
                log::warn!("couldn't open {}: {err:?}", path.display());
                self.errors
                    .push(crate::i18n::open_project_error(path, &err));
            }
        }
    }

    /// Starts with `opened`, the project at `path`, open: for opening one named on the command
    /// line, which was read before the window opened.
    pub fn with_project(mut self, path: &Path, opened: Opened) -> Self {
        self.show_opened(path, opened);
        self
    }

    /// Puts `opened`, the project read from `path`, in place of this one.
    fn show_opened(&mut self, path: &Path, opened: Opened) {
        self.new_project();
        self.pages = opened.pages;
        self.errors = opened
            .missing
            .iter()
            .map(crate::i18n::missing_photo)
            .collect();
        self.project.missing = opened.missing;
        self.project.path = Some(path.to_path_buf());
        let names: Vec<String> = self.pages.iter().map(|p| p.name.clone()).collect();
        for name in &names {
            let Some(page) = self.pages.find(name) else {
                continue;
            };
            let (bytes, corners) = (page.bytes.clone(), page.corners);
            if page.transcript.is_none() {
                self.transcriptions.look_up(name, bytes.clone());
            }
            self.ingest
                .load(name, bytes, self.clean_params.clone(), corners);
            // In the settings' style, which isn't a change to the project.
            self.restyle_if_stale(name);
        }
        self.mark_saved();
    }

    /// Starts an untitled project with no pages, keeping the app's settings, transcriber, cache
    /// and dialogs. Pages still loading or being transcribed finish, unseen.
    pub(super) fn new_project(&mut self) {
        let fresh = NotebookApp {
            transcriptions: self.transcriptions.emptied(),
            ingest: self.ingest.emptied(),
            export: Export::new(self.save_path.clone()),
            clean_params: self.clean_params.clone(),
            settings_path: self.settings_path.take(),
            eval_log: std::mem::take(&mut self.eval_log),
            input: std::mem::take(&mut self.input),
            pick_files: self.pick_files.take(),
            save_path: self.save_path.take(),
            asked_which_files: self.asked_which_files,
            // Says what happened to the files dropped with the project being opened.
            hint: self.hint.take(),
            waker: self.waker.take(),
            ..NotebookApp::default()
        };
        *self = fresh;
        self.set_wakers();
    }
}
