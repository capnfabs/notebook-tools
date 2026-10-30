//! Everything the user can do from the menu bar or the keyboard, as data: each [`Command`]'s
//! label and shortcut, and where it goes in the menus ([`MENUS`]). The app says what's possible
//! right now ([`CommandState`]) and runs commands; a [`Frontend`] turns the user's input into
//! commands, and shows their state. Exactly one frontend takes input: the native menu bar where
//! there is one, else [`EguiKeys`].
//!
//! See `decisions/2026-10-29-mac-menu.md`.

use eframe::egui;
use egui::{Key, KeyboardShortcut, Modifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    AddPages,
    /// Re-export if there's a last export, else Export….
    Export,
    /// Always asks what to export.
    ExportAs,
    ViewOriginal,
    ViewCleaned,
    ViewTranscribed,
    PreviousPage,
    NextPage,
    Transcribe,
    ForceRetranscribe,
    /// Drops the user's corrections to the page's transcript.
    ResetTranscript,
    TranscribeAll,
    MoveLeft,
    MoveRight,
    SortByName,
    /// Click the page's four corners, in place of the ones it has.
    PlaceCorners,
    /// Back to the corners cleaning found.
    ResetCorners,
    Delete,
}

/// What a command is, whatever state the app is in.
pub struct Spec {
    /// Stable, for frontends to name it by.
    pub id: &'static str,
    pub label: &'static str,
    pub shortcut: Option<KeyboardShortcut>,
    /// Its shortcut is a text-editing key, so it's off while a text field has the keyboard.
    pub edits_text: bool,
    /// It toggles or picks something, so it shows a check mark.
    pub checkable: bool,
}

/// What a command can do right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandState {
    pub enabled: bool,
    /// For commands that toggle or pick something; `None` for the rest.
    pub checked: Option<bool>,
    /// Usually the label, but it can depend on the state, as Export's does.
    pub title: &'static str,
}

const fn keys(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

const COMMAND_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const COMMAND_ALT: Modifiers = Modifiers::COMMAND.plus(Modifiers::ALT);

impl Command {
    pub const ALL: [Command; 18] = [
        Self::AddPages,
        Self::Export,
        Self::ExportAs,
        Self::ViewOriginal,
        Self::ViewCleaned,
        Self::ViewTranscribed,
        Self::PreviousPage,
        Self::NextPage,
        Self::Transcribe,
        Self::ForceRetranscribe,
        Self::ResetTranscript,
        Self::TranscribeAll,
        Self::MoveLeft,
        Self::MoveRight,
        Self::SortByName,
        Self::PlaceCorners,
        Self::ResetCorners,
        Self::Delete,
    ];

    pub const fn spec(self) -> Spec {
        let (id, label, shortcut, edits_text) = match self {
            Self::AddPages => (
                "add-pages",
                "Add Pages…",
                keys(COMMAND_SHIFT, Key::I),
                false,
            ),
            Self::Export => ("export", "Export", keys(Modifiers::COMMAND, Key::E), false),
            Self::ExportAs => ("export-as", "Export…", keys(COMMAND_SHIFT, Key::E), false),
            Self::ViewOriginal => (
                "view-original",
                "Original",
                keys(Modifiers::COMMAND, Key::Num1),
                false,
            ),
            Self::ViewCleaned => (
                "view-cleaned",
                "Cleaned",
                keys(Modifiers::COMMAND, Key::Num2),
                false,
            ),
            Self::ViewTranscribed => (
                "view-transcribed",
                "Transcribed",
                keys(Modifiers::COMMAND, Key::Num3),
                false,
            ),
            Self::PreviousPage => (
                "previous-page",
                "Previous Page",
                keys(Modifiers::NONE, Key::ArrowLeft),
                true,
            ),
            Self::NextPage => (
                "next-page",
                "Next Page",
                keys(Modifiers::NONE, Key::ArrowRight),
                true,
            ),
            Self::Transcribe => (
                "transcribe",
                "Transcribe",
                keys(Modifiers::COMMAND, Key::T),
                false,
            ),
            Self::ForceRetranscribe => (
                "force-retranscribe",
                "Force Retranscription",
                keys(COMMAND_SHIFT, Key::T),
                false,
            ),
            Self::ResetTranscript => ("reset-transcript", "Reset Transcript", None, false),
            Self::TranscribeAll => (
                "transcribe-all",
                "Transcribe All",
                keys(COMMAND_ALT, Key::T),
                false,
            ),
            Self::MoveLeft => (
                "move-left",
                "Move Left",
                keys(Modifiers::COMMAND, Key::ArrowLeft),
                true,
            ),
            Self::MoveRight => (
                "move-right",
                "Move Right",
                keys(Modifiers::COMMAND, Key::ArrowRight),
                true,
            ),
            Self::SortByName => ("sort-by-name", "Sort by Name", None, false),
            Self::PlaceCorners => ("place-corners", "Place Corners", None, false),
            Self::ResetCorners => ("reset-corners", "Reset Corners", None, false),
            Self::Delete => (
                "delete",
                "Delete",
                keys(Modifiers::COMMAND, Key::Backspace),
                true,
            ),
        };
        let checkable = matches!(
            self,
            Self::ViewOriginal | Self::ViewCleaned | Self::ViewTranscribed
        );
        Spec {
            id,
            label,
            shortcut,
            edits_text,
            checkable,
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.spec().id == id)
    }
}

/// Menu items the platform provides, rather than the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemItem {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    CloseWindow,
    Minimize,
    Zoom,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Command(Command),
    System(SystemItem),
    Separator,
}

pub struct Menu {
    /// `None` for the menu named after the app.
    pub title: Option<&'static str>,
    pub items: &'static [Item],
}

/// The menu bar, left to right. Settings… goes in the app menu once there is one.
pub const MENUS: &[Menu] = {
    use Command as C;
    use Item::{Command as Do, Separator as Sep, System as Sys};
    use SystemItem as S;
    &[
        Menu {
            title: None,
            items: &[
                Sys(S::About),
                Sep,
                Sys(S::Services),
                Sep,
                Sys(S::Hide),
                Sys(S::HideOthers),
                Sys(S::ShowAll),
                Sep,
                Sys(S::Quit),
            ],
        },
        Menu {
            title: Some("File"),
            items: &[
                Do(C::AddPages),
                Sep,
                Do(C::Export),
                Do(C::ExportAs),
                Sep,
                Sys(S::CloseWindow),
            ],
        },
        Menu {
            title: Some("View"),
            items: &[
                Do(C::ViewOriginal),
                Do(C::ViewCleaned),
                Do(C::ViewTranscribed),
                Sep,
                Do(C::PreviousPage),
                Do(C::NextPage),
            ],
        },
        Menu {
            title: Some("Page"),
            items: &[
                Do(C::Transcribe),
                Do(C::ForceRetranscribe),
                Do(C::ResetTranscript),
                Do(C::TranscribeAll),
                Sep,
                Do(C::MoveLeft),
                Do(C::MoveRight),
                Do(C::SortByName),
                Sep,
                Do(C::PlaceCorners),
                Do(C::ResetCorners),
                Sep,
                Do(C::Delete),
            ],
        },
        Menu {
            title: Some("Window"),
            items: &[Sys(S::Minimize), Sys(S::Zoom), Sep, Sys(S::Fullscreen)],
        },
    ]
};

/// Where the user's commands come from.
pub trait Frontend {
    /// Commands the user gave since the last frame, given what each can do. Called once a frame,
    /// before any widgets.
    fn poll(
        &mut self,
        ctx: &egui::Context,
        state: &dyn Fn(Command) -> CommandState,
    ) -> Vec<Command>;

    /// Shows what each command can do, e.g. by greying out menu items. Called once a frame.
    fn sync(&mut self, _state: &dyn Fn(Command) -> CommandState) {}
}

/// Takes commands from egui's key presses, for where there's no native menu bar (and tests).
#[derive(Default)]
pub struct EguiKeys;

impl Frontend for EguiKeys {
    fn poll(
        &mut self,
        ctx: &egui::Context,
        state: &dyn Fn(Command) -> CommandState,
    ) -> Vec<Command> {
        // egui ignores extra Shift and Alt when matching, so the shortcuts with most modifiers
        // go first: ⇧⌘T mustn't also be taken as ⌘T.
        let mut shortcuts: Vec<(Command, KeyboardShortcut)> = Command::ALL
            .into_iter()
            .filter(|&c| state(c).enabled)
            .filter_map(|c| Some((c, c.spec().shortcut?)))
            .collect();
        shortcuts.sort_by_key(|(_, s)| {
            let m = s.modifiers;
            std::cmp::Reverse(
                [m.alt, m.shift, m.ctrl, m.command]
                    .iter()
                    .filter(|&&m| m)
                    .count(),
            )
        });
        ctx.input_mut(|i| {
            shortcuts
                .into_iter()
                .filter(|(_, shortcut)| i.consume_shortcut(shortcut))
                .map(|(command, _)| command)
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_and_shortcuts_are_unique() {
        let ids: HashSet<_> = Command::ALL.iter().map(|c| c.spec().id).collect();
        assert_eq!(ids.len(), Command::ALL.len());
        let shortcuts: Vec<_> = Command::ALL
            .iter()
            .filter_map(|c| c.spec().shortcut)
            .collect();
        let unique: HashSet<_> = shortcuts.iter().collect();
        assert_eq!(unique.len(), shortcuts.len());
        for c in Command::ALL {
            assert_eq!(Command::from_id(c.spec().id), Some(c));
        }
    }

    /// The app keeps commands' states in an array indexed by `command as usize`.
    #[test]
    fn all_is_in_declaration_order() {
        for (i, c) in Command::ALL.into_iter().enumerate() {
            assert_eq!(c as usize, i, "{c:?}");
        }
    }

    #[test]
    fn every_command_is_in_the_menus_once() {
        let listed: Vec<Command> = MENUS
            .iter()
            .flat_map(|m| m.items)
            .filter_map(|item| match item {
                Item::Command(c) => Some(*c),
                _ => None,
            })
            .collect();
        for c in Command::ALL {
            let times = listed.iter().filter(|&&l| l == c).count();
            assert_eq!(times, 1, "{c:?} is listed {times} times");
        }
    }

    fn enabled(c: Command) -> CommandState {
        CommandState {
            enabled: c != Command::Delete,
            checked: None,
            title: c.spec().label,
        }
    }

    /// The commands [`EguiKeys`] takes from pressing `key` with `modifiers`.
    fn pressed(modifiers: Modifiers, key: Key) -> Vec<Command> {
        let ctx = egui::Context::default();
        let event = egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let input = egui::RawInput {
            events: vec![event],
            ..Default::default()
        };
        let mut commands = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            commands = EguiKeys.poll(ui.ctx(), &enabled);
        });
        output.textures_delta.clear();
        commands
    }

    #[test]
    fn the_most_specific_shortcut_wins() {
        assert_eq!(pressed(COMMAND_SHIFT, Key::T), [Command::ForceRetranscribe]);
        assert_eq!(pressed(COMMAND_ALT, Key::T), [Command::TranscribeAll]);
        assert_eq!(pressed(Modifiers::COMMAND, Key::T), [Command::Transcribe]);
        assert_eq!(
            pressed(Modifiers::COMMAND, Key::ArrowLeft),
            [Command::MoveLeft]
        );
        assert_eq!(
            pressed(Modifiers::NONE, Key::ArrowLeft),
            [Command::PreviousPage]
        );
    }

    #[test]
    fn disabled_commands_dont_take_their_keys() {
        assert_eq!(pressed(Modifiers::COMMAND, Key::Backspace), []);
    }
}
