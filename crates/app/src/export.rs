//! The export dialog, and Re-export, which repeats the last export without asking.
//!
//! Only transcribed pages can be exported: if any page in scope isn't, the dialog says which and
//! won't go. Files are chosen through a [`SavePath`], so tests can stand in for the save dialog.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use notebook_core::Pages;
use notebook_core::ocr::Transcript;

/// Asks where to save a file, given a suggested file name. `None` if the user cancelled.
pub type SavePath = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// The selected page.
    #[default]
    Current,
    Everything,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    /// Not yet: see `tasks/pdf-export.md`.
    Pdf,
    #[default]
    MarkdownFile,
    MarkdownClipboard,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Settings {
    pub scope: Scope,
    pub format: Format,
}

/// What the last export did, so Re-export can repeat it.
struct Last {
    settings: Settings,
    /// Where the file went; `None` for the clipboard.
    path: Option<PathBuf>,
}

#[derive(Default)]
pub struct Export {
    save_path: Option<SavePath>,
    dialog_open: bool,
    /// What the dialog shows; kept from one export to the next.
    settings: Settings,
    last: Option<Last>,
    /// What the last export did, or why it failed.
    status: Option<Result<String, String>>,
}

impl Export {
    pub fn new(save_path: Option<SavePath>) -> Self {
        Self {
            save_path,
            ..Self::default()
        }
    }

    pub fn open_dialog(&mut self) {
        self.dialog_open = true;
    }

    pub fn can_reexport(&self) -> bool {
        self.last.is_some()
    }

    pub fn status(&self) -> Option<&Result<String, String>> {
        self.status.as_ref()
    }

    /// The export dialog, if it's open. `selected` is the index of the selected page.
    pub fn dialog(&mut self, ctx: &egui::Context, pages: &Pages, selected: usize) {
        if !self.dialog_open {
            return;
        }
        let settings = &mut self.settings;
        let mut go = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("export dialog")).show(ctx, |ui| {
            ui.heading("Export");
            ui.radio_value(&mut settings.scope, Scope::Current, "Current page");
            ui.radio_value(&mut settings.scope, Scope::Everything, "Everything");
            ui.add_space(4.0);
            ui.label("as");
            ui.add_enabled_ui(false, |ui| {
                ui.radio_value(&mut settings.format, Format::Pdf, "PDF")
                    .on_disabled_hover_text("Coming soon");
            });
            ui.radio_value(
                &mut settings.format,
                Format::MarkdownFile,
                "Markdown to file",
            );
            ui.radio_value(
                &mut settings.format,
                Format::MarkdownClipboard,
                "Markdown to clipboard",
            );
            let missing = in_scope(pages, selected, settings.scope).err();
            if let Some(missing) = &missing {
                ui.colored_label(ui.visuals().warn_fg_color, not_transcribed(missing));
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let ready = missing.is_none() && settings.format != Format::Pdf;
                go = ui.add_enabled(ready, egui::Button::new("Go")).clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if go {
            self.dialog_open = false;
            self.run(ctx, self.settings, None, pages, selected);
        } else if cancel || modal.should_close() {
            self.dialog_open = false;
        }
    }

    /// Repeats the last export, to the same file if it went to one.
    pub fn reexport(&mut self, ctx: &egui::Context, pages: &Pages, selected: usize) {
        if let Some(Last { settings, path }) = &self.last {
            let (settings, path) = (*settings, path.clone());
            self.run(ctx, settings, path, pages, selected);
        }
    }

    /// Exports what `settings` say, to `path` if given, else asking where.
    fn run(
        &mut self,
        ctx: &egui::Context,
        settings: Settings,
        path: Option<PathBuf>,
        pages: &Pages,
        selected: usize,
    ) {
        let in_scope = match in_scope(pages, selected, settings.scope) {
            Ok(in_scope) => in_scope,
            Err(missing) => {
                self.status = Some(Err(format!(
                    "Couldn't export. {}",
                    not_transcribed(&missing)
                )));
                return;
            }
        };
        let markdown = notebook_core::export::markdown(&in_scope);
        let what = match in_scope.len() {
            1 => in_scope[0].0.to_owned(),
            n => format!("{n} pages"),
        };
        let result = match settings.format {
            Format::MarkdownClipboard => {
                ctx.copy_text(markdown);
                Ok((format!("Copied {what} to the clipboard"), None))
            }
            Format::MarkdownFile => {
                let suggested = suggested_name(&in_scope, settings.scope);
                let path = path.or_else(|| self.save_path.as_ref().and_then(|ask| ask(&suggested)));
                let Some(path) = path else {
                    // Cancelled, or there's no save dialog.
                    return;
                };
                match std::fs::write(&path, markdown) {
                    Ok(()) => Ok((format!("Exported {what} to {}", path.display()), Some(path))),
                    Err(err) => Err(format!("Couldn't write {}: {err}", path.display())),
                }
            }
            Format::Pdf => Err("PDF export isn't available yet".to_owned()),
        };
        self.status = Some(result.map(|(message, path)| {
            log::info!("{message}");
            self.last = Some(Last { settings, path });
            message
        }));
    }
}

/// The transcribed pages `scope` covers, by name, or the names of those not transcribed yet.
fn in_scope(
    pages: &Pages,
    selected: usize,
    scope: Scope,
) -> Result<Vec<(&str, &Transcript)>, Vec<&str>> {
    let covered: Vec<_> = match scope {
        Scope::Current => pages.get(selected).into_iter().collect(),
        Scope::Everything => pages.iter().collect(),
    };
    let missing: Vec<&str> = covered
        .iter()
        .filter(|p| p.transcript.is_none())
        .map(|p| p.name.as_str())
        .collect();
    if !missing.is_empty() || covered.is_empty() {
        return Err(missing);
    }
    Ok(covered
        .into_iter()
        .filter_map(|p| Some((p.name.as_str(), p.transcript.as_ref()?)))
        .collect())
}

fn not_transcribed(names: &[&str]) -> String {
    format!("Not transcribed yet: {}", names.join(", "))
}

/// The page's name as markdown for one page, else a name for the notebook.
fn suggested_name(pages: &[(&str, &Transcript)], scope: Scope) -> String {
    match (scope, pages) {
        (Scope::Current, [(name, _)]) => {
            let stem = Path::new(name).file_stem().unwrap_or_default();
            format!("{}.md", stem.to_string_lossy())
        }
        _ => "notebook.md".to_owned(),
    }
}
