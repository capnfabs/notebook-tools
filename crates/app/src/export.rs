//! The export dialog, and Re-export, which repeats the last export without asking.
//!
//! Only transcribed pages can be exported, and a PDF also needs each page's photo, loaded: if any
//! page in scope falls short, the dialog says which and won't go. Markdown needs only the saved
//! transcripts. Files are chosen through a [`SavePath`], so tests can stand in for the save dialog.
//! The export itself runs on a thread, as a PDF of a whole notebook takes a moment.
//!
//! See `decisions/2026-10-23-export-rules.md` and `decisions/2026-11-17-missing-page-export.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui;
use notebook_core::export::PdfPage;
use notebook_core::page::{Page, Pages};
use notebook_core::transcript::Transcript;

use crate::t;
use crate::ui::dialog::{self, Choice, Stakes};

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
pub struct ExportOptions {
    pub scope: Scope,
    pub format: Format,
}

/// What the last export did, so Re-export can repeat it.
struct Last {
    settings: ExportOptions,
    /// Where the file went; `None` for the clipboard.
    path: Option<PathBuf>,
}

/// An export running on its thread.
struct Running {
    settings: ExportOptions,
    pages: usize,
    done: Receiver<Result<Exported, String>>,
}

/// What a finished export did.
struct Exported {
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
    settings: ExportOptions,
    last: Option<Last>,
    running: Option<Running>,
    /// What the last export did, or why it failed.
    status: Option<Result<String, String>>,
    /// The save dialog was opened since [`Self::take_asked_where`] last looked. It holds up the
    /// frame that opens it until the user closes it.
    asked_where: bool,
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

    pub fn dialog_open(&self) -> bool {
        self.dialog_open
    }

    pub fn can_reexport(&self) -> bool {
        self.last.is_some()
    }

    pub fn status(&self) -> Option<&Result<String, String>> {
        self.status.as_ref()
    }

    /// Whether the save dialog has been opened since this was last called.
    pub fn take_asked_where(&mut self) -> bool {
        std::mem::take(&mut self.asked_where)
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
            Err(TryRecvError::Disconnected) => Err(t!("export-crashed")),
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
    pub fn dialog(&mut self, ctx: &egui::Context, pages: &Pages, selected: usize) {
        if !self.dialog_open {
            return;
        }
        let settings = &mut self.settings;
        let choice = dialog::show(ctx, "export dialog", 360.0, |ui| {
            ui.heading(t!("export-title"));
            let everything = t!("export-everything");
            ui.radio_value(&mut settings.scope, Scope::Everything, everything);
            let current = t!("export-current-page");
            ui.radio_value(&mut settings.scope, Scope::Current, current);
            ui.add_space(4.0);
            ui.label(t!("export-as"));
            for (format, label) in [
                (Format::MarkdownClipboard, t!("export-markdown-clipboard")),
                (Format::MarkdownFile, t!("export-markdown-file")),
                (Format::Pdf, t!("export-pdf")),
            ] {
                ui.radio_value(&mut settings.format, format, label);
            }
            let blocked = in_scope(pages, selected, *settings).err();
            if let Some(blocked) = &blocked {
                ui.colored_label(ui.visuals().warn_fg_color, blocked);
            }
            dialog::buttons(ui, &t!("export-go"), Stakes::Safe, blocked.is_none())
        });
        if choice.is_some() {
            self.dialog_open = false;
        }
        if choice == Some(Choice::Affirmative) {
            self.run(ctx, self.settings, None, pages, selected);
        }
    }

    /// Repeats the last export, to the same file if it went to one.
    pub fn reexport(&mut self, ctx: &egui::Context, pages: &Pages, selected: usize) {
        if let Some(Last { settings, path }) = &self.last {
            let (settings, path) = (*settings, path.clone());
            self.run(ctx, settings, path, pages, selected);
        }
    }

    /// Starts exporting what `settings` say, to `path` if given, else asking where.
    fn run(
        &mut self,
        ctx: &egui::Context,
        settings: ExportOptions,
        path: Option<PathBuf>,
        pages: &Pages,
        selected: usize,
    ) {
        if self.running.is_some() {
            return;
        }
        let in_scope = match in_scope(pages, selected, settings) {
            Ok(in_scope) => in_scope,
            Err(blocked) => {
                self.status = Some(Err(t!("export-blocked", reasons = blocked)));
                return;
            }
        };
        let path = match settings.format {
            Format::MarkdownClipboard => None,
            Format::MarkdownFile | Format::Pdf => {
                let suggested = suggested_name(&in_scope, settings.scope, settings.format);
                let path = path.or_else(|| {
                    let ask = self.save_path.as_ref()?;
                    self.asked_where = true;
                    ask(&suggested)
                });
                if path.is_none() {
                    // Cancelled, or there's no save dialog.
                    return;
                }
                path
            }
        };
        // A thread and channel of its own rather than [`crate::jobs::Jobs`], so a panic shows as
        // the channel disconnecting.
        let ctx = ctx.clone();
        let (tx, done) = mpsc::channel();
        let pages = in_scope.len();
        std::thread::spawn(move || {
            // The UI may have gone, and with it the receiver.
            tx.send(export(settings.format, &in_scope, path)).ok();
            ctx.request_repaint();
        });
        self.status = None;
        self.running = Some(Running {
            settings,
            pages,
            done,
        });
    }
}

/// A page to export, as it was when the export started.
struct Snapshot {
    name: String,
    transcript: Transcript,
    /// The cleaned page, for a PDF; Markdown doesn't need it.
    image: Option<Cleaned>,
}

/// A page's cleaned image, as a PDF embeds it.
struct Cleaned {
    /// Shared with the page, not copied.
    jpeg: Arc<[u8]>,
    size: (u32, u32),
}

/// Exports `pages` as `format` to `path`, or for the clipboard if there's no path.
fn export(format: Format, pages: &[Snapshot], path: Option<PathBuf>) -> Result<Exported, String> {
    // Messages name the page when there's one.
    let (count, name) = (pages.len(), pages.first().map_or("", |p| p.name.as_str()));
    let by_name: Vec<(&str, &Transcript)> = pages
        .iter()
        .map(|p| (p.name.as_str(), &p.transcript))
        .collect();
    let markdown = || notebook_core::export::markdown(&by_name);
    let Some(path) = path else {
        return Ok(Exported {
            message: t!("export-copied", count = count, name = name),
            path: None,
            clipboard: Some(markdown()),
        });
    };
    let contents = match format {
        Format::Pdf => {
            // [`in_scope`] gives every page its image for a PDF.
            let pdf_pages = pages
                .iter()
                .map(|p| {
                    let image = p.image.as_ref().ok_or_else(|| {
                        t!(
                            "export-blocked",
                            reasons = t!("export-needs-relink", names = &p.name)
                        )
                    })?;
                    Ok(PdfPage {
                        name: &p.name,
                        jpeg: &image.jpeg,
                        size: image.size,
                        transcript: &p.transcript,
                    })
                })
                .collect::<Result<Vec<PdfPage>, String>>()?;
            let markdown_name = path.with_extension("md");
            let markdown_name = markdown_name.file_name().unwrap_or_default();
            notebook_core::export::pdf(&pdf_pages, &markdown_name.to_string_lossy())
                .map_err(|err| t!("export-failed", error = crate::i18n::pdf_error(&err)))?
        }
        Format::MarkdownFile | Format::MarkdownClipboard => markdown().into_bytes(),
    };
    match std::fs::write(&path, contents) {
        Ok(()) => Ok(Exported {
            message: t!(
                "export-saved",
                count = count,
                name = name,
                path = path.display().to_string()
            ),
            path: Some(path),
            clipboard: None,
        }),
        Err(err) => Err(t!(
            "export-write-failed",
            path = path.display().to_string(),
            error = err.to_string()
        )),
    }
}

/// The pages `settings` cover, or why they can't be exported. Each page that can't is named once:
/// for a PDF, under its missing photo (and transcript), else as untranscribed or still loading;
/// for Markdown, only as untranscribed. See `decisions/2026-11-17-missing-page-export.md`.
fn in_scope(
    pages: &Pages,
    selected: usize,
    settings: ExportOptions,
) -> Result<Vec<Snapshot>, String> {
    let covered: Vec<&Page> = match settings.scope {
        Scope::Current => pages.get(selected).into_iter().collect(),
        Scope::Everything => pages.iter().collect(),
    };
    let pdf = settings.format == Format::Pdf;
    let names = |blocks: &dyn Fn(&Page) -> bool| -> Vec<&str> {
        let names = covered.iter().filter(|p| blocks(p));
        names.map(|p| p.name.as_str()).collect()
    };
    let missing = |p: &Page| pdf && p.missing_photo().is_some();
    let untranscribed = |p: &Page| p.transcript.is_none();
    let mut blocked = Vec::new();
    let mut add = |names: Vec<&str>, message: fn(String) -> String| {
        if !names.is_empty() {
            blocked.push(message(names.join(", ")));
        }
    };
    add(names(&|p| missing(p) && !untranscribed(p)), |names| {
        t!("export-needs-relink", names = names)
    });
    add(names(&|p| missing(p) && untranscribed(p)), |names| {
        t!("export-needs-relink-and-transcribe", names = names)
    });
    add(names(&|p| !missing(p) && untranscribed(p)), |names| {
        t!("export-not-transcribed", names = names)
    });
    add(
        names(&|p| pdf && p.missing_photo().is_none() && p.loaded.is_none()),
        |names| t!("export-still-loading", names = names),
    );
    if covered.is_empty() {
        blocked.push(t!("export-no-pages"));
    }
    if !blocked.is_empty() {
        return Err(blocked.join(". "));
    }
    Ok(covered
        .into_iter()
        .filter_map(|p| {
            let transcript = p.transcript.as_ref()?;
            if !pdf {
                return Some(Snapshot {
                    name: p.name.clone(),
                    transcript: transcript.clone(),
                    image: None,
                });
            }
            let loaded = p.loaded.as_ref()?;
            Some(Snapshot {
                name: p.name.clone(),
                // Boxes follow the page if its corners moved since it was transcribed, staying
                // upright: `decisions/2026-10-30-pdf-text-upright-boxes.md`.
                transcript: transcript.reframed(loaded.report.crop()),
                image: Some(Cleaned {
                    jpeg: loaded.cleaned_jpeg.clone(),
                    size: loaded.report.output_size,
                }),
            })
        })
        .collect())
}

/// The page's name as a `format` file for one page, else a name for the notebook.
fn suggested_name(pages: &[Snapshot], scope: Scope, format: Format) -> String {
    let extension = match format {
        Format::Pdf => "pdf",
        Format::MarkdownFile | Format::MarkdownClipboard => "md",
    };
    match (scope, pages) {
        (Scope::Current, [page]) => {
            let stem = Path::new(&page.name).file_stem().unwrap_or_default();
            format!("{}.{extension}", stem.to_string_lossy())
        }
        _ => format!("{}.{extension}", t!("export-default-name")),
    }
}
