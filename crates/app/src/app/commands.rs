//! What each [`Command`] can do right now, and doing it. Menus, key presses and the buttons that
//! do the same as a command all go by [`NotebookApp::can`]. See `decisions/2026-10-29-mac-menu.md`.

use eframe::egui;
use egui::Response;
use notebook_core::page::Why;
use notebook_core::transcript::Transcript;

use super::project::After;
use super::{NotebookApp, View};
use crate::command::{Command, CommandState};
use crate::t;

impl NotebookApp {
    /// Whether `command` makes sense now, whether or not a dialog is open or the user is typing.
    /// Buttons that do the same as a command go by this.
    pub(super) fn can(&self, command: Command) -> bool {
        let index = self.selected_index();
        let page = self.pages.get(index);
        // A page whose photo is missing can't be transcribed, or have its corners changed.
        let has_photo = page.is_some_and(|p| p.bytes().is_some());
        // Without credentials, transcribing asks for a key, if the app can keep one.
        let can_ask_for_key = self.auth.is_some();
        let can_transcribe = self.transcriptions.can_send() || can_ask_for_key;
        let can_view = self.transcriptions.can_show() || can_ask_for_key;
        let can_export = self.transcriptions.can_show() && self.export.running().is_none();
        match command {
            Command::Settings | Command::Quit | Command::NewProject => true,
            Command::OpenProject | Command::AddPages => self.pick_files.is_some(),
            Command::Save => self.save_path.is_some() && self.has_unsaved_changes(),
            Command::SaveAs => self.save_path.is_some() && self.can_save_as(),
            Command::Export | Command::ExportAs => page.is_some() && can_export,
            Command::ViewOriginal | Command::ViewCleaned => page.is_some(),
            Command::PlaceCorners => page.is_some_and(|p| p.loaded.is_some()),
            Command::ResetCorners => has_photo && page.is_some_and(|p| p.corners.is_some()),
            Command::ViewTranscribed => page.is_some() && can_view,
            Command::Transcribe => has_photo && can_transcribe,
            Command::ForceRetranscribe => {
                has_photo
                    && can_transcribe
                    && page.is_some_and(|p| {
                        p.transcript.is_some() && !self.transcriptions.queued_or_running(p.id)
                    })
            }
            Command::ResetTranscript => page.is_some_and(|p| {
                p.transcript.as_ref().is_some_and(Transcript::corrected)
                    && !self.transcriptions.queued_or_running(p.id)
            }),
            Command::TranscribeAll => can_transcribe && !self.untranscribed().is_empty(),
            Command::PreviousPage | Command::MoveLeft => page.is_some() && index > 0,
            Command::NextPage | Command::MoveRight => index + 1 < self.pages.len(),
            Command::SortByName => !self.pages.is_sorted(),
            Command::RelinkPhoto => {
                self.pick_files.is_some() && page.is_some_and(|p| p.missing_photo().is_some())
            }
            Command::UseChangedFile => {
                page.is_some_and(|p| p.missing_photo() == Some(&Why::Changed))
            }
            Command::Delete => page.is_some(),
        }
    }

    /// What `command` can do right now. Menus and key presses both go by this.
    pub(super) fn command_state(&self, ctx: &egui::Context, command: Command) -> CommandState {
        let spec = command.spec();
        let possible = self.can(command);
        // Only a text field keeps its keys from the commands; a focused label or button doesn't.
        // See `decisions/2026-10-31-keyboard-focus.md`.
        let typing = spec.edits_text && ctx.text_edit_focused();
        let checked = match command {
            Command::ViewOriginal => Some(self.view == View::Original),
            Command::ViewCleaned => Some(self.view == View::Cleaned),
            Command::ViewTranscribed => Some(self.view == View::Transcribed),
            _ => None,
        };
        let title = match command {
            Command::Export if self.export.can_reexport() => t!("command-reexport"),
            _ => command.label(),
        };
        CommandState {
            enabled: possible && !typing && !self.dialog_open(),
            checked,
            title,
        }
    }

    /// [`Self::command_state`] for each command, indexed by `command as usize`.
    pub(super) fn command_states(&self, ctx: &egui::Context) -> [CommandState; Command::ALL.len()] {
        Command::ALL.map(|c| self.command_state(ctx, c))
    }

    /// Whether a dialog is waiting for an answer. Commands wait for it too.
    pub(super) fn dialog_open(&self) -> bool {
        self.confirm_delete.is_some()
            || self.confirm_retranscribe.is_some()
            || self.confirm_reset.is_some()
            || self.confirm_transcribe_all
            || self.export.dialog_open()
            || self.settings_dialog.is_open()
            || self.key_dialog.is_open()
            || self.confirming_unsaved()
            || self.relinking.dialog_open()
    }

    /// Runs the commands given since the last frame.
    pub(super) fn run_commands(&mut self, ctx: &egui::Context) {
        let states = self.command_states(ctx);
        let commands = self.input.0.poll(ctx, &|c| states[c as usize].clone());
        for command in commands {
            self.run(ctx, command);
        }
    }

    /// Does what `command` says, if it's enabled.
    pub(super) fn run(&mut self, ctx: &egui::Context, command: Command) {
        if !self.command_state(ctx, command).enabled {
            return;
        }
        let index = self.selected_index();
        match command {
            Command::Settings => {
                let cache = self.transcriptions.cache().cloned();
                self.settings_dialog
                    .open(self.transcriptions.settings(), cache);
            }
            Command::Quit => self.after_saving(ctx, After::Quit),
            Command::NewProject => self.after_saving(ctx, After::NewProject),
            Command::OpenProject => self.after_saving(ctx, After::PickProject),
            Command::Save => {
                self.save();
            }
            Command::SaveAs => {
                self.save_as();
            }
            Command::AddPages => self.add_picked(ctx),
            Command::Export if self.export.can_reexport() => {
                self.export.reexport(ctx, &self.pages, index);
            }
            Command::Export | Command::ExportAs => self.export.open_dialog(),
            Command::ViewOriginal => self.set_view(View::Original),
            Command::ViewCleaned => self.set_view(View::Cleaned),
            Command::ViewTranscribed => self.set_view(View::Transcribed),
            Command::PreviousPage => self.select(index - 1),
            Command::NextPage => self.select(index + 1),
            Command::Transcribe => self.transcribe_selected(ctx),
            Command::ForceRetranscribe => {
                self.set_view(View::Transcribed);
                self.request_retranscribe(index);
            }
            Command::ResetTranscript => {
                self.set_view(View::Transcribed);
                self.request_reset(index);
            }
            Command::TranscribeAll => self.request_transcribe_all(),
            Command::MoveLeft => self.move_page(index, index - 1),
            Command::MoveRight => self.move_page(index, index + 1),
            Command::SortByName => self.sort_pages(),
            Command::PlaceCorners => self.place_corners(index),
            Command::ResetCorners => self.set_corners(index, None),
            Command::RelinkPhoto => self.relink_picked(ctx),
            Command::UseChangedFile => self.use_changed_file(ctx),
            Command::Delete => self.request_delete(),
        }
    }
}

/// `command`'s shortcut, as the platform writes it: "⇧⌘T" on a Mac.
pub(super) fn shortcut_text(ctx: &egui::Context, command: Command) -> String {
    let shortcut = command.spec().shortcut;
    shortcut.map_or_else(String::new, |s| ctx.format_shortcut(&s))
}

impl NotebookApp {
    /// A button for `command`, enabled when [`Self::can`] says, saying on hover which shortcut
    /// does the same.
    pub(super) fn command_button(&self, ui: &mut egui::Ui, command: Command) -> Response {
        let button = ui.add_enabled(self.can(command), egui::Button::new(command.label()));
        with_shortcut(button, command)
    }
}

/// `button`, saying on hover which shortcut does the same.
pub(super) fn with_shortcut(button: Response, command: Command) -> Response {
    if command.spec().shortcut.is_none() {
        return button;
    }
    let keys = shortcut_text(&button.ctx, command);
    button.on_hover_text(keys)
}
