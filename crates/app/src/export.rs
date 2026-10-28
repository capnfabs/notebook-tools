//! The export dialog, and Re-export, which repeats the last export without asking.
//!
//! Only transcribed pages can be exported: if any page in scope isn't, the dialog says which and
//! won't go. Files are chosen through a [`SavePath`], so tests can stand in for the save dialog.
//! The export itself runs on a thread, as a PDF cleans every page.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;
use notebook_core::export::PdfPage;
use notebook_core::transcript::Transcript;
use notebook_core::{CleanParams, Pages};

use crate::ui::dialog::{self, Action, Choice};

/// Asks where to save a file, given a suggested file name. `None` if the user cancelled.
pub type SavePath = Arc<dyn Fn(&str) -> Option<PathBuf> + Send + Sync>;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Everything,
    /// The selected page.
    Current,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    MarkdownClipboard,
    MarkdownFile,
    /// The cleaned pages, searchable, with the markdown embedded.
    Pdf,
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

/// An export running on its thread.
struct Running {
    settings: Settings,
    pages: usize,
    done: Receiver<Result<Done, String>>,
}

/// What a finished export did.
struct Done {
    message: String,
    /// Where the file went; `None` for the clipboard.
    path: Option<PathBuf>,
    /// For the UI thread to copy, which only it can.
    clipboard: Option<String>,
}

#[derive(Default)]
pub struct Export {
    save_path: Option<SavePath>,
    dialog_open: bool,
    /// What the dialog shows; kept from one export to the next.
    settings: Settings,
    last: Option<Last>,
    running: Option<Running>,
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

    /// How many pages the running export covers, if one is running.
    pub fn running(&self) -> Option<usize> {
        self.running.as_ref().map(|r| r.pages)
    }

    /// Finishes the running export if its thread is done.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(running) = &self.running else {
            return;
        };
        let result = match running.done.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("Couldn't export: it crashed".to_owned()),
        };
        let settings = running.settings;
        self.running = None;
        self.status = Some(result.map(|done| {
            if let Some(text) = done.clipboard {
                ctx.copy_text(text);
            }
            log::info!("{}", done.message);
            self.last = Some(Last {
                settings,
                path: done.path,
            });
            done.message
        }));
    }

    /// The export dialog, if it's open. `selected` is the index of the selected page.
    pub fn dialog(
        &mut self,
        ctx: &egui::Context,
        pages: &Pages,
        selected: usize,
        params: &CleanParams,
    ) {
        if !self.dialog_open {
            return;
        }
        let settings = &mut self.settings;
        let choice = dialog::show(ctx, "export dialog", 360.0, |ui| {
            ui.heading("Export");
            ui.radio_value(&mut settings.scope, Scope::Everything, "Everything");
            ui.radio_value(&mut settings.scope, Scope::Current, "Current page");
            ui.add_space(4.0);
            ui.label("as");
            ui.radio_value(
                &mut settings.format,
                Format::MarkdownClipboard,
                "Markdown to clipboard",
            );
            ui.radio_value(
                &mut settings.format,
                Format::MarkdownFile,
                "Markdown to file",
            );
            ui.radio_value(&mut settings.format, Format::Pdf, "PDF");
            let missing = in_scope(pages, selected, settings.scope).err();
            if let Some(missing) = &missing {
                ui.colored_label(ui.visuals().warn_fg_color, not_transcribed(missing));
            }
            dialog::buttons(ui, "Go", Action::Default, missing.is_none())
        });
        if choice.is_some() {
            self.dialog_open = false;
        }
        if choice == Some(Choice::Affirmative) {
            self.run(ctx, self.settings, None, pages, selected, params);
        }
    }

    /// Repeats the last export, to the same file if it went to one.
    pub fn reexport(
        &mut self,
        ctx: &egui::Context,
        pages: &Pages,
        selected: usize,
        params: &CleanParams,
    ) {
        if let Some(Last { settings, path }) = &self.last {
            let (settings, path) = (*settings, path.clone());
            self.run(ctx, settings, path, pages, selected, params);
        }
    }

    /// Starts exporting what `settings` say, to `path` if given, else asking where.
    fn run(
        &mut self,
        ctx: &egui::Context,
        settings: Settings,
        path: Option<PathBuf>,
        pages: &Pages,
        selected: usize,
        params: &CleanParams,
    ) {
        if self.running.is_some() {
            return;
        }
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
        let path = match settings.format {
            Format::MarkdownClipboard => None,
            Format::MarkdownFile | Format::Pdf => {
                let suggested = suggested_name(&in_scope, settings.scope, settings.format);
                let path = path.or_else(|| self.save_path.as_ref().and_then(|ask| ask(&suggested)));
                if path.is_none() {
                    // Cancelled, or there's no save dialog.
                    return;
                }
                path
            }
        };
        let what = match in_scope.len() {
            1 => in_scope[0].0.to_owned(),
            n => format!("{n} pages"),
        };
        // The pages' bytes are shared, not copied.
        let snapshot: Vec<(String, Arc<[u8]>, Transcript)> = in_scope
            .iter()
            .map(|(name, transcript)| {
                let bytes = pages.find(name).expect("in scope").bytes.clone();
                (name.to_string(), bytes, (*transcript).clone())
            })
            .collect();
        let (params, ctx) = (params.clone(), ctx.clone());
        let (tx, done) = mpsc::channel();
        std::thread::spawn(move || {
            // The UI may have gone, and with it the receiver.
            tx.send(export(settings.format, &snapshot, &params, path, &what))
                .ok();
            ctx.request_repaint();
        });
        self.status = None;
        self.running = Some(Running {
            settings,
            pages: in_scope.len(),
            done,
        });
    }
}

/// Exports `pages` as `format` to `path`, or for the clipboard if there's no path. `what` names
/// the pages.
fn export(
    format: Format,
    pages: &[(String, Arc<[u8]>, Transcript)],
    params: &CleanParams,
    path: Option<PathBuf>,
    what: &str,
) -> Result<Done, String> {
    let by_name: Vec<(&str, &Transcript)> = pages.iter().map(|(n, _, t)| (n.as_str(), t)).collect();
    let markdown = || notebook_core::export::markdown(&by_name);
    let Some(path) = path else {
        return Ok(Done {
            message: format!("Copied {what} to the clipboard"),
            path: None,
            clipboard: Some(markdown()),
        });
    };
    let contents = match format {
        Format::Pdf => {
            let pdf_pages: Vec<PdfPage> = pages
                .iter()
                .map(|(name, bytes, transcript)| PdfPage {
                    name,
                    bytes,
                    transcript,
                })
                .collect();
            let markdown_name = path.with_extension("md");
            let markdown_name = markdown_name.file_name().unwrap_or_default();
            notebook_core::export::pdf(&pdf_pages, params, &markdown_name.to_string_lossy())
                .map_err(|err| format!("Couldn't export: {err}"))?
        }
        Format::MarkdownFile | Format::MarkdownClipboard => markdown().into_bytes(),
    };
    match std::fs::write(&path, contents) {
        Ok(()) => Ok(Done {
            message: format!("Exported {what} to {}", path.display()),
            path: Some(path),
            clipboard: None,
        }),
        Err(err) => Err(format!("Couldn't write {}: {err}", path.display())),
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

/// The page's name as a `format` file for one page, else a name for the notebook.
fn suggested_name(pages: &[(&str, &Transcript)], scope: Scope, format: Format) -> String {
    let extension = match format {
        Format::Pdf => "pdf",
        Format::MarkdownFile | Format::MarkdownClipboard => "md",
    };
    match (scope, pages) {
        (Scope::Current, [(name, _)]) => {
            let stem = Path::new(name).file_stem().unwrap_or_default();
            format!("{}.{extension}", stem.to_string_lossy())
        }
        _ => format!("notebook.{extension}"),
    }
}
