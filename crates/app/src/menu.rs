//! The macOS menu bar, built from [`MENUS`] with muda. It takes the keys of enabled commands
//! before egui sees them; a disabled command's key reaches egui as an ordinary key press, which is
//! how a text field keeps ← and ⌘⌫.

use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use egui::{Key, KeyboardShortcut};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{
    AboutMetadata, CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem,
    Submenu,
};

use crate::command::{Command, CommandState, Frontend, Item, MENUS, SystemItem};

/// The app's name, for the About panel.
const APP_NAME: &str = "notebook-tools";

enum Entry {
    Plain(MenuItem),
    Check(CheckMenuItem),
}

pub struct NativeMenu {
    /// The menu bar goes away with this.
    _menu: Menu,
    entries: Vec<(Command, Entry)>,
    clicked: Receiver<Command>,
}

impl NativeMenu {
    /// Makes this the app's menu bar, replacing winit's. Call on the main thread once the app has
    /// launched: in eframe's app creator.
    pub fn install(ctx: &egui::Context) -> Self {
        let (tx, clicked) = mpsc::channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(command) = Command::from_id(event.id.as_ref()) {
                // The app may have gone, and with it the receiver.
                tx.send(command).ok();
                ctx.request_repaint();
            }
        }));

        let menu = Menu::new();
        let mut entries = Vec::new();
        for spec in MENUS {
            let submenu = Submenu::new(spec.title.unwrap_or(APP_NAME), true);
            for item in spec.items {
                let added: Box<dyn IsMenuItem> = match *item {
                    Item::Command(command) => {
                        let spec = command.spec();
                        let id = spec.id;
                        let keys = spec.shortcut.map(accelerator);
                        let entry = if spec.checkable {
                            Entry::Check(CheckMenuItem::with_id(id, spec.label, true, false, keys))
                        } else {
                            Entry::Plain(MenuItem::with_id(id, spec.label, true, keys))
                        };
                        let added: Box<dyn IsMenuItem> = match &entry {
                            Entry::Plain(item) => Box::new(item.clone()),
                            Entry::Check(item) => Box::new(item.clone()),
                        };
                        entries.push((command, entry));
                        added
                    }
                    Item::System(system) => Box::new(predefined(system)),
                    Item::Separator => Box::new(PredefinedMenuItem::separator()),
                };
                submenu.append(&*added).expect("menu item");
            }
            if spec.title == Some("Window") {
                submenu.set_as_windows_menu_for_nsapp();
            }
            menu.append(&submenu).expect("menu");
        }
        menu.init_for_nsapp();
        Self {
            _menu: menu,
            entries,
            clicked,
        }
    }
}

impl Frontend for NativeMenu {
    fn poll(
        &mut self,
        _ctx: &egui::Context,
        state: &dyn Fn(Command) -> CommandState,
    ) -> Vec<Command> {
        // A click can come in after the frame that last greyed the item out.
        self.clicked
            .try_iter()
            .filter(|&command| state(command).enabled)
            .collect()
    }

    fn sync(&mut self, state: &dyn Fn(Command) -> CommandState) {
        // Compared with what's shown, not what was last set: clicking a check item toggles it.
        for (command, entry) in &self.entries {
            let state = state(*command);
            match entry {
                Entry::Plain(item) => {
                    if item.is_enabled() != state.enabled {
                        item.set_enabled(state.enabled);
                    }
                    if item.text() != state.title {
                        item.set_text(state.title);
                    }
                }
                Entry::Check(item) => {
                    if item.is_enabled() != state.enabled {
                        item.set_enabled(state.enabled);
                    }
                    let checked = state.checked.unwrap_or(false);
                    if item.is_checked() != checked {
                        item.set_checked(checked);
                    }
                }
            }
        }
    }
}

fn predefined(item: SystemItem) -> PredefinedMenuItem {
    match item {
        SystemItem::About => {
            let about = AboutMetadata {
                name: Some(APP_NAME.to_owned()),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                ..AboutMetadata::default()
            };
            PredefinedMenuItem::about(None, Some(about))
        }
        SystemItem::Services => PredefinedMenuItem::services(None),
        SystemItem::Hide => PredefinedMenuItem::hide(None),
        SystemItem::HideOthers => PredefinedMenuItem::hide_others(None),
        SystemItem::ShowAll => PredefinedMenuItem::show_all(None),
        SystemItem::Quit => PredefinedMenuItem::quit(None),
        SystemItem::CloseWindow => PredefinedMenuItem::close_window(None),
        SystemItem::Minimize => PredefinedMenuItem::minimize(None),
        SystemItem::Zoom => PredefinedMenuItem::zoom(None),
        SystemItem::Fullscreen => PredefinedMenuItem::fullscreen(None),
    }
}

/// `shortcut` as a menu item's key equivalent.
fn accelerator(shortcut: KeyboardShortcut) -> Accelerator {
    let m = shortcut.modifiers;
    let mut mods = Modifiers::empty();
    for (held, modifier) in [
        (m.command || m.mac_cmd, Modifiers::META),
        (m.shift, Modifiers::SHIFT),
        (m.alt, Modifiers::ALT),
        (m.ctrl && !m.command, Modifiers::CONTROL),
    ] {
        if held {
            mods |= modifier;
        }
    }
    let code = match shortcut.logical_key {
        Key::ArrowLeft => Code::ArrowLeft,
        Key::ArrowRight => Code::ArrowRight,
        Key::Backspace => Code::Backspace,
        Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2,
        Key::Num3 => Code::Digit3,
        Key::E => Code::KeyE,
        Key::I => Code::KeyI,
        Key::T => Code::KeyT,
        key => panic!("no key equivalent for {key:?}: add it here"),
    };
    Accelerator::new(mods, code)
}

#[cfg(test)]
mod tests {
    use egui::Modifiers as Mods;

    use super::*;

    #[test]
    fn every_shortcut_has_a_key_equivalent() {
        for command in Command::ALL {
            if let Some(shortcut) = command.spec().shortcut {
                accelerator(shortcut);
            }
        }
    }

    #[test]
    fn command_is_the_command_key() {
        let keys = |m, k| accelerator(KeyboardShortcut::new(m, k));
        assert_eq!(
            keys(Mods::COMMAND, Key::Backspace),
            Accelerator::new(Modifiers::META, Code::Backspace)
        );
        assert_eq!(
            keys(Mods::COMMAND | Mods::SHIFT, Key::T),
            Accelerator::new(Modifiers::META | Modifiers::SHIFT, Code::KeyT)
        );
        assert_eq!(
            keys(Mods::NONE, Key::ArrowLeft),
            Accelerator::new(Modifiers::empty(), Code::ArrowLeft)
        );
    }
}
