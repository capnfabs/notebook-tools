//! The dialog asking for a Claude API key: where to get one, a field to paste it in, and a check
//! with the API before it's saved in the keychain. Opened from Settings, and by transcribing
//! without credentials. Without a keychain, it says to set `ANTHROPIC_API_KEY` instead.
//!
//! See `decisions/2026-11-15-api-key-in-keychain.md`.

use eframe::egui;
use egui::{Key, RichText};
use notebook_core::claude_transcribe::OcrError;
use notebook_core::credentials::{Credentials, Source, Stored};

use crate::auth::Auth;
use crate::jobs::{Jobs, Waker};
use crate::t;
use crate::ui::dialog::{self, Choice, Stakes};

/// Where keys are made.
pub const CONSOLE_KEYS_URL: &str = "https://platform.claude.com/settings/keys";

/// What the dialog did, for the app to act on.
pub enum Outcome {
    /// A key was saved, with a warning if it couldn't be checked.
    Saved { warning: Option<String> },
    /// The user closed the dialog without saving a key.
    Closed,
}

/// The key, and what the API said of it.
type Checked = (String, Result<(), OcrError>);

pub struct KeyDialog {
    open: bool,
    /// What's in the field.
    key: String,
    /// Why the last key wasn't saved.
    error: Option<String>,
    /// Which check is running, if one is. Checks from before the dialog last opened are ignored.
    checking: Option<u32>,
    opened: u32,
    /// Focus the field on the next frame.
    focus_field: bool,
    jobs: Jobs<u32, Checked>,
}

impl Default for KeyDialog {
    fn default() -> Self {
        Self {
            open: false,
            key: String::new(),
            error: None,
            checking: None,
            opened: 0,
            focus_field: false,
            jobs: Jobs::on_own_threads(),
        }
    }
}

impl KeyDialog {
    /// Opens the dialog with an empty field, forgetting any check still running.
    pub fn open(&mut self) {
        self.open = true;
        self.key.clear();
        self.error = None;
        self.checking = None;
        self.opened += 1;
        self.focus_field = true;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Repaints the UI when a check finishes.
    pub fn set_waker(&mut self, waker: Waker) {
        self.jobs.set_waker(waker);
    }

    /// The dialog, if it's open, saving the key in `auth` once the API accepts it.
    pub fn show(&mut self, ctx: &egui::Context, auth: &mut Auth) -> Option<Outcome> {
        let saved = self.take_checked(auth);
        if saved.is_some() || !self.open {
            return saved;
        }
        let closed = match auth.stored().clone() {
            Stored::Unavailable(why) => no_keychain(ctx, &why),
            Stored::Has(_) | Stored::Empty => self.ask(ctx, auth),
        };
        if closed {
            self.open = false;
            return Some(Outcome::Closed);
        }
        None
    }

    /// Saves the key whose check came back, if the API didn't reject it.
    fn take_checked(&mut self, auth: &mut Auth) -> Option<Outcome> {
        let (opened, (key, result)) = self.jobs.next()?;
        if !self.open || self.checking != Some(opened) {
            return None;
        }
        self.checking = None;
        let warning = match result {
            Ok(()) => None,
            Err(OcrError::Api {
                status: 401 | 403, ..
            }) => {
                self.error = Some(t!("api-key-rejected"));
                self.focus_field = true;
                return None;
            }
            // Maybe offline, or the API's busy: the key may well be fine.
            Err(err) => Some(t!(
                "api-key-unverified",
                error = crate::i18n::ocr_error(&err)
            )),
        };
        if let Err(err) = auth.save(&key) {
            log::warn!("couldn't save the API key: {err}");
            self.error = Some(t!("api-key-save-failed", error = err));
            return None;
        }
        self.open = false;
        Some(Outcome::Saved { warning })
    }

    /// Asks for a key. Returns whether the user closed the dialog.
    fn ask(&mut self, ctx: &egui::Context, auth: &Auth) -> bool {
        let checking = self.checking.is_some();
        let env = auth.env_source();
        let mut entered = false;
        let choice = dialog::show(ctx, "api key", 420.0, |ui| {
            ui.heading(t!("api-key-title"));
            ui.add_space(8.0);
            ui.label(t!("api-key-explanation"));
            ui.hyperlink_to(t!("api-key-console-link"), CONSOLE_KEYS_URL);
            if let Some(variable) = env.and_then(variable) {
                ui.label(
                    RichText::new(t!("api-key-env-note", variable = variable))
                        .small()
                        .weak(),
                );
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let label = ui.label(t!("api-key-field"));
                let field = egui::TextEdit::singleline(&mut self.key)
                    .password(true)
                    .hint_text("sk-ant-…")
                    .desired_width(f32::INFINITY);
                let field = ui.add_enabled(!checking, field).labelled_by(label.id);
                if std::mem::take(&mut self.focus_field) {
                    field.request_focus();
                }
                entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            });
            if checking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t!("api-key-checking"));
                });
            } else if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            let ready = !checking && !self.key.trim().is_empty();
            dialog::buttons(ui, &t!("api-key-save"), Stakes::Safe, ready)
        });
        match choice {
            Some(Choice::Cancel | Choice::Discard) => true,
            Some(Choice::Affirmative) => {
                self.start_check(auth);
                false
            }
            None => {
                if entered && !checking && !self.key.trim().is_empty() {
                    self.start_check(auth);
                }
                false
            }
        }
    }

    /// Checks the key in the field with the API, off the UI thread.
    fn start_check(&mut self, auth: &Auth) {
        let key = self.key.trim().to_owned();
        let check = auth.check();
        self.error = None;
        self.checking = Some(self.opened);
        self.jobs.spawn(self.opened, move || {
            let result = check(&Credentials::ApiKey(key.clone()));
            (key, result)
        });
    }
}

/// The environment variable credentials from `source` came from.
pub fn variable(source: Source) -> Option<&'static str> {
    match source {
        Source::EnvApiKey => Some("ANTHROPIC_API_KEY"),
        Source::EnvAuthToken => Some("ANTHROPIC_AUTH_TOKEN"),
        Source::Keychain | Source::Ant => None,
    }
}

/// Says there's no keychain, and why. Returns whether the user closed it.
fn no_keychain(ctx: &egui::Context, why: &str) -> bool {
    let choice = dialog::show(ctx, "api key", 420.0, |ui| {
        ui.heading(t!("api-key-title"));
        ui.add_space(8.0);
        ui.label(t!("api-key-no-keychain"));
        ui.label(
            RichText::new(t!("api-key-no-keychain-why", error = why))
                .small()
                .weak(),
        );
        dialog::only_button(ui, &t!("api-key-close"))
    });
    choice.is_some()
}
